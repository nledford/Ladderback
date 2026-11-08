//! The worker side: read one [`JobSpec`] from stdin, run it, and report
//! [`WorkerEvent`]s on stdout, one JSON object per line.
//!
//! The worker is a process of its own (technical design §4.1) because
//! `rustic_core` can't be cancelled, keeps whole indexes in memory and runs
//! its own Tokio runtimes. [`main`] is what `steading worker` does: it routes
//! rustic's logging into the event stream, exits on SIGTERM, and runs the job.

mod convert;
mod events;
mod fake;
mod jobs;
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
use domain::secrets::Redactor;
use domain::{OperationId, SignedDuration, Timestamp};

use crate::cache_lock::CacheLock;
use crate::job::{
    JobKind, JobSpec, Level, MAX_MESSAGE_BYTES, MAX_MESSAGE_READ, OpResult, RepositorySpec,
    WorkerEvent, exit_code,
};
use convert::JobError;
use events::Events;
use watchdog::{Terminate, WallClock, Watchdog};

/// How the worker process should exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The job ran and its result or failure was reported.
    Reported { failed: bool },
    /// Stop at once with this code, reporting nothing more: a fake job
    /// imitating a crashed engine.
    Abrupt(u8),
}

impl Exit {
    pub fn code(self) -> i32 {
        match self {
            Self::Reported { failed: false } => 0,
            Self::Reported { failed: true } => 1,
            Self::Abrupt(code) => i32::from(code),
        }
    }
}

/// What `steading worker` does. Never returns normally: the process exits
/// with the job's [`Exit`] code, or 74 when the events can't be written.
pub fn main() -> ExitCode {
    // Red-first stub: a worker that never answers. A `todo!()` would end the
    // process, and a worker that is gone proves nothing about what it does
    // when its daemon dies.
    loop {
        std::thread::park();
    }
}

/// A line on stderr, for whoever started the worker. A worker that can't
/// write to stderr has nothing better to do.
fn say(text: &str) {
    let _ = writeln!(io::stderr(), "steading worker: {text}");
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
}

impl Process {
    fn real() -> Self {
        Self {
            clock: Arc::new(Timestamp::now),
            terminate: Arc::new(|code| {
                std::process::exit(code);
            }),
        }
    }
}

/// What a finished job leaves for the worker to report.
pub(crate) enum Done {
    Result(OpResult),
    /// The job wrote its result itself, with [`Context::report`], and has
    /// nothing more to say.
    Reported,
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
        todo!()
    }

    /// Reports that the job began its work, and arms the deadline watchdog
    /// when the job has a budget. Called right before the repository index
    /// is read, because the deadline counts from there. A deadline that has
    /// passed already ends the process before this returns.
    pub(crate) fn start(&self, budget: Option<SignedDuration>) -> io::Result<()> {
        todo!()
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
}

fn run_events(input: impl Read, events: Events, pid: u32, process: Process) -> io::Result<Exit> {
    todo!()
}

fn repository_of(job: &JobKind) -> Option<&RepositorySpec> {
    match job {
        JobKind::Fake(_) => None,
        JobKind::Identify(repository) => Some(repository),
        JobKind::Backup(job) => Some(&job.repo),
        JobKind::Prune(job) => Some(&job.repo),
        JobKind::Check(job) => Some(&job.repo),
    }
}

/// On SIGTERM the worker waits for the line it is writing, writes nothing
/// more and exits. Nothing in rustic needs a clean shutdown: its writes are
/// safe to interrupt anywhere (§4.1), which the kill-at-random-points test
/// proves.
#[cfg(unix)]
fn exit_on_sigterm(events: Events) -> io::Result<()> {
    use signal_hook::consts::SIGTERM;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGTERM])?;
    std::thread::Builder::new()
        .name("sigterm".to_owned())
        .spawn(move || {
            if signals.forever().next().is_some() {
                events.end(None, Duration::from_millis(250));
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

    #[test]
    fn every_event_is_one_line_of_json() {
        let spec = fake_spec(None, 2);
        let (events, capture) = capture();
        let input = serde_json::to_vec(&spec).unwrap();
        let exit = run_events(input.as_slice(), events, 42, Process::real()).unwrap();
        assert_eq!(exit, Exit::Reported { failed: false });
        let events = capture.parsed();
        let Some(WorkerEvent::Started { op_id, pid, .. }) = events.first() else {
            panic!("the stream doesn't start with Started: {events:?}");
        };
        assert_eq!((*op_id, *pid), (spec.op_id, 42));
        assert_eq!(
            events.last(),
            Some(&WorkerEvent::Result(OpResult::Fake { steps_completed: 2 }))
        );
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
        };
        let (events, capture) = capture();
        let input = serde_json::to_vec(&fake_spec(Some(SignedDuration::ZERO), 3)).unwrap();
        run_events(input.as_slice(), events, 1, process).unwrap();
        assert_eq!(
            terminated.try_recv(),
            Ok(crate::job::exit_code::DEADLINE_PASSED)
        );
        // The fake job ran on after the injected "exit"; none of it was reported.
        let events = capture.parsed();
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
        };
        let (events, capture) = capture();
        let spec = fake_spec(Some(SignedDuration::from_secs(60)), 0);
        let ctx = Context::new(events, &spec, 1, process);
        ctx.start(Some(SignedDuration::from_secs(60))).unwrap();
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

    #[test]
    fn a_job_that_needs_a_repository_hides_its_password_even_in_a_failure() {
        let spec = JobSpec {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(9)),
            cache_dir: None,
            watchdog_interval_ms: 10,
            job: JobKind::Identify(RepositorySpec {
                repository: "/nonexistent/hunter2/repo".to_owned(),
                password: SecretValue::new("hunter2"),
                options: Default::default(),
            }),
        };
        let (events, capture) = capture();
        let input = serde_json::to_vec(&spec).unwrap();
        let exit = run_events(input.as_slice(), events, 1, Process::real()).unwrap();
        assert_eq!(exit.code(), 1);
        let text = capture.text();
        assert!(!text.contains("hunter2"), "the password leaked: {text}");
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
