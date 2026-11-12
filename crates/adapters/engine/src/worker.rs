//! The worker side: read one [`JobSpec`] from stdin, run it, and report
//! [`WorkerEvent`]s on stdout, one JSON object per line.
//!
//! The worker is a process of its own (technical design §4.1) because
//! `rustic_core` can't be cancelled, keeps whole indexes in memory and runs
//! its own Tokio runtimes. [`main`] is what `ladderback worker` does: it routes
//! rustic's logging into the event stream, exits on SIGTERM, and runs the job.
//!
//! Nothing the job holds secret reaches the stream: the worker masks the
//! password, the secrets its settings resolved to, the credential options and
//! the passwords in URLs, in every text it writes. What the programs it
//! starts write to stderr, it can't mask; the daemon does that.

mod cancel;
mod convert;
mod events;
mod fake;
mod jobs;
mod listing;
mod logging;
mod panic_hook;
mod progress;
mod repository;
mod stdio;
mod tmp_files;
mod watchdog;

use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use api::ErrorCode;
use domain::execution::deadline::Deadline;
use domain::{OperationId, RepositoryId, SignedDuration, Timestamp};

use crate::cache_lock::CacheLock;
use crate::job::{
    JobKind, JobSpec, Level, MAX_MESSAGE_BYTES, MAX_MESSAGE_READ, OpResult, RepositorySpec,
    WorkerEvent, exit_code,
};
use cancel::{AfterCancel, Cancel, Cause};
use convert::JobError;
use events::Events;
use watchdog::{Terminate, WallClock, Watchdog};

/// How the worker process should exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The job ran and its result or failure was reported.
    Reported { failed: bool },
    /// A cancel ended the job, which reports nothing more.
    Cancelled,
    /// The deadline ended a backup that wound down. Its failure is the
    /// stream's last line, written by the watchdog or else at the exit.
    DeadlinePassed,
    /// Stop at once with this code, reporting nothing more: a fake job
    /// imitating a crashed engine.
    Abrupt(u8),
}

impl Exit {
    pub fn code(self) -> i32 {
        match self {
            Self::Reported { failed: false } => 0,
            Self::Reported { failed: true } => 1,
            Self::Cancelled => exit_code::TERMINATED,
            Self::DeadlinePassed => exit_code::DEADLINE_PASSED,
            Self::Abrupt(code) => i32::from(code),
        }
    }
}

/// What `ladderback worker` does. Never returns normally: the process exits
/// with the job's [`Exit`] code, or 74 when the events can't be written.
pub fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    die_with_the_daemon();
    // Descriptor 1 stops being the event pipe: nothing a library prints, or a
    // program the worker starts, can reach the stream or hold it open.
    #[cfg(unix)]
    let stream = match stdio::detach_stdout() {
        Ok(stream) => stream,
        Err(error) => {
            say(&format!("cannot set up the event stream: {error}"));
            std::process::exit(exit_code::EVENTS_UNWRITABLE);
        }
    };
    #[cfg(not(unix))]
    let stream = io::stdout();
    // Rustic's progress and log callbacks have nobody to tell about a stream
    // that can't be written, so the worker gives itself up: the daemon is gone.
    let events =
        Events::with_broken_stream(stream, || std::process::exit(exit_code::EVENTS_UNWRITABLE));
    panic_hook::install(events.clone());
    if let Err(error) = logging::install(events.clone()) {
        say(&format!("cannot route logs into events: {error}"));
    }
    let process = Process::real();
    #[cfg(unix)]
    if let Err(error) = exit_on_sigterm(events.clone(), process.cancel.clone()) {
        say(&format!("cannot handle SIGTERM: {error}"));
    }
    match run_events(io::stdin().lock(), events, std::process::id(), process) {
        // An abrupt exit may use any code, including ones outside u8.
        Ok(exit) => std::process::exit(exit.code()),
        Err(error) => {
            say(&format!("cannot write events: {error}"));
            std::process::exit(exit_code::EVENTS_UNWRITABLE)
        }
    }
}

/// A line on stderr, for whoever started the worker. A worker that can't
/// write to stderr has nothing better to do.
fn say(text: &str) {
    let _ = writeln!(io::stderr(), "ladderback worker: {text}");
}

/// Asks the kernel for a SIGTERM when the process that started this worker
/// is gone, so a daemon that dies (a crash, the OOM killer, SIGKILL) doesn't
/// leave workers running against the repository. Linux only: elsewhere a
/// worker ends at its next event, when its pipe is broken.
///
/// The kernel ties the signal to the parent *thread*, so the daemon must
/// start workers from a thread that lives as long as it does, as a runtime's
/// worker threads do. Without this call, nothing bounds a worker whose daemon
/// died while it was between two events.
#[cfg(target_os = "linux")]
fn die_with_the_daemon() {
    use nix::sys::prctl::set_pdeathsig;
    use nix::sys::signal::Signal;

    if let Err(error) = set_pdeathsig(Signal::SIGTERM) {
        say(&format!(
            "cannot ask for SIGTERM when the daemon dies: {error}"
        ));
    }
}

/// Runs the job read from `input`, writing events to `output`. Fails only if
/// `output` can't be written, for example because the daemon went away.
///
/// Unlike [`main`], this leaves the process alone: no logger, no signal
/// handler. A job with a deadline still exits the process when it passes.
pub fn run(input: impl Read, output: impl Write + Send + 'static, pid: u32) -> io::Result<Exit> {
    run_events(input, Events::new(output), pid, Process::real())
}

/// The process-level effects a deadline needs, injectable for tests.
#[derive(Clone)]
pub(crate) struct Process {
    clock: WallClock,
    terminate: Terminate,
    cancel: Cancel,
    /// How long a backup stopped while it archives may wind down before the
    /// worker exits anyway: [`WIND_DOWN_LIMIT`] but in tests.
    wind_down_limit: Duration,
}

impl Process {
    fn real() -> Self {
        Self {
            clock: Arc::new(Timestamp::now),
            terminate: Arc::new(|code| {
                std::process::exit(code);
            }),
            cancel: Cancel::default(),
            wind_down_limit: WIND_DOWN_LIMIT,
        }
    }
}

/// What a finished job leaves for the worker to report.
pub(crate) enum Done {
    Result(OpResult),
    /// The job wrote its result itself, with [`Context::report`], and has
    /// nothing more to say.
    Reported,
    /// A cancel, or the deadline, ended the job: exit as its cause says,
    /// reporting nothing more.
    Cancelled,
    /// Exit at once with this code and report nothing more.
    Abrupt(u8),
}

/// Everything a job needs from the worker around it.
pub(crate) struct Context {
    pub(crate) events: Events,
    pub(crate) cache_dir: Option<PathBuf>,
    op_id: OperationId,
    pid: u32,
    watchdog_interval: Duration,
    process: Process,
    watchdog: RefCell<Option<Watchdog>>,
    /// Held until the worker ends: see [`Context::lock_cache`].
    cache_lock: RefCell<Option<CacheLock>>,
}

impl Context {
    fn new(events: Events, spec: &JobSpec, pid: u32, process: Process) -> Self {
        Self {
            events,
            cache_dir: spec.cache_dir.clone(),
            op_id: spec.op_id,
            pid,
            watchdog_interval: Duration::from_millis(spec.watchdog_interval_ms),
            process,
            watchdog: RefCell::new(None),
            cache_lock: RefCell::new(None),
        }
    }

    /// Takes `dir` as this worker's cache until the worker ends. Two workers
    /// must not share a cache directory (§4.1), and a daemon that was
    /// restarted can't know which ones are in use, so a directory another
    /// worker holds is refused: the caller runs without a cache instead.
    pub(crate) fn lock_cache(&self, dir: &Path) -> io::Result<()> {
        *self.cache_lock.borrow_mut() = Some(CacheLock::acquire(dir)?);
        Ok(())
    }

    /// Reports that the job began its work on `repository`, the one it
    /// opened, if any, and arms the deadline watchdog when the job has a
    /// budget. Called right before the repository index is read, because
    /// the deadline counts from there. A deadline that has passed already
    /// ends the process before this returns.
    pub(crate) fn start(
        &self,
        budget: Option<SignedDuration>,
        repository: Option<&RepositoryId>,
    ) -> io::Result<()> {
        let index_read_at = (self.process.clock)();
        self.events.emit(&WorkerEvent::Started {
            op_id: self.op_id,
            pid: self.pid,
            index_read_at,
            repository: repository.cloned(),
        })?;
        if let Some(budget) = budget {
            let watchdog = Watchdog::start(
                Deadline::after_index_read(index_read_at, budget),
                self.watchdog_interval,
                self.events.clone(),
                &self.process,
            );
            *self.watchdog.borrow_mut() = Some(watchdog);
        }
        Ok(())
    }

    /// The job's work is done. What is left (reporting the result, dropping
    /// the repository) is not what the deadline guards, and must not be
    /// failed by a deadline that passes meanwhile.
    pub(crate) fn disarm(&self) {
        drop(self.watchdog.borrow_mut().take());
    }

    /// Writes the job's result now, for a job whose cleanup comes after it:
    /// the result must not wait for the repository to be dropped.
    pub(crate) fn report(&self, result: OpResult) -> io::Result<()> {
        self.events.emit(&WorkerEvent::Result(result))
    }

    pub(crate) fn log(&self, level: Level, message: String) -> io::Result<()> {
        self.events.emit(&WorkerEvent::Log { level, message })
    }

    /// The worker's cancel: what a SIGTERM asks for.
    pub(crate) fn cancel(&self) -> &Cancel {
        &self.process.cancel
    }
}

fn run_events(input: impl Read, events: Events, pid: u32, process: Process) -> io::Result<Exit> {
    // A job spec is a few kilobytes; one past the protocol's message cap is
    // refused, and never read past the cap.
    let mut raw = String::new();
    let spec = match input
        .take(MAX_MESSAGE_READ)
        .read_to_string(&mut raw)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            if raw.len() > MAX_MESSAGE_BYTES {
                Err(format!("job spec exceeds {MAX_MESSAGE_BYTES} bytes"))
            } else {
                serde_json::from_str::<JobSpec>(&raw).map_err(|e| e.to_string())
            }
        }) {
        Ok(spec) => spec,
        Err(detail) => {
            events.emit(&WorkerEvent::Failed {
                code: ErrorCode::InvalidJobSpec,
                args: Vec::new(),
                detail: Some(detail),
            })?;
            return Ok(Exit::Reported { failed: true });
        }
    };
    // From here on, the secrets of the job never reach the stream (I4).
    if let Some(repository) = repository_of(&spec.job) {
        events.redact_with(repository.redactor());
    }
    // The job is taken. The daemon times a worker out that has said nothing
    // by its start timeout, and opening a repository can take longer than
    // that: this line, which holds nothing of the job, comes before it.
    events.emit(&WorkerEvent::Log {
        level: Level::Debug,
        message: "job accepted".to_owned(),
    })?;
    let ctx = Context::new(events.clone(), &spec, pid, process);
    let outcome = match &spec.job {
        JobKind::Fake(job) => fake::run(&ctx, job),
        JobKind::Identify(repository) => jobs::identify(&ctx, repository),
        JobKind::Init(repository) => jobs::init(&ctx, repository),
        JobKind::Backup(job) => jobs::backup(&ctx, job),
        JobKind::Forget(job) => jobs::forget(&ctx, job),
        JobKind::Rewrite(job) => jobs::rewrite(&ctx, job),
        JobKind::Prune(job) => jobs::prune(&ctx, job),
        JobKind::PrunePlan(job) => jobs::prune_plan(&ctx, job),
        JobKind::Check(job) => jobs::check(&ctx, job),
        JobKind::Ls(repository) => jobs::ls(&ctx, repository),
    };
    match outcome {
        Ok(Done::Result(result)) => {
            events.emit(&WorkerEvent::Result(result))?;
            Ok(Exit::Reported { failed: false })
        }
        Ok(Done::Reported) => Ok(Exit::Reported { failed: false }),
        Ok(Done::Cancelled) => Ok(exit_of_cancelled(&events, ctx.cancel())),
        Ok(Done::Abrupt(code)) => Ok(Exit::Abrupt(code)),
        Err(JobError::Failed { code, detail }) => {
            events.emit(&WorkerEvent::Failed {
                code,
                args: Vec::new(),
                detail: Some(detail),
            })?;
            Ok(Exit::Reported { failed: true })
        }
        Err(JobError::Events(error)) => Err(error),
    }
}

/// How a job the cancel ended exits: as the cancel's first cause says. A
/// deadline's failure the watchdog couldn't write, because another line
/// was being written then, goes out now as the stream's last line.
fn exit_of_cancelled(events: &Events, cancel: &Cancel) -> Exit {
    match cancel.cause() {
        Some(Cause::Deadline) => {
            // Nothing happens when the watchdog wrote it, or a SIGTERM
            // ended the stream since.
            events.end(Some(&watchdog::deadline_failure()), LAST_LINE_PATIENCE);
            Exit::DeadlinePassed
        }
        Some(Cause::Cancel) | None => Exit::Cancelled,
    }
}

fn repository_of(job: &JobKind) -> Option<&RepositorySpec> {
    match job {
        JobKind::Fake(_) => None,
        JobKind::Identify(repository) | JobKind::Init(repository) => Some(repository),
        JobKind::Ls(repository) => Some(repository),
        JobKind::Backup(job) => Some(&job.repo),
        JobKind::Forget(job) => Some(&job.repo),
        JobKind::Rewrite(job) => Some(&job.repo),
        JobKind::Prune(job) => Some(&job.repo),
        JobKind::PrunePlan(job) => Some(&job.repo),
        JobKind::Check(job) => Some(&job.repo),
    }
}

/// How long a backup stopped while it archives, by SIGTERM or by its
/// deadline, may wind down before the worker exits anyway. The daemon's
/// grace period (10 s by default) ends it sooner; this bounds a worker whose
/// daemon is gone.
const WIND_DOWN_LIMIT: Duration = Duration::from_secs(30);

/// How long the worker waits for a line being written before it ends the
/// stream: a writer stuck on a full pipe must not hold its exit back.
const LAST_LINE_PATIENCE: Duration = Duration::from_millis(250);

/// On SIGTERM the worker waits for the line it is writing, writes nothing
/// more and exits. Nothing in rustic needs a clean shutdown: its writes are
/// safe to interrupt anywhere (§4.1), which the kill-at-random-points test
/// proves. A backup that is saving data winds down instead (`Cancel`): it
/// lets rustic save the index of the packs it wrote, writes no snapshot,
/// and ends the worker itself, with the same code.
#[cfg(unix)]
fn exit_on_sigterm(events: Events, cancel: Cancel) -> io::Result<()> {
    use signal_hook::consts::SIGTERM;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGTERM])?;
    std::thread::Builder::new()
        .name("sigterm".to_owned())
        .spawn(move || {
            if signals.forever().next().is_some() {
                let after = cancel.request();
                events.end(None, LAST_LINE_PATIENCE);
                if after == AfterCancel::WindDown {
                    std::thread::sleep(WIND_DOWN_LIMIT);
                }
                std::process::exit(crate::job::exit_code::TERMINATED);
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc;

    use domain::secrets::SecretValue;

    use super::events::testing::capture;
    use super::*;
    use crate::job::{FakeEndSpec, FakeJobSpec};

    fn fake_spec(budget: Option<SignedDuration>, steps: u32) -> JobSpec {
        JobSpec {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(9)),
            cache_dir: None,
            watchdog_interval_ms: 10,
            job: JobKind::Fake(FakeJobSpec {
                steps,
                step_delay_ms: 0,
                warnings: Vec::new(),
                end: FakeEndSpec::Succeed,
                deadline_budget: budget,
            }),
        }
    }

    #[test]
    fn an_unreadable_job_spec_is_reported_as_a_failure() {
        let (events, capture) = capture();
        let exit = run_events(&b"{not json"[..], events, 42, Process::real()).unwrap();
        assert_eq!(exit.code(), 1);
        let events = capture.parsed();
        let [WorkerEvent::Failed { code, detail, .. }] = events.as_slice() else {
            panic!("expected exactly one failure event");
        };
        assert_eq!(*code, ErrorCode::InvalidJobSpec);
        assert!(detail.is_some());
    }

    /// The daemon gives a worker a start timeout to take its job, and opening
    /// a repository can take longer than that: the worker says it has the job
    /// before it opens anything, in a line the daemon counts as proof of
    /// life and logs at the debug level.
    #[test]
    fn a_worker_says_it_took_its_job_before_it_starts_the_work() {
        let (events, capture) = capture();
        let input = serde_json::to_vec(&fake_spec(None, 1)).unwrap();
        run_events(input.as_slice(), events, 42, Process::real()).unwrap();
        let events = capture.parsed();
        let [
            WorkerEvent::Log { level, .. },
            WorkerEvent::Started { .. },
            ..,
        ] = events.as_slice()
        else {
            panic!("expected a log line, then Started: {events:?}");
        };
        assert_eq!(*level, Level::Debug);
    }

    /// The line comes before the repository is opened, so that a job whose
    /// opening never ends has sent it: here the opening fails, and the line
    /// is still the first of the stream.
    #[test]
    fn a_worker_says_it_took_its_job_before_it_opens_the_repository() {
        let spec = JobSpec {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(9)),
            cache_dir: None,
            watchdog_interval_ms: 10,
            job: JobKind::Identify(RepositorySpec {
                repository: "/nonexistent/repo".to_owned(),
                repo_hot: None,
                password: SecretValue::new("hunter2"),
                options: Default::default(),
                options_hot: Default::default(),
                options_cold: Default::default(),
                secrets: Vec::new(),
                id: None,
            }),
        };
        let (events, capture) = capture();
        let input = serde_json::to_vec(&spec).unwrap();
        let exit = run_events(input.as_slice(), events, 1, Process::real()).unwrap();
        assert_eq!(exit.code(), 1);
        let events = capture.parsed();
        assert!(
            matches!(
                events.first(),
                Some(WorkerEvent::Log {
                    level: Level::Debug,
                    ..
                })
            ),
            "the stream doesn't start with the log line: {events:?}"
        );
        assert!(
            matches!(events.last(), Some(WorkerEvent::Failed { .. })),
            "{events:?}"
        );
    }

    #[test]
    fn every_event_is_one_line_of_json() {
        let spec = fake_spec(None, 2);
        let (events, capture) = capture();
        let input = serde_json::to_vec(&spec).unwrap();
        let exit = run_events(input.as_slice(), events, 42, Process::real()).unwrap();
        assert_eq!(exit, Exit::Reported { failed: false });
        let events = capture.parsed();
        let Some(WorkerEvent::Started { op_id, pid, .. }) = events
            .iter()
            .find(|event| !matches!(event, WorkerEvent::Log { .. }))
        else {
            panic!("the stream doesn't start with Started: {events:?}");
        };
        assert_eq!((*op_id, *pid), (spec.op_id, 42));
        assert_eq!(
            events.last(),
            Some(&WorkerEvent::Result(OpResult::Fake { steps_completed: 2 }))
        );
    }

    /// A backup the cancel ended reports nothing more, neither a result nor
    /// a failure, and the worker exits as a SIGTERM's, so that the daemon
    /// tells it from a crash when the SIGTERM wasn't its own.
    #[test]
    fn a_cancelled_backup_ends_the_worker_with_sigterms_code_and_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("notes.txt"), b"demo").unwrap();
        let repo = RepositorySpec {
            repository: dir.path().join("repo").to_string_lossy().into_owned(),
            repo_hot: None,
            password: SecretValue::new("demo-password"),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
            secrets: Vec::new(),
            id: None,
        };
        let run = |job, process| {
            let spec = JobSpec {
                op_id: OperationId::from_uuid(domain::Uuid::from_u128(9)),
                cache_dir: None,
                watchdog_interval_ms: 10,
                job,
            };
            let (events, capture) = capture();
            let input = serde_json::to_vec(&spec).unwrap();
            let exit = run_events(input.as_slice(), events, 1, process).unwrap();
            (exit, capture.parsed())
        };
        let (exit, _) = run(JobKind::Init(repo.clone()), Process::real());
        assert_eq!(exit, Exit::Reported { failed: false });

        let process = Process::real();
        process.cancel.request();
        let backup = crate::job::BackupJobSpec {
            repo,
            sources: vec![source.to_string_lossy().into_owned()],
            plan: domain::PlanId::parse("home").unwrap(),
            agent: domain::AgentId::from_uuid(domain::Uuid::from_u128(1)),
            extra_tags: Vec::new(),
            label: None,
            host: None,
            filter: Default::default(),
            deadline_budget: SignedDuration::from_hours(1),
        };
        let (exit, events) = run(JobKind::Backup(backup), process);
        assert_eq!(exit.code(), crate::job::exit_code::TERMINATED);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, WorkerEvent::Result(_) | WorkerEvent::Failed { .. })),
            "{events:?}"
        );
        let snapshots = std::fs::read_dir(dir.path().join("repo/snapshots")).unwrap();
        assert_eq!(snapshots.count(), 0, "a cancelled backup wrote a snapshot");
    }

    #[test]
    fn a_job_with_a_budget_counts_it_from_the_started_event_and_is_stopped_when_it_ends() {
        let at = Timestamp::from_second(1_800_000_000).unwrap();
        let (tx, terminated) = mpsc::channel();
        let tx = Mutex::new(tx);
        let process = Process {
            clock: Arc::new(move || at),
            terminate: Arc::new(move |code| {
                let _ = tx.lock().unwrap().send(code);
            }),
            cancel: Cancel::default(),
            wind_down_limit: WIND_DOWN_LIMIT,
        };
        let (events, capture) = capture();
        let input = serde_json::to_vec(&fake_spec(Some(SignedDuration::ZERO), 3)).unwrap();
        run_events(input.as_slice(), events, 1, process).unwrap();
        assert_eq!(
            terminated.try_recv(),
            Ok(crate::job::exit_code::DEADLINE_PASSED)
        );
        // The fake job ran on after the injected "exit"; none of it was reported.
        let events: Vec<_> = capture
            .parsed()
            .into_iter()
            .filter(|event| !matches!(event, WorkerEvent::Log { .. }))
            .collect();
        let [
            WorkerEvent::Started { index_read_at, .. },
            WorkerEvent::Failed { code, .. },
        ] = events.as_slice()
        else {
            panic!("expected Started then the deadline failure: {events:?}");
        };
        assert_eq!(*index_read_at, at);
        assert_eq!(*code, ErrorCode::DeadlinePassed);
    }

    #[test]
    fn a_job_whose_work_is_done_is_not_held_to_its_deadline_any_more() {
        let seconds = Arc::new(std::sync::atomic::AtomicI64::new(1_800_000_000));
        let (tx, terminated) = mpsc::channel();
        let tx = Mutex::new(tx);
        let process = Process {
            clock: {
                let seconds = Arc::clone(&seconds);
                Arc::new(move || {
                    Timestamp::from_second(seconds.load(std::sync::atomic::Ordering::SeqCst))
                        .unwrap()
                })
            },
            terminate: Arc::new(move |code| {
                let _ = tx.lock().unwrap().send(code);
            }),
            cancel: Cancel::default(),
            wind_down_limit: WIND_DOWN_LIMIT,
        };
        let (events, capture) = capture();
        let spec = fake_spec(Some(SignedDuration::from_secs(60)), 0);
        let ctx = Context::new(events, &spec, 1, process);
        ctx.start(Some(SignedDuration::from_secs(60)), None)
            .unwrap();
        ctx.disarm();
        // The deadline passes, many watchdog intervals (10 ms) go by.
        seconds.fetch_add(120, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            terminated.try_recv().is_err(),
            "the worker was ended after its work was done"
        );
        assert!(
            matches!(capture.parsed().as_slice(), [WorkerEvent::Started { .. }]),
            "{:?}",
            capture.parsed()
        );
    }

    /// A process whose wall clock reads `seconds`, and whose "exit" only
    /// sends its code on the receiver.
    fn process_on(seconds: &Arc<std::sync::atomic::AtomicI64>) -> (Process, mpsc::Receiver<i32>) {
        let (tx, terminated) = mpsc::channel();
        let tx = Mutex::new(tx);
        let process = Process {
            clock: {
                let seconds = Arc::clone(seconds);
                Arc::new(move || {
                    Timestamp::from_second(seconds.load(std::sync::atomic::Ordering::SeqCst))
                        .unwrap()
                })
            },
            terminate: Arc::new(move |code| {
                let _ = tx.lock().unwrap().send(code);
            }),
            cancel: Cancel::default(),
            wind_down_limit: WIND_DOWN_LIMIT,
        };
        (process, terminated)
    }

    /// The watchdog tries the stream once at the deadline, so a line that
    /// is being written then keeps its failure off the stream. A backup that
    /// wound down writes it as the worker ends, before the exit with 75, and
    /// the daemon reads why the operation failed.
    #[test]
    fn a_deadline_failure_the_watchdog_could_not_write_goes_out_as_the_worker_ends() {
        let seconds = Arc::new(std::sync::atomic::AtomicI64::new(1_800_000_000));
        let (process, terminated) = process_on(&seconds);
        let cancel = process.cancel.clone();
        let (events, written, entered, release) = events::testing::stuck_then_captured();
        let watchdog = Watchdog::start(
            Deadline::after_index_read((process.clock)(), SignedDuration::from_secs(60)),
            Duration::from_millis(10),
            events.clone(),
            &process,
        );
        let progress = WorkerEvent::Log {
            level: Level::Info,
            message: "progress".to_owned(),
        };
        let saved = cancel.saving(|| {
            // A line stuck on a full pipe as the deadline passes.
            let writer = std::thread::spawn({
                let (events, progress) = (events.clone(), progress.clone());
                move || events.emit(&progress)
            });
            entered.recv().unwrap();
            seconds.fetch_add(60, std::sync::atomic::Ordering::SeqCst);
            let passed = std::time::Instant::now();
            while !cancel.is_requested() {
                assert!(
                    passed.elapsed() < Duration::from_secs(10),
                    "the deadline was not noticed"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            // The watchdog tries the stream right after it asks for the
            // cancel, and found it taken.
            std::thread::sleep(Duration::from_millis(100));
            drop(release);
            writer.join().unwrap().unwrap();
        });
        assert_eq!(saved, None);
        let exit = exit_of_cancelled(&events, &cancel);
        drop(watchdog);
        assert_eq!(exit.code(), exit_code::DEADLINE_PASSED);
        assert!(
            terminated.try_recv().is_err(),
            "the watchdog ended the worker under its backup"
        );
        assert_eq!(
            written.parsed(),
            [
                progress,
                WorkerEvent::Failed {
                    code: ErrorCode::DeadlinePassed,
                    args: Vec::new(),
                    detail: None,
                },
            ]
        );
    }

    /// The first cause stands: a backup winding down on a SIGTERM is not
    /// ended by its deadline passing meanwhile, and ends as a cancelled one,
    /// with 143 and nothing more on the stream.
    #[test]
    fn a_backup_winding_down_on_a_sigterm_ends_cancelled_when_its_deadline_passes_meanwhile() {
        let seconds = Arc::new(std::sync::atomic::AtomicI64::new(1_800_000_000));
        let (process, terminated) = process_on(&seconds);
        let cancel = process.cancel.clone();
        let (events, capture) = capture();
        let watchdog = Watchdog::start(
            Deadline::after_index_read((process.clock)(), SignedDuration::from_secs(60)),
            Duration::from_millis(10),
            events.clone(),
            &process,
        );
        let saved = cancel.saving(|| {
            // What the SIGTERM handler does.
            assert_eq!(cancel.request(), AfterCancel::WindDown);
            events.end(None, Duration::from_millis(250));
            seconds.fetch_add(60, std::sync::atomic::Ordering::SeqCst);
            // Many watchdog intervals.
            assert!(
                terminated.recv_timeout(Duration::from_millis(200)).is_err(),
                "the deadline ended a worker winding down on a SIGTERM"
            );
        });
        assert_eq!(saved, None);
        let exit = exit_of_cancelled(&events, &cancel);
        drop(watchdog);
        assert_eq!(exit.code(), exit_code::TERMINATED);
        assert!(
            terminated.try_recv().is_err(),
            "the watchdog ended the worker after its backup had wound down"
        );
        assert_eq!(capture.text(), "");
    }

    /// The data packs in a local repository: the files under `data`, but
    /// for the temporary ones the local backend writes before renaming.
    fn packs_in(repository: &Path) -> usize {
        let Ok(folders) = std::fs::read_dir(repository.join("data")) else {
            return 0;
        };
        folders
            .flatten()
            .filter_map(|folder| std::fs::read_dir(folder.path()).ok())
            .flatten()
            .flatten()
            .filter(|file| !file.file_name().to_string_lossy().contains("-tmp-"))
            .count()
    }

    /// A backup whose deadline passes while it archives winds down as a
    /// cancelled one does (decision 50): the worker reports the deadline,
    /// the packs it saved are in the index, no snapshot is written (I3),
    /// and the job, not the watchdog, ends the worker with the deadline's
    /// code once rustic has returned.
    #[test]
    fn a_backup_whose_deadline_passes_mid_archive_keeps_its_packs_indexed_and_writes_no_snapshot() {
        use rustic_core::{
            CheckOptions, ConfigOptions, Credentials, KeyOptions, Repository, RepositoryOptions,
        };
        use std::sync::atomic::{AtomicI64, Ordering};

        let dir = tempfile::tempdir().unwrap();
        let (source, repo) = (dir.path().join("source"), dir.path().join("repo"));
        std::fs::create_dir(&source).unwrap();
        // 64 MiB that neither compresses nor deduplicates, saved in packs of
        // 1 MiB: whole packs are in the repository long before the end.
        let mut state = 0x5EED_u64;
        let mut next = move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        for n in 0..16 {
            let bytes: Vec<u8> = (0..(4 << 20) / 8)
                .flat_map(|_| next().to_le_bytes())
                .collect();
            std::fs::write(source.join(format!("file{n}.bin")), bytes).unwrap();
        }
        let backends = rustic_backend::BackendOptions::default()
            .repository(repo.to_string_lossy().into_owned())
            .to_backends()
            .unwrap();
        let repository =
            || Repository::new(&RepositoryOptions::default().no_cache(true), &backends).unwrap();
        let small_packs: ConfigOptions = serde_json::from_value(serde_json::json!({
            "set-datapack-size": "1 MiB",
            "set-datapack-growfactor": 0,
        }))
        .unwrap();
        repository()
            .init(
                &Credentials::password("demo-password"),
                &KeyOptions::default(),
                &small_packs,
            )
            .unwrap();

        let seconds = Arc::new(AtomicI64::new(1_800_000_000));
        let (tx, terminated) = mpsc::channel();
        let tx = Mutex::new(tx);
        let process = Process {
            clock: {
                let seconds = Arc::clone(&seconds);
                Arc::new(move || Timestamp::from_second(seconds.load(Ordering::SeqCst)).unwrap())
            },
            terminate: Arc::new(move |code| {
                let _ = tx.lock().unwrap().send(code);
            }),
            cancel: Cancel::default(),
            wind_down_limit: WIND_DOWN_LIMIT,
        };
        let spec = JobSpec {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(9)),
            cache_dir: None,
            watchdog_interval_ms: 10,
            job: JobKind::Backup(crate::job::BackupJobSpec {
                repo: RepositorySpec {
                    password: SecretValue::new("demo-password"),
                    ..repository_at(&repo.to_string_lossy())
                },
                sources: vec![source.to_string_lossy().into_owned()],
                plan: domain::PlanId::parse("home").unwrap(),
                agent: domain::AgentId::from_uuid(domain::Uuid::from_u128(1)),
                extra_tags: Vec::new(),
                label: None,
                host: None,
                filter: Default::default(),
                deadline_budget: SignedDuration::from_secs(60),
            }),
        };
        let (events, capture) = capture();
        let input = serde_json::to_vec(&spec).unwrap();
        let running =
            std::thread::spawn(move || run_events(input.as_slice(), events, 1, process).unwrap());
        while packs_in(&repo) < 2 {
            assert!(
                !running.is_finished(),
                "the backup ended before its deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        seconds.fetch_add(120, Ordering::SeqCst);
        let exit = running.join().unwrap();

        assert_eq!(exit.code(), crate::job::exit_code::DEADLINE_PASSED);
        assert!(
            terminated.try_recv().is_err(),
            "the watchdog ended the worker under its backup"
        );
        let events = capture.parsed();
        assert_eq!(
            events.last(),
            Some(&WorkerEvent::Failed {
                code: ErrorCode::DeadlinePassed,
                args: Vec::new(),
                detail: None,
            }),
            "{events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, WorkerEvent::Result(_))),
            "{events:?}"
        );
        let snapshots = std::fs::read_dir(repo.join("snapshots")).unwrap();
        assert_eq!(
            snapshots.count(),
            0,
            "a snapshot was written past the deadline"
        );
        assert!(
            packs_in(&repo) >= 2,
            "the deadline came after packs were saved"
        );
        let findings = repository()
            .open(&Credentials::password("demo-password"))
            .unwrap()
            .check(CheckOptions::default().read_data(true))
            .unwrap();
        let findings: Vec<String> = findings
            .0
            .iter()
            .map(|(level, finding)| format!("{level:?}: {finding}"))
            .collect();
        assert!(findings.is_empty(), "{findings:#?}");
    }

    /// An Identify job for `repository`, which must fail: what the worker
    /// reported for it.
    fn failure_text(repository: RepositorySpec) -> String {
        let spec = JobSpec {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(9)),
            cache_dir: None,
            watchdog_interval_ms: 10,
            job: JobKind::Identify(repository),
        };
        let (events, capture) = capture();
        let input = serde_json::to_vec(&spec).unwrap();
        let exit = run_events(input.as_slice(), events, 1, Process::real()).unwrap();
        assert_eq!(exit.code(), 1);
        capture.text()
    }

    /// A repository in one place, with nothing hot or cold and nothing
    /// resolved.
    fn repository_at(repository: &str) -> RepositorySpec {
        RepositorySpec {
            repository: repository.to_owned(),
            repo_hot: None,
            password: SecretValue::new("hunter2"),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
            secrets: Vec::new(),
            id: None,
        }
    }

    #[test]
    fn a_job_that_needs_a_repository_hides_its_password_even_in_a_failure() {
        let text = failure_text(repository_at("/nonexistent/hunter2/repo"));
        assert!(!text.contains("hunter2"), "the password leaked: {text}");
    }

    /// An Identify job on a local path that doesn't exist: it fails with the
    /// path in its message, so what the path holds shows whether it is masked.
    fn failure_text_for(path_part: &str, options: &[(&str, &str)], secrets: &[&str]) -> String {
        let text = failure_text(RepositorySpec {
            options: options
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            secrets: secrets.iter().map(|s| SecretValue::new(*s)).collect(),
            ..repository_at(&format!("/nonexistent/{path_part}/repo"))
        });
        assert!(
            text.contains("nonexistent"),
            "the failure is not about the path: {text}"
        );
        text
    }

    #[test]
    fn a_resolved_secret_is_hidden_in_a_failure_whatever_it_sits_in() {
        let text = failure_text_for("CANARY-resolved", &[], &["CANARY-resolved"]);
        assert!(
            !text.contains("CANARY-resolved"),
            "a resolved secret leaked: {text}"
        );
    }

    #[test]
    fn the_value_of_a_credential_option_is_hidden_in_a_failure() {
        for (name, canary) in [
            ("sas_token", "CANARY-sas"),
            ("secret_access_key", "CANARY-sak"),
        ] {
            let text = failure_text_for(canary, &[(name, canary)], &[]);
            assert!(!text.contains(canary), "{name} leaked: {text}");
        }
    }

    /// The cold half is a path that exists nowhere, so the failure comes from
    /// the hot half: its URL has no valid port, rustic can't parse it, and its
    /// error quotes the URL.
    #[test]
    fn the_password_in_the_url_of_the_hot_repository_is_hidden_in_a_failure() {
        let text = failure_text(RepositorySpec {
            repo_hot: Some("rest:http://demo:CANARY-hot-password@127.0.0.1:99999/hot".to_owned()),
            ..repository_at("/nonexistent/repo")
        });
        assert!(
            text.contains("127.0.0.1:99999"),
            "the failure is not about the hot repository: {text}"
        );
        assert!(
            !text.contains("CANARY-hot-password"),
            "the hot repository's password leaked: {text}"
        );
    }

    #[test]
    fn the_password_in_the_url_of_an_option_is_hidden_in_a_failure() {
        let text = failure_text_for(
            "CANARY-userinfo",
            &[("endpoint", "http://demo:CANARY-userinfo@127.0.0.1:9")],
            &[],
        );
        assert!(
            !text.contains("CANARY-userinfo"),
            "endpoint credentials leaked: {text}"
        );
    }

    fn invalid_spec_failure(events: &[WorkerEvent]) {
        let [WorkerEvent::Failed { code, detail, .. }] = events else {
            panic!("expected exactly one failure event, got {events:?}");
        };
        assert_eq!(*code, ErrorCode::InvalidJobSpec);
        let detail = detail.as_deref().unwrap_or_default();
        assert!(
            detail.contains(&crate::job::MAX_MESSAGE_BYTES.to_string()),
            "the failure should name the cap: {detail}"
        );
    }

    #[test]
    fn a_job_spec_past_the_cap_is_rejected_even_when_it_parses() {
        let mut input = serde_json::to_vec(&fake_spec(None, 0)).unwrap();
        // Trailing whitespace is still valid JSON.
        input.resize(crate::job::MAX_MESSAGE_BYTES + 1, b' ');
        let (events, capture) = capture();
        let exit = run_events(input.as_slice(), events, 42, Process::real()).unwrap();
        assert_eq!(exit.code(), 1);
        invalid_spec_failure(&capture.parsed());
    }

    #[test]
    fn a_job_spec_of_exactly_the_cap_is_accepted() {
        let mut input = serde_json::to_vec(&fake_spec(None, 0)).unwrap();
        input.resize(crate::job::MAX_MESSAGE_BYTES, b' ');
        let (events, _capture) = capture();
        let exit = run_events(input.as_slice(), events, 42, Process::real()).unwrap();
        assert_eq!(exit, Exit::Reported { failed: false });
    }

    /// Serves up to 64 MiB of spaces and counts how much was asked for.
    struct Endless<'a> {
        served: &'a std::cell::Cell<usize>,
        left: usize,
    }

    impl Read for Endless<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.left);
            buf[..n].fill(b' ');
            self.left -= n;
            self.served.set(self.served.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn a_job_spec_that_never_ends_is_not_read_past_the_cap() {
        let served = std::cell::Cell::new(0);
        let input = Endless {
            served: &served,
            left: 64 << 20,
        };
        let (events, capture) = capture();
        let exit = run_events(input, events, 42, Process::real()).unwrap();
        assert_eq!(exit.code(), 1);
        invalid_spec_failure(&capture.parsed());
        assert!(
            served.get() <= crate::job::MAX_MESSAGE_BYTES + 1,
            "read {} bytes of a job spec capped at {}",
            served.get(),
            crate::job::MAX_MESSAGE_BYTES
        );
    }
}
