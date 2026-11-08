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
    Job, JobControl, ProgressEvent, ProgressKind as AppProgressKind, PruneSummary,
    RepositoryAccess, RunningJob,
};
use async_trait::async_trait;
use domain::execution::{FailureReason, Warning, WarningKind};
use domain::{OperationId, RepositoryId};
use futures::stream::{self, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::{mpsc, watch};

use crate::cache_lock::CacheLock;
use crate::job::{
    BackupJobSpec, CheckJobSpec, FakeEndSpec, FakeJobSpec, FakeWarning, JobKind, JobSpec, Level,
    MAX_MESSAGE_BYTES, OpResult, ProgressKind, PruneJobSpec, RepositorySpec, WorkerEvent,
    exit_code,
};

/// How long a worker may keep running after it reported its result, for
/// cleanup, before it is killed.
const AFTER_RESULT_GRACE: Duration = Duration::from_secs(10);

/// How long the events of a job whose worker has exited may go on arriving.
/// What the worker wrote is in the pipe already, so this is only the time to
/// read it: a process the worker left running can hold the pipe open for as
/// long as it lives, and the events end with the worker, not with it.
const DRAIN_AFTER_EXIT: Duration = Duration::from_secs(1);

/// How long writing a job to a new worker may take before the worker counts
/// as hung and is killed: the default of [`WorkerEngine::with_start_timeout`].
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a worker that closed its output may go on living before it is
/// killed: the default of [`WorkerEngine::with_exit_timeout`].
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most cache slots one repository gets. Looking for a free one stops
/// here, so a lock that is wrongly always held can't send the search on to
/// the end of the numbers.
const MAX_CACHE_SLOTS: u32 = 64;

/// Runs each operation in a new worker process: `program args…`, normally
/// `steading worker`. A worker leads a process group of its own, and what is
/// done to the worker is done to the group, so the programs it started (an
/// rclone, say) end with it.
///
/// Stopping a job sends SIGTERM and, after a grace period, SIGKILL. Dropping
/// a [`RunningJob`] before its result kills the worker at once; after the
/// result, the worker gets a grace period to finish its cleanup.
///
/// A worker that hangs doesn't hold the daemon. One that doesn't read its job
/// within the start timeout, and one that closes its output but is still
/// there after the exit timeout, are killed.
pub struct WorkerEngine {
    program: PathBuf,
    args: Vec<OsString>,
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
            cache_root: None,
            stop_grace: Duration::from_secs(10),
            watchdog_interval: Duration::from_secs(5),
            start_timeout: START_TIMEOUT,
            exit_timeout: EXIT_TIMEOUT,
            slots: Arc::new(CacheSlots::default()),
        }
    }

    /// Gives each worker of a repository with a known ID its own rustic cache
    /// under `root/steading/<repository id>/<slot>`. Without a root, workers
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

    /// How long writing a job to a new worker's stdin may take, 30 seconds by
    /// default. A worker that never reads its job, because it hung before it
    /// got to it, leaves the write blocked once the pipe is full. When the
    /// time has passed, the worker and what it started are killed, and
    /// [`Engine::run`] fails with [`EngineError::StartTimedOut`]. Only the
    /// write is bounded, however slowly the worker reads: a write that
    /// finishes in time is not affected, and one that fails is
    /// [`EngineError::Unavailable`], as ever.
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
        // A worker that never reads its job, because it hung before it got to
        // it, leaves this write blocked once the pipe is full.
        match tokio::time::timeout(self.start_timeout, stdin.write_all(&spec)).await {
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
        tokio::spawn(forward_stderr(BufReader::new(stderr), exited.clone()));
        let stopped = Arc::new(AtomicBool::new(false));
        let reading = Reading {
            stdout: BufReader::new(stdout),
            line: Vec::new(),
            exited: exited.clone(),
            commands: commands.clone(),
            stopped: Arc::clone(&stopped),
            done: false,
            drain_until: None,
            exit_timeout: self.exit_timeout,
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
    /// SIGKILL at once: the worker closed its output and lives on.
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
    /// How long the exit of a worker that closed its output is waited for.
    exit_timeout: Duration,
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
                Ok(Line::Complete) => {}
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
            if let Some(event) = translate(event) {
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
    /// While the worker runs, the wait is as long as it takes. Once it has
    /// exited, what it wrote is in the pipe already, so reading goes on for
    /// that, but for [`DRAIN_AFTER_EXIT`] in all: nothing is waited for on a
    /// pipe that another process holds open.
    async fn read_next_line(&mut self) -> std::io::Result<Line> {
        let Self {
            stdout,
            line,
            exited,
            drain_until,
            ..
        } = self;
        let reading = read_line(stdout, line);
        tokio::pin!(reading);
        tokio::select! {
            // What is in the pipe goes before the worker's exit.
            biased;
            read = &mut reading => return read,
            () = worker_exited(exited) => {}
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
        // Dropping the job kills the worker.
        self.done = true;
        EngineEvent::Failed(FailureReason::EngineFailed { detail })
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
async fn forward_stderr(mut stderr: BufReader<ChildStderr>, mut exited: watch::Receiver<Exited>) {
    let mut line = Vec::new();
    let mut drain_until = None;
    loop {
        line.clear();
        match next_log_line(&mut stderr, &mut line, &mut exited, &mut drain_until).await {
            Ok(Some(cut)) => {
                if !line.is_empty() || cut {
                    log::log!(
                        target: "steading::worker",
                        log::Level::Warn,
                        "{}{}",
                        String::from_utf8_lossy(&line),
                        if cut { "…" } else { "" }
                    );
                }
            }
            Ok(None) | Err(_) => return,
        }
    }
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

fn translate(event: WorkerEvent) -> Option<EngineEvent> {
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
                prefix,
            })
        }
        WorkerEvent::ProgressLength { key, len } => {
            EngineEvent::Progress(ProgressEvent::Length { key, len })
        }
        WorkerEvent::ProgressInc { key, inc } => {
            EngineEvent::Progress(ProgressEvent::Advance { key, by: inc })
        }
        WorkerEvent::ProgressTitle { key, title } => {
            EngineEvent::Progress(ProgressEvent::Title { key, title })
        }
        WorkerEvent::ProgressFinish { key } => EngineEvent::Progress(ProgressEvent::Finish { key }),
        WorkerEvent::Warning {
            item,
            during,
            code,
            detail,
        } => EngineEvent::Warning(Warning {
            kind: match code {
                WarningCode::SourceUnreadable => WarningKind::SourceUnreadable,
                WarningCode::Unknown => WarningKind::Other,
            },
            item,
            during,
            detail,
        }),
        // Logs are for the daemon's own log, which gets them under the target
        // `steading::worker`.
        WorkerEvent::Log { level, message } => {
            let level = match level {
                Level::Error => log::Level::Error,
                Level::Warn => log::Level::Warn,
                Level::Info => log::Level::Info,
                Level::Debug => log::Level::Debug,
            };
            log::log!(target: "steading::worker", level, "{message}");
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
        }),
        WorkerEvent::Failed { code, detail, .. } => {
            let detail = detail.unwrap_or_default();
            EngineEvent::Failed(match code {
                ErrorCode::InvalidJobSpec => FailureReason::JobRejected { detail },
                ErrorCode::DeadlinePassed => FailureReason::DeadlinePassed,
                // A worker only ever says one of the codes above or
                // `operation-failed`. The daemon's own codes are the log's
                // words for what the daemon saw, and the request-check codes
                // belong to the HTTP adapter: a worker that sent one has failed
                // in a way this side doesn't know, so it is reported as the
                // failure it claims, in its own words.
                ErrorCode::OperationFailed
                | ErrorCode::EngineUnavailable
                | ErrorCode::EngineExited
                | ErrorCode::ProtocolViolated
                | ErrorCode::EngineStartTimedOut
                | ErrorCode::EngineExitTimedOut
                | ErrorCode::HostNotAllowed
                | ErrorCode::OriginNotAllowed
                | ErrorCode::JsonRequired
                | ErrorCode::Unknown => FailureReason::EngineFailed { detail },
            })
        }
    })
}

fn repository_id(job: &Job) -> Option<&RepositoryId> {
    match job {
        Job::Fake(_) => None,
        Job::Identify(access) => access.id.as_ref(),
        Job::Backup(job) => job.repository.id.as_ref(),
        Job::Prune(job) => job.repository.id.as_ref(),
        Job::Check(job) => job.repository.id.as_ref(),
    }
}

fn repository_spec(access: RepositoryAccess) -> RepositorySpec {
    RepositorySpec {
        repository: access.repository,
        password: access.password,
        options: access.options,
    }
}

fn job_kind(job: Job) -> Result<JobKind, EngineError> {
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
            root.join("steading")
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
    use domain::secrets::SecretValue;

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
            translate(failed(ErrorCode::InvalidJobSpec)),
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
                translate(failed(code)),
                Some(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "bad spec".to_owned()
                }))
            );
        }
    }

    #[test]
    fn a_deadline_failure_from_the_worker_is_a_passed_deadline() {
        assert_eq!(
            translate(failed(ErrorCode::DeadlinePassed)),
            Some(EngineEvent::Failed(FailureReason::DeadlinePassed))
        );
    }

    fn repository(byte: &str) -> RepositoryId {
        RepositoryId::parse(&byte.repeat(32)).unwrap()
    }

    fn cache_dir(root: &Path, id: &RepositoryId, slot: u32) -> PathBuf {
        root.join("steading")
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
        let scratch = std::env::temp_dir().join(format!("steading-reap-{}", std::process::id()));
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
        let scratch = std::env::temp_dir().join(format!("steading-drop-{}", std::process::id()));
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

    /// What the capturing logger was given for `steading::worker`.
    static WORKER_LOG: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target() == "steading::worker"
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
        let engine = WorkerEngine::new("/nonexistent/steading-worker", ["worker"]);
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
                password: SecretValue::new("demo"),
                options: BTreeMap::new(),
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
