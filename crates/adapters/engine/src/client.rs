//! The daemon side: spawn a worker per operation and translate its events.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use api::{ErrorCode, WarningCode};
use app::ports::{
    BackupSummary, CheckSummary, Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob,
    ForgetJob, ForgetSummary, Job, JobControl, MAX_JOB_IDS, ProgressEvent,
    ProgressKind as AppProgressKind, PruneSummary, RepositoryAccess, RewriteJob, RewriteSummary,
    RunningJob, SnapshotSummary,
};
use async_trait::async_trait;
use domain::execution::{FailureReason, Warning, WarningKind};
use domain::secrets::{Redactor, RepositorySecrets, SecretValue};
use domain::{OperationId, RepositoryId};
use futures::stream::{self, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::{mpsc, watch};

use crate::cache_lock::CacheLock;
use crate::environment;
use crate::job::{
    BackupJobSpec, CheckJobSpec, FakeEndSpec, FakeJobSpec, FakeWarning, ForgetJobSpec, JobKind,
    JobSpec, Level, MAX_LISTED_BYTES, MAX_LISTED_SNAPSHOTS, MAX_MESSAGE_BYTES, OpResult,
    ProgressKind, PruneJobSpec, RepositorySpec, RewriteJobSpec, WorkerEvent, exit_code,
};

/// How long a worker may keep running after it reported its result, for
/// cleanup, before it is killed.
const AFTER_RESULT_GRACE: Duration = Duration::from_secs(10);

/// How long the events of a job whose worker has exited may go on arriving.
/// What the worker wrote is in the pipe already, so this is only the time to
/// read it: a process the worker left running can hold the pipe open for as
/// long as it lives, and the events end with the worker, not with it.
const DRAIN_AFTER_EXIT: Duration = Duration::from_secs(1);

/// How long a new worker may take to take its job, which it shows with its
/// first line of output, before it counts as hung and is killed: the default
/// of [`WorkerEngine::with_start_timeout`].
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a worker that closed its output may go on living before it is
/// killed: the default of [`WorkerEngine::with_exit_timeout`].
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most cache slots one repository gets. Looking for a free one stops
/// here, so a lock that is wrongly always held can't send the search on to
/// the end of the numbers.
const MAX_CACHE_SLOTS: u32 = 64;

/// Runs each operation in a new worker process: `program args…`, normally
/// `ladderback worker`. A worker leads a process group of its own, and what is
/// done to the worker is done to the group, so the programs it started (an
/// rclone, say) end with it.
///
/// Stopping a job sends SIGTERM and, after a grace period, SIGKILL. Dropping
/// a [`RunningJob`] before its result kills the worker at once; after the
/// result, the worker gets a grace period to finish its cleanup.
///
/// A worker that hangs doesn't hold the daemon. One that doesn't take its
/// job within the start timeout, and one that closes its output but is still
/// there after the exit timeout, are killed.
///
/// A worker starts with a scrubbed environment (invariant I4): only the
/// variables that name a place or a setting the worker, rclone or ssh need
/// (`PATH`, `HOME`, `SSH_AUTH_SOCK`, the proxy variables and the like), and
/// none of the rest of the daemon's, which may hold credentials. The base is
/// this process's environment, read at each job, unless
/// [`WorkerEngine::with_environment`] gives another.
///
/// What the worker writes to stderr is logged with the job's secrets masked,
/// for the programs it starts write there too, unmasked.
pub struct WorkerEngine {
    program: PathBuf,
    args: Vec<OsString>,
    environment: Option<Vec<(OsString, OsString)>>,
    cache_root: Option<PathBuf>,
    stop_grace: Duration,
    watchdog_interval: Duration,
    start_timeout: Duration,
    exit_timeout: Duration,
    slots: Arc<CacheSlots>,
}

impl WorkerEngine {
    pub fn new(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            environment: None,
            cache_root: None,
            stop_grace: Duration::from_secs(10),
            watchdog_interval: Duration::from_secs(5),
            start_timeout: START_TIMEOUT,
            exit_timeout: EXIT_TIMEOUT,
            slots: Arc::new(CacheSlots::default()),
        }
    }

    /// Gives each worker of a repository with a known ID its own rustic cache
    /// under `root/ladderback/<repository id>/<slot>`. Without a root, workers
    /// run without a cache.
    #[must_use]
    pub fn with_cache_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.cache_root = Some(root.into());
        self
    }

    /// How long [`JobControl::stop`] waits after SIGTERM before SIGKILL.
    #[must_use]
    pub fn with_stop_grace(mut self, grace: Duration) -> Self {
        self.stop_grace = grace;
        self
    }

    /// The longest a worker's deadline watchdog sleeps between two looks at
    /// the wall clock.
    #[must_use]
    pub fn with_watchdog_interval(mut self, interval: Duration) -> Self {
        self.watchdog_interval = interval;
        self
    }

    /// How long a new worker has to take its job, 30 seconds by default: one
    /// deadline, counted from the start of the write of the job to its stdin
    /// and ending with the worker's first line of output. A worker that
    /// doesn't read its job, because it hung before it got to it, leaves the
    /// write blocked once the pipe is full, which a job spec larger than the
    /// pipe's buffer (64 KiB on Linux) does. A worker that took its job and
    /// then says nothing is the other case. A worker says it has its job with
    /// a debug `log` line, written once the spec is read and before anything
    /// else is done, so what follows is not on this clock: opening a
    /// repository, which a slow backend can make take longer than the
    /// timeout, is not bounded by it. A line of any kind counts, and a line is
    /// there when its newline is.
    ///
    /// When the time has passed, the worker and what it started are killed.
    /// A write still blocked then fails [`Engine::run`] with
    /// [`EngineError::StartTimedOut`]. A worker that has taken its job is
    /// not waited for in `run`, which would hold back the job's control and
    /// so a user's cancel: its events are one
    /// `Failed(EngineStartTimedOut)` and end. Once the first line has come
    /// the worker is not on this clock any more, however quiet it goes. A
    /// smaller spec is buffered at once, whether or not the worker reads it,
    /// so only the wait for the first line covers it; a write that fails is
    /// [`EngineError::Unavailable`], as ever. A timeout too long to
    /// represent, such as [`Duration::MAX`], never passes.
    #[must_use]
    pub fn with_start_timeout(mut self, timeout: Duration) -> Self {
        self.start_timeout = timeout;
        self
    }

    /// How long a worker that has closed its output may go on living, 10
    /// seconds by default. Its events end with its output, and the daemon
    /// waits for its exit to tell how it ended. A worker still there when the
    /// time has passed is killed, with what it started, and the events end
    /// with `Failed(EngineExitTimedOut)`. One that exits in time is reported
    /// by its exit, and a job that is being stopped ends its events without a
    /// failure, as ever.
    #[must_use]
    pub fn with_exit_timeout(mut self, timeout: Duration) -> Self {
        self.exit_timeout = timeout;
        self
    }

    /// Starts workers with `vars` as the environment they are scrubbed from
    /// (see [`environment`]) instead of this process's own. For tests, and
    /// for a composition root that has read its environment already.
    #[must_use]
    pub fn with_environment(
        mut self,
        vars: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>,
    ) -> Self {
        self.environment = Some(
            vars.into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        );
        self
    }

    /// The environment workers are scrubbed from: the one given, or else
    /// this process's own as it is now.
    fn base_environment(&self) -> Vec<(OsString, OsString)> {
        self.environment
            .clone()
            .unwrap_or_else(|| std::env::vars_os().collect())
    }
}

#[async_trait]
impl Engine for WorkerEngine {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let slot = match (&self.cache_root, repository_id(&job)) {
            (Some(root), Some(id)) => {
                let (slots, root, id) = (Arc::clone(&self.slots), root.clone(), id.clone());
                // Looking at the slots' locks is file I/O.
                tokio::task::spawn_blocking(move || slots.acquire(&root, &id))
                    .await
                    .unwrap_or(None)
            }
            _ => None,
        };
        // What the worker starts with, and so what its proxy settings are:
        // a proxy URL's credentials are masked like the job's own secrets.
        let environment = environment::scrubbed(self.base_environment());
        let redactor = job_redactor(&job, environment::proxy_credentials(&environment));
        let spec = serde_json::to_vec(&JobSpec {
            op_id: op,
            cache_dir: slot.as_ref().map(|slot| slot.dir.clone()),
            watchdog_interval_ms: u64::try_from(self.watchdog_interval.as_millis())
                .unwrap_or(u64::MAX),
            job: job_kind(job)?,
        })
        .map_err(|e| failed_to_start(&self.program, e))?;
        // The worker refuses a longer spec, so don't start one to be refused.
        if spec.len() > MAX_MESSAGE_BYTES {
            return Err(EngineError::Unavailable {
                detail: format!("job spec exceeds {MAX_MESSAGE_BYTES} bytes"),
            });
        }
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            // Nothing the daemon holds in its environment goes to a worker
            // that doesn't need it: see `environment`.
            .env_clear()
            .envs(environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Piped, not inherited: the worker leads a process group of its
            // own, which a terminal with `stty tostop` treats as a background
            // job, and a write to the terminal would suspend it.
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // The worker leads a process group of its own: see `signal_group`.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|e| failed_to_start(&self.program, e))?;
        let Some(mut stdin) = child.stdin.take() else {
            return Err(failed_to_start(&self.program, "the worker has no stdin"));
        };
        // The deadline for the job to be taken and answered: see
        // `Reading::first_line_by`. `None` is a timeout too long to
        // represent, which never passes.
        let first_line_by = tokio::time::Instant::now().checked_add(self.start_timeout);
        // A worker that never reads its job, because it hung before it got to
        // it, leaves this write blocked once the pipe is full.
        let write = stdin.write_all(&spec);
        let written = match first_line_by {
            Some(at) => tokio::time::timeout_at(at, write).await,
            None => Ok(write.await),
        };
        match written {
            Ok(written) => written.map_err(|e| failed_to_start(&self.program, e))?,
            Err(_) => {
                // The worker is not waited for: nothing here may be able to
                // hold `run` back. The runtime reaps the dropped child, and
                // the cache slot is free again as `slot` goes out of scope.
                kill_now(&mut child);
                return Err(EngineError::StartTimedOut);
            }
        }
        // Closing stdin tells the worker the job spec is complete.
        drop(stdin);
        let Some(stdout) = child.stdout.take() else {
            return Err(failed_to_start(&self.program, "the worker has no stdout"));
        };
        let Some(stderr) = child.stderr.take() else {
            return Err(failed_to_start(&self.program, "the worker has no stderr"));
        };

        let (commands, command_rx) = mpsc::unbounded_channel();
        let (exited_tx, exited) = watch::channel(None);
        tokio::spawn(supervise(
            child,
            command_rx,
            exited_tx,
            self.stop_grace,
            slot,
        ));
        tokio::spawn(forward_stderr(
            BufReader::new(stderr),
            exited.clone(),
            redactor.clone(),
        ));
        let stopped = Arc::new(AtomicBool::new(false));
        let reading = Reading {
            stdout: BufReader::new(stdout),
            line: Vec::new(),
            exited: exited.clone(),
            commands: commands.clone(),
            stopped: Arc::clone(&stopped),
            done: false,
            drain_until: None,
            redactor,
            exit_timeout: self.exit_timeout,
            first_line_by,
            listing: Listing::default(),
        };
        Ok(RunningJob {
            events: stream::unfold(reading, Reading::next).boxed(),
            control: Box::new(Control {
                commands,
                exited,
                stopped,
            }),
        })
    }
}

fn failed_to_start(program: &Path, error: impl std::fmt::Display) -> EngineError {
    EngineError::Unavailable {
        detail: format!("{}: {error}", program.display()),
    }
}

/// What the daemon asks of the task that owns the worker process.
enum Order {
    /// SIGTERM, then SIGKILL after the grace period.
    Stop,
    /// SIGKILL at once: the worker closed its output and lives on, or sent
    /// nothing in the time it had to start.
    Kill,
    /// The worker reported its result and may finish its cleanup.
    ResultReported,
}

/// The exit code of a worker that ended: `None` when a signal ended it.
type Exited = Option<Option<i32>>;

/// Owns the worker process, and the cache slot it uses, until it exits.
async fn supervise(
    mut child: Child,
    commands: mpsc::UnboundedReceiver<Order>,
    exited: watch::Sender<Exited>,
    grace: Duration,
    _slot: Option<CacheSlot>,
) {
    let mut commands = Some(commands);
    let mut after_result = None;
    let leader = child.id();
    // The exit is seen without reaping the worker where that can be done:
    // see `WorkerExit`. Elsewhere the worker is reaped first.
    let gone = exit_unreaped(leader);
    tokio::pin!(gone);
    let mut unreaped_exit = true;
    let status = loop {
        let cleanup_over = async {
            match after_result {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            exit = &mut gone, if unreaped_exit => match exit {
                WorkerExit::Unreaped => break reap(&mut child, leader, true).await,
                WorkerExit::Reaped => break reap(&mut child, leader, false).await,
                WorkerExit::Unobservable => unreaped_exit = false,
            },
            status = child.wait(), if !unreaped_exit => {
                signal_group(leader, Signal::Kill);
                break status;
            }
            command = next_command(&mut commands) => match command {
                Some(Order::Stop) => break terminate(&mut child, grace).await,
                Some(Order::Kill) => break kill(&mut child).await,
                Some(Order::ResultReported) => {
                    after_result = Some(deadline_after(tokio::time::Instant::now(), AFTER_RESULT_GRACE));
                }
                // The daemon let go of the job. Before a result that means
                // kill; after it, the worker finishes its cleanup first.
                None if after_result.is_some() => commands = None,
                None => break kill(&mut child).await,
            },
            () = cleanup_over => break kill(&mut child).await,
        }
    };
    let _ = exited.send(Some(status.ok().and_then(|status| status.code())));
}

async fn next_command(commands: &mut Option<mpsc::UnboundedReceiver<Order>>) -> Option<Order> {
    match commands {
        Some(commands) => commands.recv().await,
        None => std::future::pending().await,
    }
}

/// When a worker that reported its result must be done with its cleanup:
/// `grace` after `now`, or `now` when that time can't be represented.
fn deadline_after(now: tokio::time::Instant, grace: Duration) -> tokio::time::Instant {
    now.checked_add(grace).unwrap_or(now)
}

/// Kills the worker and everything it started, and waits for the worker to
/// exit.
async fn kill(child: &mut Child) -> std::io::Result<std::process::ExitStatus> {
    kill_now(child);
    child.wait().await
}

/// SIGKILL to the worker and everything it started, without waiting for the
/// worker to exit.
fn kill_now(child: &mut Child) {
    if !signal_group(child.id(), Signal::Kill) {
        // No process group to signal: the worker alone.
        let _ = child.start_kill();
    }
}

/// SIGTERM to the worker and everything it started, then SIGKILL once `grace`
/// has passed without the worker exiting. What of the group is left when the
/// worker has gone, a process that ignored the SIGTERM say, is killed then.
async fn terminate(
    child: &mut Child,
    grace: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    let leader = child.id();
    if !signal_group(leader, Signal::Terminate) {
        return kill(child).await;
    }
    let over = deadline_after(tokio::time::Instant::now(), grace);
    match tokio::time::timeout_at(over, exit_unreaped(leader)).await {
        Ok(WorkerExit::Unreaped) => return reap(child, leader, true).await,
        Ok(WorkerExit::Reaped) => return reap(child, leader, false).await,
        Ok(WorkerExit::Unobservable) => {}
        Err(_) => return kill(child).await,
    }
    match tokio::time::timeout_at(over, child.wait()).await {
        Ok(status) => {
            // Reaped already: see `WorkerExit::Unobservable`.
            signal_group(leader, Signal::Kill);
            status
        }
        Err(_) => kill(child).await,
    }
}

/// Reaps a worker that has exited, after SIGKILL to what it started and left
/// running: rustic's rclone is not stopped by the worker's `process::exit`.
/// `unreaped` tells whether the worker is still a zombie. A zombie holds its
/// process id, so the id is its group's still and nobody else's, and the
/// group may be signaled by it. Once the worker is reaped the id is free to
/// be given to another process, which a signal would reach.
async fn reap(
    child: &mut Child,
    leader: Option<u32>,
    unreaped: bool,
) -> std::io::Result<std::process::ExitStatus> {
    if unreaped {
        signal_group(leader, Signal::Kill);
    }
    child.wait().await
}

/// What a wait for the worker's exit, without reaping it, found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(all(target_os = "linux", not(target_env = "uclibc"))),
    expect(dead_code, reason = "only Linux can see an exit without reaping")
)]
enum WorkerExit {
    /// The worker exited and is a zombie still, holding its process id.
    Unreaped,
    /// Somebody reaped the worker first.
    Reaped,
    /// The exit can't be seen without reaping here. nix 0.31.3 has `waitid`
    /// on Linux only; on macOS the safe alternative, a kqueue `NOTE_EXIT`, is
    /// not known to report a process that exited before it was registered.
    /// A sandbox may refuse `waitid` too.
    ///
    /// The worker is then waited for by `Child::wait`, which reaps it, and
    /// its group is signaled by the id it had. POSIX keeps a process id from
    /// being reused while a process group of that id has a live member, so
    /// when the worker left a process running, which is what the signal is
    /// for, it can only reach that group. The residual window is this: the
    /// group has no live member, and in the instants between the reaping and
    /// the signal a new process takes the id and makes itself a group
    /// leader. The signal then reaches that stranger.
    Unobservable,
}

/// What one `waitid(WEXITED | WNOWAIT)` found; `None` when interrupted.
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
fn worker_exit(waited: nix::Result<nix::sys::wait::WaitStatus>) -> Option<WorkerExit> {
    use nix::errno::Errno;

    match waited {
        // The arguments are fixed and valid, so EINVAL comes from nix after
        // libc's `waitid` succeeded: a status it can't convert, such as a
        // real-time signal. The worker exited all the same.
        Ok(_) | Err(Errno::EINVAL) => Some(WorkerExit::Unreaped),
        Err(Errno::ECHILD) => Some(WorkerExit::Reaped),
        Err(Errno::EINTR) => None,
        Err(_) => Some(WorkerExit::Unobservable),
    }
}

/// Resolves once the worker has exited, *without reaping it*: `waitid` with
/// `WNOWAIT` leaves the worker a zombie that holds its process id, which
/// `Child::wait` would free. The wait is a thread of its own, one for each
/// running worker, and ends with the worker.
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn exit_unreaped(leader: Option<u32>) -> WorkerExit {
    use nix::sys::wait::{Id, WaitPidFlag, waitid};
    use nix::unistd::Pid;

    let Some(pid) = leader.and_then(|pid| i32::try_from(pid).ok()) else {
        return WorkerExit::Unobservable;
    };
    let waiting = tokio::task::spawn_blocking(move || {
        loop {
            let flags = WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT;
            if let Some(exit) = worker_exit(waitid(Id::Pid(Pid::from_raw(pid)), flags)) {
                return exit;
            }
        }
    });
    waiting.await.unwrap_or(WorkerExit::Unobservable)
}

/// See [`WorkerExit::Unobservable`].
#[cfg(not(all(target_os = "linux", not(target_env = "uclibc"))))]
async fn exit_unreaped(_leader: Option<u32>) -> WorkerExit {
    WorkerExit::Unobservable
}

#[derive(Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

/// Signals the process group the worker leads: the worker and every process
/// it started that didn't leave the group. `leader` is the worker's process
/// id, `None` once the worker was reaped, and nothing is signaled then: the
/// number may belong to somebody else by now. Whether the signal was sent;
/// off Unix there are no process groups.
#[cfg(unix)]
fn signal_group(leader: Option<u32>, signal: Signal) -> bool {
    use nix::sys::signal::{Signal as Sig, killpg};
    use nix::unistd::Pid;

    let signal = match signal {
        Signal::Terminate => Sig::SIGTERM,
        Signal::Kill => Sig::SIGKILL,
    };
    leader
        .and_then(|pid| i32::try_from(pid).ok())
        .is_some_and(|pid| killpg(Pid::from_raw(pid), signal).is_ok())
}

#[cfg(not(unix))]
fn signal_group(_leader: Option<u32>, _signal: Signal) -> bool {
    false
}

/// Stops one job.
struct Control {
    commands: mpsc::UnboundedSender<Order>,
    exited: watch::Receiver<Exited>,
    stopped: Arc<AtomicBool>,
}

#[async_trait]
impl JobControl for Control {
    async fn stop(&mut self) {
        // The job's events end here, whatever the worker still writes.
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Order::Stop);
        let _ = self.exited.wait_for(Option::is_some).await;
    }
}

/// The events of one job, read from the worker's stdout.
struct Reading {
    stdout: BufReader<ChildStdout>,
    line: Vec<u8>,
    exited: watch::Receiver<Exited>,
    commands: mpsc::UnboundedSender<Order>,
    stopped: Arc<AtomicBool>,
    done: bool,
    /// When the output of a worker that has exited is no longer waited for.
    drain_until: Option<tokio::time::Instant>,
    /// What masks the job's secrets in what the worker says.
    redactor: Redactor,
    /// How long the exit of a worker that closed its output is waited for.
    exit_timeout: Duration,
    /// When the worker's first complete line is due, until it has come: the
    /// start timeout counted from the start of the write of the job.
    /// `None` once a line has come, or for a timeout that can't be
    /// represented.
    first_line_by: Option<tokio::time::Instant>,
    /// The listing the worker has sent so far, when the job is one.
    listing: Listing,
}

/// A repository's listing as it arrives: the `snapshots` events of a worker
/// until the result that closes it. The list is held up to
/// [`MAX_LISTED_SNAPSHOTS`] and no further, and it is handed on only once
/// the closing result's count agrees with it: a list that lost a batch is
/// never taken for the whole.
#[derive(Default)]
struct Listing {
    snapshots: Vec<SnapshotSummary>,
    /// What the snapshots held so far weigh, as [`weight`] counts it.
    bytes: usize,
}

/// What a listed snapshot costs the daemon to hold: its texts, and a fixed
/// amount for the rest.
fn weight(snapshot: &SnapshotSummary) -> usize {
    let texts = snapshot.hostname.len()
        + snapshot.label.len()
        + snapshot.tags.iter().map(String::len).sum::<usize>()
        + snapshot.paths.iter().map(String::len).sum::<usize>();
    texts.saturating_add(256)
}

/// What a [`Listing`] made of one event of the worker's.
enum Absorbed {
    /// A batch, kept; nothing to tell the job's owner yet.
    Kept,
    /// The listing's end: the whole list.
    Complete(Vec<SnapshotSummary>),
    /// A listing that can't be taken, and why.
    Refused(String),
    /// Not part of a listing.
    Passed(WorkerEvent),
}

impl Listing {
    fn absorb(&mut self, event: WorkerEvent) -> Absorbed {
        match event {
            WorkerEvent::Snapshots { snapshots } => {
                if self.snapshots.len().saturating_add(snapshots.len()) > MAX_LISTED_SNAPSHOTS {
                    return Absorbed::Refused(format!(
                        "worker listing longer than {MAX_LISTED_SNAPSHOTS} snapshots"
                    ));
                }
                let batch: Vec<SnapshotSummary> = snapshots
                    .into_iter()
                    .map(|s| SnapshotSummary {
                        id: s.id,
                        time: s.time,
                        hostname: s.hostname,
                        label: s.label,
                        tags: s.tags,
                        paths: s.paths,
                        paths_truncated: s.paths_truncated,
                        tags_truncated: s.tags_truncated,
                    })
                    .collect();
                let bytes = batch
                    .iter()
                    .fold(self.bytes, |all, s| all.saturating_add(weight(s)));
                if bytes > MAX_LISTED_BYTES {
                    return Absorbed::Refused(format!(
                        "worker listing larger than {MAX_LISTED_BYTES} bytes"
                    ));
                }
                self.bytes = bytes;
                self.snapshots.extend(batch);
                Absorbed::Kept
            }
            WorkerEvent::Result(OpResult::Snapshots { count }) => {
                let snapshots = std::mem::take(&mut self.snapshots);
                self.bytes = 0;
                if u64::try_from(snapshots.len()).is_ok_and(|held| held == count) {
                    Absorbed::Complete(snapshots)
                } else {
                    Absorbed::Refused(format!(
                        "worker listing ended at {count} snapshots after sending {}",
                        snapshots.len()
                    ))
                }
            }
            other => Absorbed::Passed(other),
        }
    }
}

impl Reading {
    async fn next(mut self) -> Option<(EngineEvent, Self)> {
        if self.done {
            return None;
        }
        loop {
            self.line.clear();
            let read = self.read_next_line().await;
            if self.stopped.load(Ordering::SeqCst) {
                return None;
            }
            match read {
                Ok(Line::Complete) => self.first_line_by = None,
                // No line, not even a `log` one, by the time it was due: the
                // worker is killed without waiting for the job's control,
                // which may be held for as long as its owner likes.
                Ok(Line::Silent) => {
                    self.done = true;
                    let _ = self.commands.send(Order::Kill);
                    return Some((
                        EngineEvent::Failed(FailureReason::EngineStartTimedOut),
                        self,
                    ));
                }
                // A line without its newline was cut off by a worker that
                // died mid-write, say at its deadline.
                Ok(Line::Cut) => return self.exited().await,
                Ok(Line::TooLong) => {
                    return Some((
                        self.broken(format!("worker event longer than {MAX_EVENT_LINE} bytes")),
                        self,
                    ));
                }
                Err(error) => {
                    return Some((self.broken(format!("reading worker output: {error}")), self));
                }
            }
            let event = match serde_json::from_slice::<WorkerEvent>(&self.line) {
                Ok(event) => event,
                Err(error) => {
                    return Some((
                        self.broken(format!("malformed worker event: {error}")),
                        self,
                    ));
                }
            };
            let event = match self.listing.absorb(event) {
                Absorbed::Kept => continue,
                Absorbed::Complete(snapshots) => {
                    self.done = true;
                    let _ = self.commands.send(Order::ResultReported);
                    return Some((
                        EngineEvent::Succeeded(EngineResult::Snapshots(snapshots)),
                        self,
                    ));
                }
                Absorbed::Refused(detail) => return Some((self.broken(detail), self)),
                Absorbed::Passed(event) => event,
            };
            if let Some(event) = translate(event, &self.redactor) {
                if matches!(event, EngineEvent::Succeeded(_) | EngineEvent::Failed(_)) {
                    self.done = true;
                    let _ = self.commands.send(Order::ResultReported);
                }
                return Some((event, self));
            }
        }
    }

    /// Reads the next line of the worker's output into `self.line`.
    ///
    /// While the worker runs, the wait is as long as it takes, but for the
    /// first line, which is due by [`Reading::first_line_by`]. Once the
    /// worker has exited, what it wrote is in the pipe already, so reading
    /// goes on for that, but for [`DRAIN_AFTER_EXIT`] in all: nothing is
    /// waited for on a pipe that another process holds open.
    async fn read_next_line(&mut self) -> std::io::Result<Line> {
        let Self {
            stdout,
            line,
            exited,
            drain_until,
            first_line_by,
            ..
        } = self;
        let reading = read_line(stdout, line);
        tokio::pin!(reading);
        let silent = async {
            match *first_line_by {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            // What is in the pipe goes before the worker's exit, and a
            // worker that has exited is not a silent one: its exit is told
            // by what follows.
            biased;
            read = &mut reading => return read,
            () = worker_exited(exited) => {}
            () = silent => return Ok(Line::Silent),
        }
        let until = *drain_until
            .get_or_insert_with(|| deadline_after(tokio::time::Instant::now(), DRAIN_AFTER_EXIT));
        // A pipe that stays quiet is an ended one: the cut line, if any, is
        // reported as a worker that died mid-write.
        match tokio::time::timeout_at(until, reading).await {
            Ok(read) => read,
            Err(_) => Ok(Line::Cut),
        }
    }

    /// The worker closed stdout without a result: it crashed, was killed, its
    /// deadline watchdog stopped it, or it lives on. Its exit is waited for
    /// the exit timeout and no longer: a worker still there then is killed,
    /// and the failure says so.
    async fn exited(mut self) -> Option<(EngineEvent, Self)> {
        self.done = true;
        // `None`: the worker was still there when the time was up. The wait
        // is a scope of its own: what it borrows from `self.exited` is let go
        // before `self` moves on.
        let exit = {
            let waited =
                tokio::time::timeout(self.exit_timeout, self.exited.wait_for(Option::is_some))
                    .await;
            match waited {
                Ok(Ok(exit)) => Some((*exit).flatten()),
                // The task that owns the worker is gone, and the worker with it.
                Ok(Err(_)) => return None,
                Err(_) => None,
            }
        };
        if self.stopped.load(Ordering::SeqCst) {
            return None;
        }
        let reason = match exit {
            Some(Some(exit_code::DEADLINE_PASSED)) => FailureReason::DeadlinePassed,
            Some(exit_code) => FailureReason::EngineExited { exit_code },
            None => {
                // The worker is killed without waiting for the job's control,
                // which may be held for as long as its owner likes.
                let _ = self.commands.send(Order::Kill);
                FailureReason::EngineExitTimedOut
            }
        };
        Some((EngineEvent::Failed(reason), self))
    }

    fn broken(&mut self, detail: String) -> EngineEvent {
        // Dropping the job kills the worker. What a parser says of the line
        // it couldn't read can quote it.
        self.done = true;
        EngineEvent::Failed(FailureReason::EngineFailed {
            detail: self.redactor.redact(&detail),
        })
    }
}

/// The longest line of events a worker may write: the protocol's message
/// cap. A longer line is a worker gone wrong, and is never read into memory.
const MAX_EVENT_LINE: usize = crate::job::MAX_MESSAGE_BYTES;

enum Line {
    Complete,
    /// The output ended before the line's newline.
    Cut,
    TooLong,
    /// No line was complete by the time the first was due.
    Silent,
}

/// Reads one line, newline included, into `line`, holding at most
/// [`MAX_EVENT_LINE`] bytes of it before the newline.
async fn read_line(
    reader: &mut BufReader<ChildStdout>,
    line: &mut Vec<u8>,
) -> std::io::Result<Line> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(Line::Cut);
        }
        let (taken, complete) = match available.iter().position(|&byte| byte == b'\n') {
            Some(newline) => (newline.saturating_add(1), true),
            None => (available.len(), false),
        };
        let content = if complete {
            taken.saturating_sub(1)
        } else {
            taken
        };
        if line.len().saturating_add(content) > MAX_EVENT_LINE {
            return Ok(Line::TooLong);
        }
        line.extend_from_slice(available.get(..taken).unwrap_or(available));
        reader.consume(taken);
        if complete {
            return Ok(Line::Complete);
        }
    }
}

/// Resolves once the worker has exited.
async fn worker_exited(exited: &mut watch::Receiver<Exited>) {
    let _ = exited.wait_for(Option::is_some).await;
}

/// The longest line of the worker's stderr that is logged.
const MAX_LOG_LINE: usize = crate::job::MAX_MESSAGE_BYTES;

/// Logs what the worker writes to stderr, a line at a time, as warnings: its
/// panic report, and whatever a library of its says there. It runs beside the
/// events, which a worker that writes a lot to stderr can't hold up, and ends
/// like they do, [`DRAIN_AFTER_EXIT`] after the worker.
async fn forward_stderr(
    mut stderr: BufReader<ChildStderr>,
    mut exited: watch::Receiver<Exited>,
    redactor: Redactor,
) {
    let mut line = Vec::new();
    let mut drain_until = None;
    loop {
        line.clear();
        match next_log_line(&mut stderr, &mut line, &mut exited, &mut drain_until).await {
            Ok(Some(cut)) => {
                if !line.is_empty() || cut {
                    log::log!(
                        target: "ladderback::worker",
                        log::Level::Warn,
                        "{}{}",
                        redacted_log_line(&line, cut, &redactor),
                        if cut { "…" } else { "" }
                    );
                }
            }
            Ok(None) | Err(_) => return,
        }
    }
}

/// One line of stderr with the job's secrets masked. A line cut at the cap
/// first loses as many bytes of its end as the longest secret has, less
/// one: a secret that straddles the cut can't leave its first characters
/// behind, for the mask only finds what is whole. (The worker does the
/// equivalent by masking before it cuts.)
fn redacted_log_line(line: &[u8], cut: bool, redactor: &Redactor) -> String {
    let kept = if cut {
        line.len()
            .saturating_sub(redactor.longest_match().saturating_sub(1))
    } else {
        line.len()
    };
    let text = String::from_utf8_lossy(line.get(..kept).unwrap_or(line));
    redactor.redact(&text)
}

/// The next line of the worker's stderr: see [`read_log_line`]. Once the
/// worker has exited, what it wrote is read, but not for longer than
/// [`DRAIN_AFTER_EXIT`] in all: another process may hold the pipe open.
async fn next_log_line(
    stderr: &mut BufReader<ChildStderr>,
    line: &mut Vec<u8>,
    exited: &mut watch::Receiver<Exited>,
    drain_until: &mut Option<tokio::time::Instant>,
) -> std::io::Result<Option<bool>> {
    let reading = read_log_line(stderr, line);
    tokio::pin!(reading);
    tokio::select! {
        biased;
        read = &mut reading => return read,
        () = worker_exited(exited) => {}
    }
    let until = *drain_until
        .get_or_insert_with(|| deadline_after(tokio::time::Instant::now(), DRAIN_AFTER_EXIT));
    match tokio::time::timeout_at(until, reading).await {
        Ok(read) => read,
        Err(_) => Ok(None),
    }
}

/// Reads one line of the worker's stderr into `line`: the first
/// [`MAX_LOG_LINE`] bytes of it. The rest of a longer line is read and
/// dropped, never held. `Some(cut)` is a line, `cut` whether it was longer
/// than that; `None` is the end of the stream.
async fn read_log_line(
    reader: &mut BufReader<ChildStderr>,
    line: &mut Vec<u8>,
) -> std::io::Result<Option<bool>> {
    let mut cut = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // A last line without its newline counts.
            return Ok((!line.is_empty() || cut).then_some(cut));
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let content = newline.unwrap_or(available.len());
        let taken = newline.map_or(available.len(), |at| at.saturating_add(1));
        let room = MAX_LOG_LINE.saturating_sub(line.len());
        cut |= content > room;
        line.extend_from_slice(available.get(..content.min(room)).unwrap_or_default());
        reader.consume(taken);
        if newline.is_some() {
            return Ok(Some(cut));
        }
    }
}

/// The event a worker's line says, with the job's secrets masked in every
/// text of the worker's that reaches the daemon's log, the history or the
/// user. The worker masks its own output with what it is given, but not what
/// it can't know: the proxy credentials of its environment, and what it says
/// of a spec before it has read it.
fn translate(event: WorkerEvent, redactor: &Redactor) -> Option<EngineEvent> {
    Some(match event {
        WorkerEvent::Started { index_read_at, .. } => EngineEvent::Started { index_read_at },
        WorkerEvent::ProgressStart { key, kind, prefix } => {
            EngineEvent::Progress(ProgressEvent::Begin {
                key,
                kind: match kind {
                    ProgressKind::Spinner => AppProgressKind::Spinner,
                    ProgressKind::Counter => AppProgressKind::Counter,
                    ProgressKind::Bytes => AppProgressKind::Bytes,
                },
                prefix: redactor.redact(&prefix),
            })
        }
        WorkerEvent::ProgressLength { key, len } => {
            EngineEvent::Progress(ProgressEvent::Length { key, len })
        }
        WorkerEvent::ProgressInc { key, inc } => {
            EngineEvent::Progress(ProgressEvent::Advance { key, by: inc })
        }
        WorkerEvent::ProgressTitle { key, title } => EngineEvent::Progress(ProgressEvent::Title {
            key,
            title: redactor.redact(&title),
        }),
        WorkerEvent::ProgressFinish { key } => EngineEvent::Progress(ProgressEvent::Finish { key }),
        WorkerEvent::Warning {
            item,
            during,
            code,
            detail,
        } => EngineEvent::Warning(Warning {
            kind: match code {
                WarningCode::SourceUnreadable => WarningKind::SourceUnreadable,
                WarningCode::HookWarning => WarningKind::Hook,
                WarningCode::MaintenancePartial => WarningKind::MaintenancePartial,
                // The profile codes belong to the import and export, which
                // run in the CLI; a worker that sent one is reported as the
                // problem it claims, in its own words.
                WarningCode::ProfileKeyIgnored
                | WarningCode::ProfilePrune
                | WarningCode::ProfileEnvIgnored
                | WarningCode::ProfileHooksPerPlan
                | WarningCode::ProfileUsedMissing
                | WarningCode::InlineSecret
                | WarningCode::ConfigInlineSecret
                | WarningCode::ProfileTagDropped
                | WarningCode::ProfileOptionLeftOut
                | WarningCode::ProfileRetentionEmpty
                | WarningCode::Unknown => WarningKind::Other,
            },
            item: item.map(|item| redactor.redact(&item)),
            during: redactor.redact(&during),
            detail: redactor.redact(&detail),
        }),
        // Logs are for the daemon's own log, which gets them under the target
        // `ladderback::worker`.
        WorkerEvent::Log { level, message } => {
            let level = match level {
                Level::Error => log::Level::Error,
                Level::Warn => log::Level::Warn,
                Level::Info => log::Level::Info,
                Level::Debug => log::Level::Debug,
            };
            log::log!(target: "ladderback::worker", level, "{}", redactor.redact(&message));
            return None;
        }
        WorkerEvent::Result(result) => EngineEvent::Succeeded(match result {
            OpResult::Fake { steps_completed } => EngineResult::Fake { steps_completed },
            OpResult::Repository { id } => EngineResult::Repository { id },
            OpResult::Backup(r) => EngineResult::Backup(BackupSummary {
                repository: r.repository,
                snapshot: r.snapshot,
                parent: r.parent,
                files_new: r.files_new,
                files_changed: r.files_changed,
                files_unmodified: r.files_unmodified,
                data_added: r.data_added,
                data_added_packed: r.data_added_packed,
                error_count: r.error_count,
            }),
            OpResult::Rewrite(r) => EngineResult::Rewrite(RewriteSummary {
                repository: r.repository,
                created: r.created,
            }),
            OpResult::Forget(r) => EngineResult::Forget(ForgetSummary {
                repository: r.repository,
                removed: r.removed,
            }),
            OpResult::Prune(r) => EngineResult::Prune(PruneSummary {
                repository: r.repository,
                packs_marked: r.packs_marked,
                packs_deleted: r.packs_deleted,
                packs_recovered: r.packs_recovered,
            }),
            OpResult::Check(r) => EngineResult::Check(CheckSummary {
                repository: r.repository,
                errors: r.errors,
                warnings: r.warnings,
            }),
            // The listing's end, which `Listing` turns into the list before
            // an event reaches here.
            OpResult::Snapshots { .. } => return None,
        }),
        WorkerEvent::Snapshots { .. } => return None,
        WorkerEvent::Failed { code, detail, .. } => {
            let detail = redactor.redact(&detail.unwrap_or_default());
            EngineEvent::Failed(match code {
                ErrorCode::InvalidJobSpec => FailureReason::JobRejected { detail },
                ErrorCode::DeadlinePassed => FailureReason::DeadlinePassed,
                // A repository that already exists where one was to be
                // initialised is the one failure the agent reports as a
                // code of its own: `InitRepository` refuses with it, and
                // nothing was written. The worker's `location` argument
                // says where; the job's spec already does.
                ErrorCode::RepositoryExists => FailureReason::RepositoryExists,
                // A worker only ever says one of the codes above or
                // `operation-failed`. The daemon's own codes are the log's
                // words for what the daemon saw, the request-check codes
                // belong to the HTTP adapter, and the configuration codes to
                // the config store: a worker that sent one has failed in a way
                // this side doesn't know, so it is reported as the failure it
                // claims, in its own words.
                ErrorCode::OperationFailed
                | ErrorCode::EngineUnavailable
                | ErrorCode::RepositoryAliasTaken
                | ErrorCode::RepositoryAliasInvalid
                | ErrorCode::SecretUnresolved
                | ErrorCode::EngineExited
                | ErrorCode::ProtocolViolated
                | ErrorCode::EngineStartTimedOut
                | ErrorCode::EngineExitTimedOut
                | ErrorCode::RepositoryTimedOut
                | ErrorCode::HostNotAllowed
                | ErrorCode::OriginNotAllowed
                | ErrorCode::JsonRequired
                | ErrorCode::ConfigUnavailable
                | ErrorCode::ConfigTooLarge
                | ErrorCode::ConfigInvalid
                | ErrorCode::ConfigSchemaMissing
                | ErrorCode::ConfigSchemaNewer
                | ErrorCode::PlanUnknown
                | ErrorCode::RepositoryUnknown
                | ErrorCode::MaintenanceBusy
                | ErrorCode::RetentionKeepsNothing
                | ErrorCode::MaintenanceKindUnknown
                | ErrorCode::AdoptionNoMatch
                | ErrorCode::AdoptionAmbiguous
                | ErrorCode::ProfileUnreadable
                | ErrorCode::ProfileTooLarge
                | ErrorCode::ProfileInvalid
                | ErrorCode::ProfileUnsupported
                | ErrorCode::PlanIdTaken
                | ErrorCode::InlineSecret
                | ErrorCode::ProfileExportReference
                | ErrorCode::ProfileExportUrlPassword
                | ErrorCode::ProfileExportTagComma
                | ErrorCode::OperationUnknown
                | ErrorCode::OperationNotRunning
                | ErrorCode::InvalidCursor
                | ErrorCode::InvalidBody
                | ErrorCode::BodyTooLarge
                | ErrorCode::Unknown => FailureReason::EngineFailed { detail },
            })
        }
    })
}

fn repository_id(job: &Job) -> Option<&RepositoryId> {
    match job {
        Job::Fake(_) => None,
        Job::Identify(access) | Job::Init(access) | Job::Ls(access) => access.id.as_ref(),
        Job::Backup(job) => job.repository.id.as_ref(),
        Job::Forget(job) => job.repository.id.as_ref(),
        Job::Rewrite(job) => job.repository.id.as_ref(),
        Job::Prune(job) => job.repository.id.as_ref(),
        Job::Check(job) => job.repository.id.as_ref(),
    }
}

/// What masks the job's secrets: the worker's stderr is logged through it,
/// and so are the events. `extra` is what else must not show, the proxy
/// credentials of the worker's environment. A fake job has no secrets of its
/// own.
fn job_redactor(job: &Job, extra: Vec<SecretValue>) -> Redactor {
    let access = match job {
        Job::Fake(_) => return Redactor::new(&extra),
        Job::Identify(access) | Job::Init(access) | Job::Ls(access) => access,
        Job::Backup(job) => &job.repository,
        Job::Forget(job) => &job.repository,
        Job::Rewrite(job) => &job.repository,
        Job::Prune(job) => &job.repository,
        Job::Check(job) => &job.repository,
    };
    let resolved: Vec<SecretValue> = access.secrets.iter().cloned().chain(extra).collect();
    Redactor::for_repository(RepositorySecrets {
        resolved: &resolved,
        ..access.known_secrets()
    })
}

fn repository_spec(access: RepositoryAccess) -> RepositorySpec {
    RepositorySpec {
        repository: access.repository,
        repo_hot: access.repo_hot,
        password: access.password,
        options: access.options,
        options_hot: access.options_hot,
        options_cold: access.options_cold,
        secrets: access.secrets,
    }
}

fn job_kind(job: Job) -> Result<JobKind, EngineError> {
    // A spec names the snapshots it works on, each about 70 bytes of a
    // message that is capped: the caller splits a longer list into jobs.
    if let Job::Forget(ForgetJob { ids, .. }) | Job::Rewrite(RewriteJob { ids, .. }) = &job
        && ids.len() > MAX_JOB_IDS
    {
        return Err(EngineError::Unavailable {
            detail: format!(
                "a job names {} snapshots, over the {MAX_JOB_IDS} it takes",
                ids.len()
            ),
        });
    }
    Ok(match job {
        Job::Fake(FakeJob {
            steps,
            step_delay,
            warnings,
            end,
            deadline_budget,
        }) => JobKind::Fake(FakeJobSpec {
            steps,
            step_delay_ms: u64::try_from(step_delay.as_millis()).map_err(|_| {
                EngineError::Unavailable {
                    detail: "step delay out of range".to_owned(),
                }
            })?,
            warnings: warnings
                .into_iter()
                .map(|w| FakeWarning {
                    code: match w.kind {
                        WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
                        WarningKind::Hook => WarningCode::HookWarning,
                        WarningKind::MaintenancePartial => WarningCode::MaintenancePartial,
                        WarningKind::Other => WarningCode::Unknown,
                    },
                    item: w.item,
                    during: w.during,
                    detail: w.detail,
                })
                .collect(),
            end: match end {
                FakeEnd::Succeed => FakeEndSpec::Succeed,
                FakeEnd::Fail { detail } => FakeEndSpec::Fail { detail },
                FakeEnd::Exit {
                    after_steps,
                    exit_code,
                } => FakeEndSpec::Exit {
                    after_steps,
                    exit_code,
                },
            },
            deadline_budget,
        }),
        Job::Identify(access) => JobKind::Identify(repository_spec(access)),
        Job::Init(access) => JobKind::Init(repository_spec(access)),
        Job::Ls(access) => JobKind::Ls(repository_spec(access)),
        Job::Backup(job) => JobKind::Backup(BackupJobSpec {
            repo: repository_spec(job.repository),
            sources: job.sources,
            plan: job.tags.plan().clone(),
            agent: job.tags.agent(),
            extra_tags: job
                .extra_tags
                .iter()
                .map(|tag| tag.as_str().to_owned())
                .collect(),
            label: job.label,
            deadline_budget: job.deadline_budget,
        }),
        Job::Forget(job) => JobKind::Forget(ForgetJobSpec {
            repo: repository_spec(job.repository),
            ids: job.ids.iter().map(|id| id.as_str().to_owned()).collect(),
        }),
        Job::Rewrite(job) => JobKind::Rewrite(RewriteJobSpec {
            repo: repository_spec(job.repository),
            ids: job.ids.iter().map(|id| id.as_str().to_owned()).collect(),
            tags: job.tags.tags().to_vec(),
        }),
        Job::Prune(job) => JobKind::Prune(PruneJobSpec {
            repo: repository_spec(job.repository),
            keep_delete: job.keep_delete.get(),
        }),
        Job::Check(job) => JobKind::Check(CheckJobSpec {
            repo: repository_spec(job.repository),
            read_data: job.read_data,
        }),
    })
}

/// The cache directories of the running workers. Two workers of one
/// repository must not share a cache: rustic's cache isn't safe for
/// concurrent writers, so each gets the lowest slot nobody else uses.
///
/// The slots this daemon hands out are remembered here. A worker that
/// outlived a daemon that was restarted is not: it holds a lock on its
/// directory (see [`CacheLock`]), and its slot is skipped.
#[derive(Default)]
struct CacheSlots {
    in_use: Mutex<HashMap<String, BTreeSet<u32>>>,
}

impl CacheSlots {
    /// `None` when every slot is taken: that worker runs without a cache.
    fn acquire(self: &Arc<Self>, root: &Path, repository: &RepositoryId) -> Option<CacheSlot> {
        let directory = |number: u32| {
            root.join("ladderback")
                .join(repository.as_str())
                .join(number.to_string())
        };
        let mut in_use = self.in_use.lock().unwrap_or_else(PoisonError::into_inner);
        let taken = in_use.entry(repository.as_str().to_owned()).or_default();
        let number = (0..MAX_CACHE_SLOTS)
            .find(|number| !taken.contains(number) && !held_by_another(&directory(*number)))?;
        taken.insert(number);
        Some(CacheSlot {
            slots: Arc::clone(self),
            repository: repository.as_str().to_owned(),
            number,
            dir: directory(number),
        })
    }
}

/// Whether a worker that isn't one of this daemon's holds `dir`. A directory
/// that can't be looked at counts as free: the worker finds out for itself,
/// and runs without a cache if it must.
fn held_by_another(dir: &Path) -> bool {
    matches!(
        CacheLock::acquire(dir),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    )
}

/// A cache directory in use by one worker; free again once dropped.
struct CacheSlot {
    slots: Arc<CacheSlots>,
    repository: String,
    number: u32,
    dir: PathBuf,
}

impl Drop for CacheSlot {
    fn drop(&mut self) {
        let mut in_use = self
            .slots
            .in_use
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(taken) = in_use.get_mut(&self.repository) {
            taken.remove(&self.number);
            if taken.is_empty() {
                in_use.remove(&self.repository);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use domain::Uuid;

    use super::*;
    use crate::job::MAX_MESSAGE_BYTES;

    fn failed(code: ErrorCode) -> WorkerEvent {
        WorkerEvent::Failed {
            code,
            args: Vec::new(),
            detail: Some("bad spec".to_owned()),
        }
    }

    #[test]
    fn a_worker_that_rejects_its_job_spec_rejects_the_job() {
        assert_eq!(
            translate(failed(ErrorCode::InvalidJobSpec), &Redactor::default()),
            Some(EngineEvent::Failed(FailureReason::JobRejected {
                detail: "bad spec".to_owned()
            }))
        );
    }

    #[test]
    fn other_worker_failures_carry_the_engine_message_verbatim() {
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::EngineUnavailable,
            ErrorCode::EngineExited,
            ErrorCode::ProtocolViolated,
            ErrorCode::EngineStartTimedOut,
            ErrorCode::EngineExitTimedOut,
            ErrorCode::Unknown,
        ] {
            assert_eq!(
                translate(failed(code), &Redactor::default()),
                Some(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "bad spec".to_owned()
                }))
            );
        }
    }

    #[test]
    fn a_deadline_failure_from_the_worker_is_a_passed_deadline() {
        assert_eq!(
            translate(failed(ErrorCode::DeadlinePassed), &Redactor::default()),
            Some(EngineEvent::Failed(FailureReason::DeadlinePassed))
        );
    }

    fn redactor_of(secrets: &[&str]) -> Redactor {
        Redactor::new(
            &secrets
                .iter()
                .map(|secret| SecretValue::new(*secret))
                .collect::<Vec<_>>(),
        )
    }

    fn failed_saying(code: ErrorCode, detail: &str) -> WorkerEvent {
        WorkerEvent::Failed {
            code,
            args: Vec::new(),
            detail: Some(detail.to_owned()),
        }
    }

    /// The worker rejects a spec before it has the job's secrets to mask
    /// with, and a deserialization error can quote the value it choked on.
    #[test]
    fn what_a_rejected_job_spec_quotes_of_the_jobs_secrets_is_masked() {
        let redactor = redactor_of(&["hunter2-demo"]);
        assert_eq!(
            translate(
                failed_saying(
                    ErrorCode::InvalidJobSpec,
                    "invalid type: string \"hunter2-demo\", expected u64 at line 1 column 90"
                ),
                &redactor
            ),
            Some(EngineEvent::Failed(FailureReason::JobRejected {
                detail: "invalid type: string \"***\", expected u64 at line 1 column 90".to_owned()
            }))
        );
    }

    #[test]
    fn what_any_other_failure_says_of_the_jobs_secrets_is_masked_too() {
        let redactor = redactor_of(&["hunter2-demo"]);
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::EngineUnavailable,
            ErrorCode::EngineExited,
            ErrorCode::ProtocolViolated,
            ErrorCode::EngineStartTimedOut,
            ErrorCode::EngineExitTimedOut,
            ErrorCode::Unknown,
        ] {
            assert_eq!(
                translate(
                    failed_saying(code, "could not open with hunter2-demo"),
                    &redactor
                ),
                Some(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "could not open with ***".to_owned()
                })),
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_warning_hides_the_jobs_secrets_in_every_text_it_carries() {
        let redactor = redactor_of(&["hunter2-demo"]);
        let warning = WorkerEvent::Warning {
            item: Some("/home/demo/hunter2-demo.txt".to_owned()),
            during: "reading hunter2-demo".to_owned(),
            code: WarningCode::SourceUnreadable,
            detail: "denied for hunter2-demo".to_owned(),
        };
        assert_eq!(
            translate(warning, &redactor),
            Some(EngineEvent::Warning(Warning {
                kind: WarningKind::SourceUnreadable,
                item: Some("/home/demo/***.txt".to_owned()),
                during: "reading ***".to_owned(),
                detail: "denied for ***".to_owned(),
            }))
        );
    }

    #[test]
    fn a_warning_without_an_item_stays_without_one() {
        let warning = WorkerEvent::Warning {
            item: None,
            during: "scan".to_owned(),
            code: WarningCode::Unknown,
            detail: "odd".to_owned(),
        };
        assert_eq!(
            translate(warning, &redactor_of(&["hunter2-demo"])),
            Some(EngineEvent::Warning(Warning {
                kind: WarningKind::Other,
                item: None,
                during: "scan".to_owned(),
                detail: "odd".to_owned(),
            }))
        );
    }

    #[test]
    fn progress_text_hides_the_jobs_secrets() {
        let redactor = redactor_of(&["hunter2-demo"]);
        assert_eq!(
            translate(
                WorkerEvent::ProgressStart {
                    key: 1,
                    kind: ProgressKind::Spinner,
                    prefix: "opening hunter2-demo".to_owned(),
                },
                &redactor
            ),
            Some(EngineEvent::Progress(ProgressEvent::Begin {
                key: 1,
                kind: AppProgressKind::Spinner,
                prefix: "opening ***".to_owned(),
            }))
        );
        assert_eq!(
            translate(
                WorkerEvent::ProgressTitle {
                    key: 1,
                    title: "still hunter2-demo".to_owned(),
                },
                &redactor
            ),
            Some(EngineEvent::Progress(ProgressEvent::Title {
                key: 1,
                title: "still ***".to_owned(),
            }))
        );
    }

    fn repository(byte: &str) -> RepositoryId {
        RepositoryId::parse(&byte.repeat(32)).unwrap()
    }

    fn cache_dir(root: &Path, id: &RepositoryId, slot: u32) -> PathBuf {
        root.join("ladderback")
            .join(id.as_str())
            .join(slot.to_string())
    }

    #[test]
    fn each_worker_of_a_repository_gets_the_lowest_free_cache_slot() {
        let slots = Arc::new(CacheSlots::default());
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let id = repository("ab");
        let first = slots.acquire(root, &id).unwrap();
        let second = slots.acquire(root, &id).unwrap();
        let third = slots.acquire(root, &id).unwrap();
        assert_eq!(
            [&first.dir, &second.dir, &third.dir],
            [
                &cache_dir(root, &id, 0),
                &cache_dir(root, &id, 1),
                &cache_dir(root, &id, 2)
            ]
        );
        drop(second);
        let reused = slots.acquire(root, &id).unwrap();
        assert_eq!(reused.dir, cache_dir(root, &id, 1));
        let other = slots.acquire(root, &repository("cd")).unwrap();
        assert_eq!(other.dir.file_name().unwrap(), "0");
        drop((first, third, reused, other));
        assert!(slots.in_use.lock().unwrap().is_empty());
    }

    /// A worker outlives a daemon that was restarted: the new daemon knows
    /// nothing of the old worker's slot, but the old worker still holds a
    /// lock on its directory.
    #[test]
    fn a_slot_whose_directory_another_worker_has_locked_is_skipped() {
        let slots = Arc::new(CacheSlots::default());
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let id = repository("ab");
        std::fs::create_dir_all(cache_dir(root, &id, 0)).unwrap();
        let old_worker = std::fs::File::open(cache_dir(root, &id, 0)).unwrap();
        old_worker.try_lock().unwrap();
        let slot = slots.acquire(root, &id).unwrap();
        assert_eq!(slot.dir, cache_dir(root, &id, 1));
        // Once that worker is gone, its slot is free again. A process that a
        // test running in parallel forked meanwhile holds the lock until it
        // has executed its program, so give it a moment.
        drop(old_worker);
        let freed = (0..200).any(|_| {
            let next = slots.acquire(root, &id).unwrap();
            let freed = next.dir == cache_dir(root, &id, 0);
            drop(next);
            if !freed {
                std::thread::sleep(Duration::from_millis(10));
            }
            freed
        });
        assert!(freed, "the slot of a worker that is gone stayed taken");
    }

    fn fake_job() -> Job {
        Job::Fake(FakeJob {
            steps: 0,
            step_delay: Duration::ZERO,
            warnings: Vec::new(),
            end: FakeEnd::Succeed,
            deadline_budget: None,
        })
    }

    async fn run(engine: &WorkerEngine) -> RunningJob {
        engine
            .run(OperationId::from_uuid(Uuid::from_u128(1)), fake_job())
            .await
            .unwrap()
    }

    /// A worker may still be cleaning up after it reports its result; the
    /// daemon dropping the job must not kill it mid-way.
    #[tokio::test]
    async fn a_worker_may_finish_after_reporting_its_result() {
        let scratch = std::env::temp_dir().join(format!("ladderback-reap-{}", std::process::id()));
        let _ = std::fs::remove_file(&scratch);
        let result =
            serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 }))
                .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{result}'; sleep 0.3; touch '{}'",
            scratch.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob {
            mut events,
            control,
        } = run(&engine).await;
        assert!(matches!(
            events.next().await,
            Some(EngineEvent::Succeeded(_))
        ));
        drop((events, control));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(scratch.exists(), "the worker was killed after its result");
        let _ = std::fs::remove_file(&scratch);
    }

    #[tokio::test]
    async fn stopping_a_worker_ends_it_with_sigterm_and_ends_its_events_silently() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; sleep 5"]);
        let RunningJob {
            mut events,
            mut control,
        } = run(&engine).await;
        let began = Instant::now();
        control.stop().await;
        assert!(began.elapsed() < Duration::from_secs(5));
        assert!(events.next().await.is_none(), "events followed a stop");
        control.stop().await; // idempotent
    }

    #[tokio::test]
    async fn a_worker_that_ignores_sigterm_is_killed_after_the_grace_period() {
        let engine = WorkerEngine::new("sh", ["-c", "trap '' TERM; cat >/dev/null; sleep 5"])
            .with_stop_grace(Duration::from_millis(300));
        let RunningJob {
            mut events,
            mut control,
        } = run(&engine).await;
        // Let the shell install its trap before the signal arrives.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let began = Instant::now();
        control.stop().await;
        let took = began.elapsed();
        assert!(
            took >= Duration::from_millis(250),
            "killed too early: {took:?}"
        );
        assert!(
            took < Duration::from_secs(4),
            "waited for the worker: {took:?}"
        );
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn a_worker_that_exits_without_a_result_is_reported_with_its_code() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; exit 3"]);
        let RunningJob { mut events, .. } = run(&engine).await;
        assert_eq!(
            events.next().await,
            Some(EngineEvent::Failed(FailureReason::EngineExited {
                exit_code: Some(3)
            }))
        );
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn a_worker_cut_off_mid_line_at_its_deadline_is_a_passed_deadline() {
        let script = format!(
            "cat >/dev/null; printf '{{\"event\":\"log\"'; exit {}",
            exit_code::DEADLINE_PASSED
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob { mut events, .. } = run(&engine).await;
        assert_eq!(
            events.next().await,
            Some(EngineEvent::Failed(FailureReason::DeadlinePassed))
        );
    }

    #[tokio::test]
    async fn dropping_a_running_job_kills_the_worker_at_once() {
        let scratch = std::env::temp_dir().join(format!("ladderback-drop-{}", std::process::id()));
        let _ = std::fs::remove_file(&scratch);
        let script = format!("cat >/dev/null; sleep 1; touch '{}'", scratch.display());
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        drop(run(&engine).await);
        tokio::time::sleep(Duration::from_millis(1600)).await;
        assert!(!scratch.exists(), "the worker outlived its dropped job");
    }

    /// The process id a script wrote to `file`.
    #[cfg(unix)]
    async fn pid_from(file: &Path) -> i32 {
        for _ in 0..200 {
            let written = std::fs::read_to_string(file)
                .ok()
                .and_then(|text| text.trim().parse().ok());
            if let Some(pid) = written {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the script never wrote a process id to {}", file.display());
    }

    /// Whether `pid` is gone within five seconds. A zombie is gone: it ran
    /// to its end, and only waits for a parent that may never reap it (an
    /// orphan, in a container whose PID 1 doesn't reap).
    #[cfg(unix)]
    async fn gone_soon(pid: i32) -> bool {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        for _ in 0..200 {
            if kill(Pid::from_raw(pid), None).is_err() || is_zombie(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    }

    /// The state in `/proc/<pid>/stat` follows the `)` that ends the name.
    #[cfg(target_os = "linux")]
    fn is_zombie(pid: i32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
        })
    }

    /// No `/proc` to tell: `kill` alone decides.
    #[cfg(all(unix, not(target_os = "linux")))]
    fn is_zombie(_pid: i32) -> bool {
        false
    }

    #[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
    #[test]
    fn a_worker_that_exited_is_still_unreaped_even_when_its_status_cannot_be_read() {
        use nix::errno::Errno;
        use nix::sys::wait::WaitStatus;
        use nix::unistd::Pid;

        let exited = Ok(WaitStatus::Exited(Pid::from_raw(42), 3));
        assert_eq!(worker_exit(exited), Some(WorkerExit::Unreaped));
        // A real-time signal: libc's waitid succeeded, nix can't name it.
        assert_eq!(worker_exit(Err(Errno::EINVAL)), Some(WorkerExit::Unreaped));
    }

    #[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
    #[test]
    fn a_worker_reaped_elsewhere_is_only_reaped_and_an_interrupted_wait_is_retried() {
        use nix::errno::Errno;

        assert_eq!(worker_exit(Err(Errno::ECHILD)), Some(WorkerExit::Reaped));
        assert_eq!(worker_exit(Err(Errno::EINTR)), None);
    }

    /// A sandbox that refuses `waitid` must not leave the worker unwaited:
    /// it is waited for by `Child::wait` instead.
    #[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
    #[test]
    fn a_wait_the_system_refuses_falls_back_to_reaping_the_worker() {
        use nix::errno::Errno;

        for refused in [Errno::ENOSYS, Errno::EPERM, Errno::EFAULT] {
            assert_eq!(
                worker_exit(Err(refused)),
                Some(WorkerExit::Unobservable),
                "{refused}"
            );
        }
    }

    /// A worker script that leaves a `sleep` of its own running, as rustic's
    /// rclone backend does, and the file that holds that process's id.
    #[cfg(unix)]
    fn worker_with_a_child(trap: &str, dir: &Path) -> (WorkerEngine, PathBuf) {
        let pid_file = dir.join("child.pid");
        let script = format!(
            "{trap} cat >/dev/null; sleep 30 & echo $! >'{}'; wait",
            pid_file.display()
        );
        (WorkerEngine::new("sh", ["-c", script.as_str()]), pid_file)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_a_worker_stops_the_processes_it_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, pid_file) = worker_with_a_child("", dir.path());
        let RunningJob {
            events: _events,
            mut control,
        } = run(&engine).await;
        let child = pid_from(&pid_file).await;
        control.stop().await;
        assert!(
            gone_soon(child).await,
            "the worker's child outlived the stop"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_ignores_sigterm_is_killed_with_the_worker_after_the_grace_period() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, pid_file) = worker_with_a_child("trap '' TERM;", dir.path());
        let engine = engine.with_stop_grace(Duration::from_millis(300));
        let RunningJob {
            events: _events,
            mut control,
        } = run(&engine).await;
        let child = pid_from(&pid_file).await;
        control.stop().await;
        assert!(
            gone_soon(child).await,
            "the worker's child survived the kill"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_a_running_job_kills_the_processes_the_worker_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, pid_file) = worker_with_a_child("", dir.path());
        let job = run(&engine).await;
        let child = pid_from(&pid_file).await;
        drop(job);
        assert!(
            gone_soon(child).await,
            "the worker's child outlived the dropped job"
        );
    }

    /// A process the worker started, rclone say, can outlive it and keep the
    /// pipe open. The events end with the worker all the same, after what
    /// the worker had written, and the process is ended with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_events_end_with_the_worker_even_if_another_process_holds_the_pipe() {
        let warning = serde_json::to_string(&WorkerEvent::Warning {
            item: None,
            during: "backup".to_owned(),
            code: WarningCode::Unknown,
            detail: "slow disk".to_owned(),
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        // The `sleep` inherits the worker's stdout and outlives the shell.
        let script = format!(
            "cat >/dev/null; echo '{warning}'; sleep 8 & echo $! >'{}'; exit 3",
            pid_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let events = run(&engine).await.events;
        let events = tokio::time::timeout(Duration::from_secs(4), events.collect::<Vec<_>>())
            .await
            .expect("the events ended while another process held the pipe");
        assert!(
            matches!(
                events.as_slice(),
                [
                    EngineEvent::Warning(_),
                    EngineEvent::Failed(FailureReason::EngineExited { exit_code: Some(3) })
                ]
            ),
            "{events:?}"
        );
        let child = pid_from(&pid_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker left running outlived it"
        );
    }

    /// rustic's rclone is not stopped by the worker's `process::exit`, so
    /// however the worker ends, at its deadline, with a stream it cannot
    /// write to, on a SIGTERM, or killed, what it started ends with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_ends_by_itself_takes_the_processes_it_started_with_it() {
        for (how, ending) in [
            ("at its deadline", "exit 75"),
            ("with a stream it cannot write to", "exit 74"),
            ("on a SIGTERM", "exit 143"),
            ("by being killed", "kill -KILL $$"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let pid_file = dir.path().join("child.pid");
            let script = format!(
                "cat >/dev/null; sleep 30 & echo $! >'{}'; {ending}",
                pid_file.display()
            );
            let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
            let events = run(&engine).await.events;
            let child = pid_from(&pid_file).await;
            let _ = events.collect::<Vec<_>>().await;
            assert!(
                gone_soon(child).await,
                "the process a worker started outlived it when it ended {how}"
            );
        }
    }

    /// What the capturing logger was given for `ladderback::worker`.
    static WORKER_LOG: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target() == "ladderback::worker"
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                WORKER_LOG
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((record.level(), record.args().to_string()));
            }
        }

        fn flush(&self) {}
    }

    fn capture_the_worker_log() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = log::set_logger(&CapturingLogger);
            log::set_max_level(log::LevelFilter::Debug);
        });
    }

    /// The first record of `level` that `wanted` accepts, looked for five
    /// seconds.
    async fn logged_soon(level: log::Level, wanted: impl Fn(&str) -> bool) -> Option<String> {
        for _ in 0..200 {
            let found = WORKER_LOG
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .find(|(logged, text)| *logged == level && wanted(text))
                .map(|(_, text)| text.clone());
            if found.is_some() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        None
    }

    fn result_line() -> String {
        serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 })).unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn what_a_worker_writes_to_stderr_reaches_the_log_as_a_warning() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; echo 'stderr marker 1' >&2; echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let events = run(&engine).await.events.collect::<Vec<_>>().await;
        assert!(
            matches!(events.as_slice(), [EngineEvent::Succeeded(_)]),
            "{events:?}"
        );
        assert!(
            logged_soon(log::Level::Warn, |text| text == "stderr marker 1")
                .await
                .is_some(),
            "the worker's stderr never reached the log"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stderr_line_over_the_cap_is_cut_and_the_lines_after_it_still_arrive() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; head -c {} /dev/zero | tr '\\0' x >&2; echo >&2; \
             echo 'stderr marker 2' >&2; echo '{}'",
            3 * MAX_MESSAGE_BYTES,
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let _ = run(&engine).await.events.collect::<Vec<_>>().await;
        let long = logged_soon(log::Level::Warn, |text| text.starts_with("xxxx"))
            .await
            .expect("the long stderr line never reached the log");
        assert!(
            long.len() <= MAX_MESSAGE_BYTES + '…'.len_utf8() && long.ends_with('…'),
            "the long line was not cut at the cap: {} bytes",
            long.len()
        );
        assert!(
            logged_soon(log::Level::Warn, |text| text == "stderr marker 2")
                .await
                .is_some(),
            "the line after the long one was lost"
        );
    }

    fn identify(password: &str, secrets: &[&str]) -> Job {
        Job::Identify(RepositoryAccess {
            repository: "/backups/demo".to_owned(),
            repo_hot: None,
            password: domain::secrets::SecretValue::new(password),
            options: std::collections::BTreeMap::new(),
            options_hot: std::collections::BTreeMap::new(),
            options_cold: std::collections::BTreeMap::new(),
            secrets: secrets
                .iter()
                .map(|secret| domain::secrets::SecretValue::new(*secret))
                .collect(),
            id: None,
        })
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_jobs_password_and_secrets_are_masked_in_what_the_worker_writes_to_stderr() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; echo 'redaction one: hunter2-demo then CANARY-resolved' >&2; echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = identify("hunter2-demo", &["CANARY-resolved"]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(events.as_slice(), [EngineEvent::Succeeded(_)]));
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("redaction one:"))
            .await
            .expect("the worker's stderr never reached the log");
        assert_eq!(line, "redaction one: *** then ***");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_secret_straddling_the_cut_of_a_long_stderr_line_leaves_nothing_of_itself() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; head -c {} /dev/zero | tr '\\0' x >&2; \
             printf 'CANARY-straddle and more\\n' >&2; echo '{}'",
            MAX_MESSAGE_BYTES - 3,
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = identify("hunter2-demo", &["CANARY-straddle"]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let _ = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("xxxx"))
            .await
            .expect("the long stderr line never reached the log");
        let kept = line.strip_suffix('…').expect("the line was not cut");
        assert!(
            kept.chars().all(|c| c == 'x'),
            "the start of the secret is in the log: {:?}",
            kept.chars().rev().take(8).collect::<String>()
        );
    }

    fn line_of(event: &WorkerEvent) -> String {
        serde_json::to_string(event).unwrap()
    }

    /// The events of `job` when a stand-in worker, which started with
    /// `environment` (the real one when `None`), writes `stderr` and then
    /// `lines`.
    #[cfg(unix)]
    async fn answered(
        job: Job,
        environment: Option<Vec<(OsString, OsString)>>,
        stderr: &str,
        lines: &[String],
    ) -> Vec<EngineEvent> {
        let lines: String = lines.iter().map(|line| format!(" '{line}'")).collect();
        let script = format!("cat >/dev/null; echo '{stderr}' >&2; printf '%s\\n'{lines}");
        let mut engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        if let Some(environment) = environment {
            engine = engine.with_environment(environment);
        }
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine.run(op, job).await.unwrap().events;
        tokio::time::timeout(Duration::from_secs(10), events.collect::<Vec<_>>())
            .await
            .expect("the worker's events ended")
    }

    /// The environment of a daemon behind a proxy that wants credentials.
    #[cfg(unix)]
    fn behind_a_proxy() -> Option<Vec<(OsString, OsString)>> {
        Some(vec![
            (
                OsString::from("PATH"),
                std::env::var_os("PATH").unwrap_or_default(),
            ),
            (
                OsString::from("HTTPS_PROXY"),
                OsString::from("http://demo:CANARY-proxy@proxy.demo:3128"),
            ),
        ])
    }

    /// The worker can refuse a spec before it has the job's secrets to mask
    /// with, and what it says then can quote one.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_rejected_job_spec_that_quotes_the_jobs_password_is_masked_in_the_failure() {
        let rejection = failed_saying(
            ErrorCode::InvalidJobSpec,
            "invalid type: string \"hunter2-demo\", expected a map",
        );
        let events = answered(
            identify("hunter2-demo", &[]),
            None,
            "rejecting",
            &[line_of(&rejection)],
        )
        .await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::JobRejected {
                detail: "invalid type: string \"***\", expected a map".to_owned()
            })]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_line_that_cannot_be_read_and_quotes_a_secret_is_masked_in_the_failure() {
        let events = answered(
            identify("hunter2-demo", &[]),
            None,
            "garbling",
            &[r#"{"event":"hunter2-demo"}"#.to_owned()],
        )
        .await;
        let [EngineEvent::Failed(FailureReason::EngineFailed { detail })] = events.as_slice()
        else {
            panic!("expected one engine failure, got {events:?}");
        };
        assert!(detail.starts_with("malformed worker event: "), "{detail}");
        assert!(detail.contains("***"), "{detail}");
        assert!(!detail.contains("hunter2-demo"), "{detail}");
    }

    /// The proxy variables pass the worker's scrubbed environment, so a
    /// program the worker starts can use them, and what a proxy answers can
    /// echo them: on stderr and in an event.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_credentials_of_a_proxy_url_are_masked_in_stderr_and_in_events() {
        capture_the_worker_log();
        let warning = WorkerEvent::Warning {
            item: None,
            during: "connecting".to_owned(),
            code: WarningCode::Unknown,
            detail: "proxy asked for CANARY-proxy".to_owned(),
        };
        let failure = failed_saying(
            ErrorCode::OperationFailed,
            "refused by http://demo:CANARY-proxy@proxy.demo:3128",
        );
        let events = answered(
            fake_job(),
            behind_a_proxy(),
            "proxy echo: http://demo:CANARY-proxy@proxy.demo:3128",
            &[line_of(&warning), line_of(&failure)],
        )
        .await;
        assert_eq!(
            events,
            [
                EngineEvent::Warning(Warning {
                    kind: WarningKind::Other,
                    item: None,
                    during: "connecting".to_owned(),
                    detail: "proxy asked for ***".to_owned(),
                }),
                EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "refused by http://***@proxy.demo:3128".to_owned()
                }),
            ]
        );
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("proxy echo:"))
            .await
            .expect("the worker's stderr never reached the log");
        assert_eq!(line, "proxy echo: http://***@proxy.demo:3128");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_jobs_own_secrets_and_the_proxy_credentials_are_both_masked() {
        capture_the_worker_log();
        let failure = failed_saying(
            ErrorCode::OperationFailed,
            "hunter2-demo then CANARY-proxy then CANARY-resolved",
        );
        let events = answered(
            identify("hunter2-demo", &["CANARY-resolved"]),
            behind_a_proxy(),
            "both: hunter2-demo CANARY-proxy CANARY-resolved",
            &[line_of(&failure)],
        )
        .await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineFailed {
                detail: "*** then *** then ***".to_owned()
            })]
        );
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("both:"))
            .await
            .expect("the worker's stderr never reached the log");
        assert_eq!(line, "both: *** *** ***");
    }

    /// What a worker logs goes to the daemon's log as it says it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_log_line_from_the_worker_hides_the_jobs_secrets() {
        capture_the_worker_log();
        let log_line = WorkerEvent::Log {
            level: Level::Info,
            message: "log one: hunter2-demo".to_owned(),
        };
        let events = answered(
            identify("hunter2-demo", &[]),
            None,
            "logging",
            &[line_of(&log_line), result_line()],
        )
        .await;
        assert!(matches!(events.as_slice(), [EngineEvent::Succeeded(_)]));
        let line = logged_soon(log::Level::Info, |text| text.starts_with("log one:"))
            .await
            .expect("the worker's log line never reached the log");
        assert_eq!(line, "log one: ***");
    }

    /// What a stand-in worker saw, written to files: its environment, its
    /// arguments and its stdin.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_starts_with_a_scrubbed_environment_and_still_gets_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let script = format!(
            "cat > '{dir}/spec.json'; env > '{dir}/env.txt'; echo \"$0 $*\" > '{dir}/args.txt'; \
             echo '{result}'",
            dir = dir.path().display(),
            result = result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_environment([
            (OsString::from("PATH"), path),
            (OsString::from("HOME"), OsString::from("/home/demo")),
            (
                OsString::from("AWS_SECRET_ACCESS_KEY"),
                OsString::from("CANARY-aws"),
            ),
            (
                OsString::from("RUSTIC_PASSWORD"),
                OsString::from("CANARY-rustic"),
            ),
            (OsString::from("DEMO_B2_KEY"), OsString::from("CANARY-b2")),
        ]);
        let job = identify("demo-password", &[]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(events.as_slice(), [EngineEvent::Succeeded(_)]));
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        let env = read("env.txt");
        assert!(env.contains("HOME=/home/demo"), "HOME is missing: {env}");
        assert!(env.lines().any(|line| line.starts_with("PATH=")), "{env}");
        for leaked in [
            "AWS_SECRET_ACCESS_KEY",
            "RUSTIC_PASSWORD",
            "DEMO_B2_KEY",
            "CANARY",
        ] {
            assert!(!env.contains(leaked), "{leaked} reached the worker: {env}");
        }
        let spec: serde_json::Value = serde_json::from_str(&read("spec.json")).unwrap();
        assert_eq!(spec["job"]["password"], "demo-password");
        assert!(!read("args.txt").contains("demo-password"));
    }

    /// A hot/cold repository: each half has a secret of its own.
    fn hot_cold_access() -> RepositoryAccess {
        let options = |pairs: [(&str, &str); 2]| -> BTreeMap<String, String> {
            pairs
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect()
        };
        RepositoryAccess {
            repository: "opendal:azblob".to_owned(),
            repo_hot: Some("rest:https://demo:HOT-URL-PASSWORD@hot.example:8000/hot".to_owned()),
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: options([
                ("secret_access_key", "HOT-ACCESS-KEY"),
                ("region", "hot-region"),
            ]),
            options_cold: options([
                ("sas_token", "COLD-SAS-TOKEN"),
                ("container", "cold-container"),
            ]),
            secrets: Vec::new(),
            id: None,
        }
    }

    /// A worker can only open both halves of a repository if its job says
    /// what they are.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_is_given_the_hot_repository_and_the_hot_and_cold_options_of_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "cat > '{dir}/spec.json'; echo '{result}'",
            dir = dir.path().display(),
            result = result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let access = hot_cold_access();
        let events = engine
            .run(op, Job::Identify(access.clone()))
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(events.as_slice(), [EngineEvent::Succeeded(_)]));
        let spec: JobSpec =
            serde_json::from_slice(&std::fs::read(dir.path().join("spec.json")).unwrap()).unwrap();
        let JobKind::Identify(repository) = spec.job else {
            panic!("the worker was not given an identify job: {:?}", spec.job);
        };
        assert_eq!(repository.repository, access.repository);
        assert_eq!(repository.repo_hot, access.repo_hot);
        assert_eq!(repository.options_hot, access.options_hot);
        assert_eq!(repository.options_cold, access.options_cold);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_secrets_of_both_halves_are_masked_in_what_the_worker_writes_to_stderr() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; \
             echo 'redaction two: HOT-URL-PASSWORD HOT-ACCESS-KEY COLD-SAS-TOKEN cold-container' >&2; \
             echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, Job::Identify(hot_cold_access()))
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(events.as_slice(), [EngineEvent::Succeeded(_)]));
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("redaction two:"))
            .await
            .expect("the worker's stderr never reached the log");
        // Only what is a secret goes; the name of the cold container shows.
        assert_eq!(line, "redaction two: *** *** *** cold-container");
    }

    /// More than a pipe holds, written between two events: a stderr that
    /// nobody reads would stop the worker there.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_floods_stderr_cannot_stall_its_events_or_the_end_of_its_job() {
        capture_the_worker_log();
        let warning = serde_json::to_string(&WorkerEvent::Warning {
            item: None,
            during: "backup".to_owned(),
            code: WarningCode::Unknown,
            detail: "slow disk".to_owned(),
        })
        .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{warning}'; i=0; \
             while [ $i -lt 4000 ]; do echo 'flood flood flood flood flood' >&2; i=$((i+1)); done; \
             echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob {
            events,
            mut control,
        } = run(&engine).await;
        let events = tokio::time::timeout(Duration::from_secs(4), events.collect::<Vec<_>>())
            .await
            .expect("the flood on stderr stalled the events");
        assert!(
            matches!(
                events.as_slice(),
                [EngineEvent::Warning(_), EngineEvent::Succeeded(_)]
            ),
            "{events:?}"
        );
        tokio::time::timeout(Duration::from_secs(5), control.stop())
            .await
            .expect("the flood on stderr held the end of the job back");
    }

    #[tokio::test]
    async fn a_cleanup_deadline_that_cannot_be_represented_ends_the_cleanup_at_once() {
        let now = tokio::time::Instant::now();
        assert_eq!(deadline_after(now, Duration::MAX), now);
        assert_eq!(
            deadline_after(now, Duration::from_secs(10)),
            now + Duration::from_secs(10)
        );
    }

    /// Runs `script` as the worker and collects its events; the test fails if
    /// they don't end within ten seconds.
    async fn events_from(script: &str) -> Vec<EngineEvent> {
        let engine = WorkerEngine::new("sh", ["-c", script]);
        let events = run(&engine).await.events;
        tokio::time::timeout(Duration::from_secs(10), events.collect::<Vec<_>>())
            .await
            .expect("the worker's events ended")
    }

    fn assert_over_the_cap(events: &[EngineEvent]) {
        let [EngineEvent::Failed(FailureReason::EngineFailed { detail })] = events else {
            panic!("expected exactly one engine failure, got {events:?}");
        };
        assert!(
            detail.contains(&MAX_MESSAGE_BYTES.to_string()),
            "the failure should name the cap: {detail}"
        );
    }

    #[tokio::test]
    async fn an_event_line_past_the_cap_fails_the_job() {
        // 3 MiB of `x`, then the newline.
        let events =
            events_from("cat >/dev/null; head -c 3145728 /dev/zero | tr '\\0' x; echo").await;
        assert_over_the_cap(&events);
    }

    #[tokio::test]
    async fn a_line_that_never_ends_fails_the_job_instead_of_growing_forever() {
        // 64 MiB without a newline, then silence: the worker is still alive.
        let events =
            events_from("cat >/dev/null; head -c 67108864 /dev/zero | tr '\\0' x; exec sleep 30")
                .await;
        assert_over_the_cap(&events);
    }

    #[tokio::test]
    async fn a_job_spec_past_the_cap_is_refused_before_a_worker_starts() {
        let job = Job::Fake(FakeJob {
            warnings: vec![domain::execution::Warning {
                kind: domain::execution::WarningKind::Other,
                item: None,
                during: "backup".to_owned(),
                detail: "x".repeat(2 * MAX_MESSAGE_BYTES),
            }],
            steps: 0,
            step_delay: Duration::ZERO,
            end: FakeEnd::Succeed,
            deadline_budget: None,
        });
        // A program that can't start, so only the check before spawning can
        // produce the cap's message.
        let engine = WorkerEngine::new("/nonexistent/ladderback-worker", ["worker"]);
        let error = engine
            .run(OperationId::from_uuid(Uuid::from_u128(1)), job)
            .await
            .err()
            .expect("a spec past the cap is refused");
        assert!(
            matches!(&error, EngineError::Unavailable { detail } if detail.contains(&MAX_MESSAGE_BYTES.to_string())),
            "the error should name the cap: {error:?}"
        );
    }

    fn listed(n: u128) -> crate::job::SnapshotResult {
        crate::job::SnapshotResult {
            id: domain::SnapshotId::parse(&format!("{n:064x}")).unwrap(),
            time: domain::Timestamp::from_second(i64::try_from(n).unwrap()).unwrap(),
            hostname: "demo-host".to_owned(),
            label: format!("label {n}"),
            tags: vec!["tag".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: n.is_multiple_of(2),
            tags_truncated: n.is_multiple_of(3),
        }
    }

    fn batch(from: u128, to: u128) -> WorkerEvent {
        WorkerEvent::Snapshots {
            snapshots: (from..to).map(listed).collect(),
        }
    }

    /// The script of a worker that prints these events, one line each.
    fn printing(events: &[WorkerEvent]) -> String {
        let lines: String = events
            .iter()
            .map(|event| format!("echo '{}'; ", serde_json::to_string(event).unwrap()))
            .collect();
        format!("cat >/dev/null; {lines}")
    }

    #[tokio::test]
    async fn a_worker_that_lists_in_batches_gives_the_job_one_whole_list() {
        let events = events_from(&printing(&[
            batch(0, 2),
            batch(2, 3),
            WorkerEvent::Result(OpResult::Snapshots { count: 3 }),
        ]))
        .await;
        let [EngineEvent::Succeeded(EngineResult::Snapshots(all))] = events.as_slice() else {
            panic!("one list, whole: {events:?}");
        };
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn a_worker_whose_listing_comes_up_short_fails_the_job() {
        let events = events_from(&printing(&[
            batch(0, 2),
            WorkerEvent::Result(OpResult::Snapshots { count: 3 }),
        ]))
        .await;
        let [EngineEvent::Failed(FailureReason::EngineFailed { detail })] = events.as_slice()
        else {
            panic!("the job fails: {events:?}");
        };
        assert!(detail.contains('3') && detail.contains('2'), "{detail}");
    }

    #[test]
    fn a_listing_arrives_in_batches_and_is_handed_on_whole_at_its_end() {
        let mut listing = Listing::default();
        assert!(matches!(listing.absorb(batch(0, 2)), Absorbed::Kept));
        assert!(matches!(listing.absorb(batch(2, 5)), Absorbed::Kept));
        let Absorbed::Complete(all) =
            listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count: 5 }))
        else {
            panic!("the listing is complete");
        };
        let ids: Vec<String> = all.iter().map(|s| s.id.as_str().to_owned()).collect();
        let expected: Vec<String> = (0..5).map(|n| listed(n).id.as_str().to_owned()).collect();
        assert_eq!(ids, expected, "in the order sent");
        // What was cut from a snapshot goes with it.
        assert!(all[0].paths_truncated && all[0].tags_truncated);
        assert!(!all[1].paths_truncated && !all[1].tags_truncated);
        assert_eq!(all[3].label, "label 3");
        assert_eq!(all[3].paths, ["/home/demo"]);
        assert_eq!(all[3].tags, ["tag"]);
        assert!(all[3].tags_truncated && !all[3].paths_truncated);
    }

    #[test]
    fn an_empty_listing_is_complete_with_nothing_in_it() {
        let mut listing = Listing::default();
        let Absorbed::Complete(all) =
            listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count: 0 }))
        else {
            panic!("the listing is complete");
        };
        assert!(all.is_empty());
    }

    #[test]
    fn a_listing_that_lost_a_batch_is_refused_not_taken_for_the_whole() {
        for count in [4, 6] {
            let mut listing = Listing::default();
            let _ = listing.absorb(batch(0, 5));
            let Absorbed::Refused(detail) =
                listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count }))
            else {
                panic!("a listing that disagrees with its count is refused");
            };
            assert!(
                detail.contains(&count.to_string()) && detail.contains('5'),
                "{detail}"
            );
        }
    }

    #[test]
    fn a_listing_past_the_most_the_daemon_holds_is_refused() {
        let mut listing = Listing::default();
        let most = MAX_LISTED_SNAPSHOTS;
        let fill = |from: usize, to: usize| WorkerEvent::Snapshots {
            snapshots: (from..to).map(|_| listed(1)).collect(),
        };
        assert!(matches!(listing.absorb(fill(0, most - 1)), Absorbed::Kept));
        assert!(matches!(listing.absorb(fill(0, 1)), Absorbed::Kept));
        let Absorbed::Refused(detail) = listing.absorb(fill(0, 1)) else {
            panic!("one past the most is refused");
        };
        assert!(detail.contains(&most.to_string()), "{detail}");
    }

    #[test]
    fn a_listing_past_the_bytes_the_daemon_holds_is_refused_whatever_its_count() {
        let mut listing = Listing::default();
        let fat = || crate::job::SnapshotResult {
            label: "x".repeat(130 * 1024),
            tags: Vec::new(),
            paths: Vec::new(),
            ..listed(1)
        };
        let per = 130 * 1024 + "demo-host".len() + 256;
        let fits = MAX_LISTED_BYTES / per;
        let batch = |n: usize| WorkerEvent::Snapshots {
            snapshots: (0..n).map(|_| fat()).collect(),
        };
        assert!(matches!(listing.absorb(batch(fits)), Absorbed::Kept));
        let Absorbed::Refused(detail) = listing.absorb(batch(2)) else {
            panic!("past the bytes held is refused");
        };
        assert!(detail.contains(&MAX_LISTED_BYTES.to_string()), "{detail}");
    }

    #[test]
    fn what_is_not_a_listing_passes_through_untouched() {
        let mut listing = Listing::default();
        let event = WorkerEvent::Result(OpResult::Fake { steps_completed: 3 });
        assert!(matches!(
            listing.absorb(event.clone()),
            Absorbed::Passed(passed) if passed == event
        ));
    }

    fn access() -> app::ports::RepositoryAccess {
        app::ports::RepositoryAccess {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: domain::secrets::SecretValue::new("demo-password"),
            options: std::collections::BTreeMap::new(),
            options_hot: std::collections::BTreeMap::new(),
            options_cold: std::collections::BTreeMap::new(),
            secrets: Vec::new(),
            id: None,
        }
    }

    fn snapshot_ids(count: usize) -> Vec<domain::SnapshotId> {
        (0..count)
            .map(|n| domain::SnapshotId::parse(&format!("{n:064x}")).unwrap())
            .collect()
    }

    #[test]
    fn a_forget_of_the_most_one_job_may_name_fits_one_message_with_room_to_spare() {
        let job = Job::Forget(app::ports::ForgetJob {
            repository: access(),
            ids: snapshot_ids(app::ports::MAX_JOB_IDS),
        });
        let spec = serde_json::to_vec(&job_kind(job).unwrap()).unwrap();
        assert!(
            spec.len() < MAX_MESSAGE_BYTES / 2,
            "{} bytes for {} IDs",
            spec.len(),
            app::ports::MAX_JOB_IDS
        );
    }

    #[tokio::test]
    async fn a_forget_or_rewrite_naming_more_than_a_job_takes_is_refused_before_a_worker_starts() {
        let too_many = snapshot_ids(app::ports::MAX_JOB_IDS + 1);
        let forget = Job::Forget(app::ports::ForgetJob {
            repository: access(),
            ids: too_many.clone(),
        });
        let rewrite = Job::Rewrite(app::ports::RewriteJob {
            repository: access(),
            ids: too_many,
            tags: domain::execution::plan_tags::PlanTags::new(
                domain::PlanId::parse("home").unwrap(),
                domain::AgentId::from_uuid(Uuid::from_u128(1)),
            ),
        });
        // A program that can't start, so only the check before spawning can
        // produce the message.
        let engine = WorkerEngine::new("/nonexistent/ladderback-worker", ["worker"]);
        for job in [forget, rewrite] {
            let error = engine
                .run(OperationId::from_uuid(Uuid::from_u128(1)), job)
                .await
                .err()
                .expect("a job naming too many snapshots is refused");
            assert!(
                matches!(&error, EngineError::Unavailable { detail } if detail.contains(&app::ports::MAX_JOB_IDS.to_string())),
                "the error should name the limit: {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_event_line_of_exactly_the_cap_is_read() {
        let started = serde_json::to_string(&WorkerEvent::Started {
            op_id: OperationId::from_uuid(Uuid::from_u128(1)),
            pid: 1,
            index_read_at: domain::Timestamp::from_second(1_800_000_000).unwrap(),
        })
        .unwrap();
        let head =
            r#"{"event":"warning","item":null,"during":"backup","code":"unknown","detail":""#;
        let tail = r#""}"#;
        let detail_len = MAX_MESSAGE_BYTES - head.len() - tail.len();
        let result =
            serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 }))
                .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{started}'; printf '%s' '{head}'; \
             head -c {detail_len} /dev/zero | tr '\\0' x; echo '{tail}'; echo '{result}'"
        );
        let events = events_from(&script).await;
        assert!(
            matches!(
                events.as_slice(),
                [EngineEvent::Started { .. }, EngineEvent::Warning(warning), EngineEvent::Succeeded(_)]
                    if warning.detail.len() == detail_len
            ),
            "unexpected events: {} events",
            events.len()
        );
    }

    // The start and exit timeouts: a worker that hangs must not hold the
    // daemon. Every wait on a worker is guarded, so that a regression fails
    // at once instead of hanging the suite.

    /// A job whose spec is about 900 KB: under the protocol's cap, and more
    /// than any pipe holds, so a worker that never reads its stdin leaves
    /// the daemon's write of the job blocked.
    fn job_larger_than_a_pipe() -> Job {
        Job::Fake(FakeJob {
            steps: 0,
            step_delay: Duration::ZERO,
            warnings: vec![Warning {
                kind: WarningKind::Other,
                item: None,
                during: "backup".to_owned(),
                detail: "x".repeat(900_000),
            }],
            end: FakeEnd::Succeed,
            deadline_budget: None,
        })
    }

    /// The engine's answer to `job`, which it has `guard` to give.
    async fn run_within(
        guard: Duration,
        engine: &WorkerEngine,
        job: Job,
    ) -> Result<RunningJob, EngineError> {
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        tokio::time::timeout(guard, engine.run(op, job))
            .await
            .expect("`run` never returned: a worker that never read its job held it")
    }

    /// Everything `events` yields, which has `guard` to end.
    async fn collected_within(
        guard: Duration,
        events: impl futures::Stream<Item = EngineEvent>,
    ) -> Vec<EngineEvent> {
        tokio::time::timeout(guard, events.collect::<Vec<_>>())
            .await
            .expect("the events never ended")
    }

    /// The daemon writes a worker its job before anything else, and a worker
    /// that never reads it leaves that write blocked once the pipe is full.
    /// The start timeout ends the wait, and the worker with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_never_reads_its_job_is_killed_when_the_start_timeout_passes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!("echo $$ >'{}'; exec sleep 30", pid_file.display());
        // Long enough for a loaded machine to start the shell, which the
        // kill must not beat to its first command.
        let timeout = Duration::from_secs(1);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_start_timeout(timeout);
        let began = Instant::now();
        let answer = run_within(Duration::from_secs(5), &engine, job_larger_than_a_pipe()).await;
        let took = began.elapsed();
        assert_eq!(answer.err(), Some(EngineError::StartTimedOut));
        assert!(
            took >= timeout.saturating_sub(Duration::from_millis(50)),
            "gave up before the start timeout: {took:?}"
        );
        let worker = pid_from(&pid_file).await;
        assert!(
            gone_soon(worker).await,
            "the worker that never read its job outlived the start timeout"
        );
    }

    /// The start timeout bounds the write, not the worker's pace: one that
    /// takes a while to start reading, but reads in time, runs normally.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_reads_its_job_late_but_within_the_start_timeout_still_runs() {
        let script = format!("sleep 0.3; cat >/dev/null; echo '{}'", result_line());
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_start_timeout(Duration::from_secs(5));
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(15), &engine, job_larger_than_a_pipe())
            .await
            .expect("a worker that read its job in time was timed out");
        let events = collected_within(Duration::from_secs(10), events).await;
        assert_eq!(
            events,
            [EngineEvent::Succeeded(EngineResult::Fake {
                steps_completed: 0
            })]
        );
    }

    /// A write that fails, here to a worker that is gone, is not a timeout.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_is_gone_before_it_reads_its_job_is_unavailable_not_timed_out() {
        let engine =
            WorkerEngine::new("sh", ["-c", "exit 0"]).with_start_timeout(Duration::from_secs(30));
        let answer = run_within(Duration::from_secs(10), &engine, job_larger_than_a_pipe()).await;
        let error = answer.err();
        assert!(
            matches!(error, Some(EngineError::Unavailable { .. })),
            "{error:?}"
        );
    }

    /// What the worker started is killed with it, as at every other end of a
    /// worker: dropping the worker alone would leave an rclone running.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_start_timeout_also_kills_the_processes_the_worker_started() {
        let dir = tempfile::tempdir().unwrap();
        let child_file = dir.path().join("child.pid");
        let script = format!(
            "sleep 30 & echo $! >'{}'; exec sleep 30",
            child_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_start_timeout(Duration::from_secs(1));
        let answer = run_within(Duration::from_secs(5), &engine, job_larger_than_a_pipe()).await;
        assert_eq!(answer.err(), Some(EngineError::StartTimedOut));
        let child = pid_from(&child_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker started outlived the start timeout"
        );
    }

    /// A worker that timed out at start never took its job, and the daemon's
    /// own record of its cache slot must let go too: the next worker of the
    /// repository gets the same slot, and not one more, until the slots run
    /// out and workers run without a cache.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_cache_slot_of_a_worker_that_timed_out_at_start_is_free_for_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let first_ran = dir.path().join("first-worker-ran");
        let spec_file = dir.path().join("spec.json");
        // The first worker never reads its job. The one after it keeps the
        // job it is given, and reports a result.
        let script = format!(
            "if [ -e '{first}' ]; then cat >'{spec}'; echo '{result}'; \
             else touch '{first}'; exec sleep 30; fi",
            first = first_ran.display(),
            spec = spec_file.display(),
            result = result_line()
        );
        let cache_root = dir.path().join("cache");
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_cache_root(&cache_root)
            .with_start_timeout(Duration::from_secs(1));
        let id = repository("ab");
        let identify = |repository: String| {
            Job::Identify(RepositoryAccess {
                repository,
                repo_hot: None,
                password: SecretValue::new("demo"),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
                secrets: Vec::new(),
                id: Some(id.clone()),
            })
        };

        let timed_out = run_within(
            Duration::from_secs(5),
            &engine,
            identify("x".repeat(900_000)),
        )
        .await;
        assert_eq!(timed_out.err(), Some(EngineError::StartTimedOut));

        let RunningJob {
            events,
            control: _control,
        } = run_within(
            Duration::from_secs(5),
            &engine,
            identify("/home/demo/repo".to_owned()),
        )
        .await
        .expect("the job after the timed out one was refused");
        let events = collected_within(Duration::from_secs(10), events).await;
        assert!(
            matches!(events.as_slice(), [EngineEvent::Succeeded(_)]),
            "{events:?}"
        );
        let spec: JobSpec = serde_json::from_slice(&std::fs::read(&spec_file).unwrap()).unwrap();
        assert_eq!(
            spec.cache_dir,
            Some(cache_dir(&cache_root, &id, 0)),
            "the slot of the worker that timed out stayed taken"
        );
    }

    // The first-line deadline: a worker that took its job and then says
    // nothing is as hung as one that never read it, and the daemon can't
    // tell it from one that is slowly opening a repository, except by the
    // time that passed since the job was written.

    /// A worker that read its job (the script says so with `cat`) and then
    /// stays silent: nothing it could still do makes it a worker to wait for.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_reads_its_job_but_stays_silent_is_failed_when_the_start_timeout_passes()
    {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!(
            "cat >/dev/null; echo $$ >'{}'; exec sleep 30",
            pid_file.display()
        );
        // Long enough for a loaded machine to start the shell.
        let timeout = Duration::from_secs(1);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_start_timeout(timeout);
        let began = Instant::now();
        let RunningJob {
            events,
            mut control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` has no first event to wait for, and must not fail");
        let events = collected_within(Duration::from_secs(6), events).await;
        let took = began.elapsed();
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
        assert!(
            took >= timeout.saturating_sub(Duration::from_millis(50)),
            "gave up before the start timeout: {took:?}"
        );
        let worker = pid_from(&pid_file).await;
        assert!(
            gone_soon(worker).await,
            "the silent worker outlived the start timeout"
        );
        // Nobody stopped the job, and its control is still here: the worker
        // is gone all the same.
        tokio::time::timeout(Duration::from_secs(5), control.stop())
            .await
            .expect("stopping a worker that was killed already hung");
    }

    /// As everywhere a worker ends, what it started goes with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_start_timeout_of_a_silent_worker_also_kills_the_processes_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let child_file = dir.path().join("child.pid");
        // The child's output goes elsewhere, so that the worker's own
        // descriptor is the last one on the pipe.
        let script = format!(
            "cat >/dev/null; sleep 30 >/dev/null & echo $! >'{}'; exec sleep 30",
            child_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_start_timeout(Duration::from_secs(1));
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        let events = collected_within(Duration::from_secs(6), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
        let child = pid_from(&child_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker started outlived the start timeout"
        );
    }

    /// One deadline covers the write of the job and the wait for the first
    /// line: a worker that is slow to read its job has that much less time to
    /// answer. The job is larger than a pipe, so the write waits for the
    /// worker's reading.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_start_timeout_counts_from_the_start_of_the_write_of_the_job() {
        let script = "sleep 3; cat >/dev/null; exec sleep 30";
        let timeout = Duration::from_secs(4);
        let engine = WorkerEngine::new("sh", ["-c", script]).with_start_timeout(timeout);
        let began = Instant::now();
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, job_larger_than_a_pipe())
            .await
            .expect("a worker that read its job in time was timed out");
        let events = collected_within(Duration::from_secs(10), events).await;
        let took = began.elapsed();
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
        assert!(
            took < Duration::from_millis(5500),
            "the wait for the first line started over after the write: {took:?}"
        );
    }

    /// A line is not there before its newline: a worker that starts one and
    /// stalls has not said anything yet.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_stalls_in_its_first_line_is_failed_when_the_start_timeout_passes() {
        let script = r#"cat >/dev/null; printf '{"event":"star'; exec sleep 30"#;
        let engine =
            WorkerEngine::new("sh", ["-c", script]).with_start_timeout(Duration::from_secs(1));
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        let events = collected_within(Duration::from_secs(6), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
    }

    fn started_line() -> String {
        serde_json::to_string(&WorkerEvent::Started {
            op_id: OperationId::from_uuid(Uuid::from_u128(1)),
            pid: 1,
            index_read_at: domain::Timestamp::from_second(1_800_000_000).unwrap(),
        })
        .unwrap()
    }

    fn log_line() -> String {
        serde_json::to_string(&WorkerEvent::Log {
            level: Level::Info,
            message: "opening the repository".to_owned(),
        })
        .unwrap()
    }

    /// Runs `script`, which reads its job first, under a start timeout of
    /// `timeout`, and returns the events.
    #[cfg(unix)]
    async fn events_under_a_start_timeout(timeout: Duration, script: &str) -> Vec<EngineEvent> {
        let engine = WorkerEngine::new("sh", ["-c", script]).with_start_timeout(timeout);
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        collected_within(Duration::from_secs(15), events).await
    }

    /// Once the worker has been heard from it is not on the clock any more:
    /// a repository can be slow to open, and then a backup slow to finish.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_sent_its_first_event_in_time_may_go_quiet_for_longer_than_the_start_timeout()
     {
        let script = format!(
            "cat >/dev/null; echo '{}'; sleep 2.5; echo '{}'",
            started_line(),
            result_line()
        );
        let events = events_under_a_start_timeout(Duration::from_secs(1), &script).await;
        assert!(
            matches!(
                events.as_slice(),
                [EngineEvent::Started { .. }, EngineEvent::Succeeded(_)]
            ),
            "{events:?}"
        );
    }

    /// A `log` line is a line like any other: it shows that the worker lives,
    /// and the `started` that follows may be slow, for it comes once the
    /// repository is open.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_whose_first_line_is_a_log_line_may_go_quiet_for_longer_than_the_start_timeout()
     {
        let script = format!(
            "cat >/dev/null; echo '{}'; sleep 2.5; echo '{}'",
            log_line(),
            result_line()
        );
        let events = events_under_a_start_timeout(Duration::from_secs(1), &script).await;
        assert!(
            matches!(events.as_slice(), [EngineEvent::Succeeded(_)]),
            "{events:?}"
        );
    }

    /// The timeout bounds the wait for the first line, not the worker: one
    /// that answers in time is reported as it always was.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_answers_within_the_start_timeout_is_not_timed_out() {
        let script = format!("cat >/dev/null; sleep 0.3; echo '{}'", result_line());
        let events = events_under_a_start_timeout(Duration::from_secs(5), &script).await;
        assert!(
            matches!(events.as_slice(), [EngineEvent::Succeeded(_)]),
            "{events:?}"
        );
    }

    /// A timeout too long to be a time never passes, and doesn't panic
    /// where it is added to the clock.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_start_timeout_too_long_to_represent_never_passes() {
        let script = format!("cat >/dev/null; sleep 0.3; echo '{}'", result_line());
        let engine =
            WorkerEngine::new("sh", ["-c", script.as_str()]).with_start_timeout(Duration::MAX);
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        let events = collected_within(Duration::from_secs(10), events).await;
        assert!(
            matches!(events.as_slice(), [EngineEvent::Succeeded(_)]),
            "{events:?}"
        );
    }

    /// A worker that closes its output produces no more events, and the
    /// daemon waits for its exit to tell what became of it. One that lives on
    /// would be waited for forever: the exit timeout kills it, and says why.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_closes_its_output_but_lives_on_is_killed_when_the_exit_timeout_passes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!(
            "cat >/dev/null; echo $$ >'{}'; exec >&-; exec sleep 30",
            pid_file.display()
        );
        let timeout = Duration::from_millis(300);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_exit_timeout(timeout);
        let RunningJob {
            events,
            mut control,
        } = run(&engine).await;
        let began = Instant::now();
        let events = collected_within(Duration::from_secs(5), events).await;
        let took = began.elapsed();
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineExitTimedOut)]
        );
        assert!(
            took >= timeout.saturating_sub(Duration::from_millis(50)),
            "the exit timeout fired early: {took:?}"
        );
        // Nobody stopped the job, and its control is still here: the worker
        // is gone all the same.
        let worker = pid_from(&pid_file).await;
        assert!(
            gone_soon(worker).await,
            "the worker outlived the exit timeout"
        );
        tokio::time::timeout(Duration::from_secs(5), control.stop())
            .await
            .expect("stopping a worker that was killed already hung");
    }

    /// Neither does the kill leave behind what the worker started.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_exit_timeout_also_kills_the_processes_the_worker_started() {
        let dir = tempfile::tempdir().unwrap();
        let child_file = dir.path().join("child.pid");
        // The child's output goes elsewhere, so that the worker's own
        // descriptor is the last one on the pipe.
        let script = format!(
            "cat >/dev/null; sleep 30 >/dev/null & echo $! >'{}'; exec >&-; exec sleep 30",
            child_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_exit_timeout(Duration::from_millis(300));
        let RunningJob {
            events,
            control: _control,
        } = run(&engine).await;
        let events = collected_within(Duration::from_secs(5), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineExitTimedOut)]
        );
        let child = pid_from(&child_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker started outlived the exit timeout"
        );
    }

    /// The exit timeout bounds the wait, not the worker: one that closes its
    /// output and exits in time is reported as it always was.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_closes_its_output_and_exits_within_the_exit_timeout_keeps_its_code() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; exec >&-; sleep 0.2; exit 3"])
            .with_exit_timeout(Duration::from_secs(5));
        let RunningJob {
            events,
            control: _control,
        } = run(&engine).await;
        let events = collected_within(Duration::from_secs(10), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineExited {
                exit_code: Some(3)
            })]
        );
    }

    /// A stop that is under way ends the events silently, as it always does:
    /// the exit timeout passing meanwhile adds no failure to them.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_requested_while_the_exit_is_awaited_ends_the_events_without_a_failure() {
        // The worker ignores SIGTERM, so the stop is still waiting out its
        // grace period when the exit timeout passes.
        let script = "trap '' TERM; cat >/dev/null; exec >&-; exec sleep 30";
        let engine = WorkerEngine::new("sh", ["-c", script])
            .with_stop_grace(Duration::from_secs(2))
            .with_exit_timeout(Duration::from_secs(1));
        let RunningJob {
            events,
            mut control,
        } = run(&engine).await;
        let stopping = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            control.stop().await;
        };
        let (events, ()) = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(events.collect::<Vec<_>>(), stopping)
        })
        .await
        .expect("the stop or the events never ended");
        assert!(
            events.is_empty(),
            "a stopped job's events ended with {events:?}"
        );
    }
}
