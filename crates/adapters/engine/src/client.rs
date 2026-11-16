//! The daemon side: spawn a worker per operation and translate its events.
//!
//! - [`spawn`] starts a worker: its job, its scrubbed environment and what
//!   masks the job's secrets in what it says (invariant I4).
//! - [`job_kind`] maps each of `app`'s jobs to the spec the worker takes.
//! - [`events`] reads the job's events from the worker's stdout, and
//!   [`translate`] turns each into the engine's.
//! - [`stderr`] logs what the worker writes to stderr, masked.
//! - [`stop`] supervises the worker until it exits, and stops it.
//! - [`cache_slots`] gives each worker of a repository a cache of its own.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use cache_slots::CacheSlots;

mod cache_slots;
mod events;
mod job_kind;
mod spawn;
mod stderr;
mod stop;
#[cfg(test)]
pub(crate) mod test_support;
mod translate;

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
pub(crate) const MAX_CACHE_SLOTS: u32 = 64;

/// Runs each operation in a new worker process: `program args…`, normally
/// `ladderback worker`. A worker leads a process group of its own, and every
/// kill is the group's, so the programs it started (an rclone, say) end with
/// it.
///
/// Stopping a job sends SIGTERM to the worker alone and, after a grace
/// period ([`STOP_GRACE`] unless [`WorkerEngine::with_stop_grace`] says
/// otherwise), SIGKILL to the group. [`Workers::stop`] stops every job so
/// when the daemon stops. While the stop waits, what the worker still
/// writes is read and dropped, so a backup winding down is never held by a
/// full event pipe. Dropping a [`RunningJob`] before its result kills the
/// worker at once; after the result, the worker gets a grace period to
/// finish its cleanup.
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
///
/// [`RunningJob`]: app::ports::RunningJob
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
    /// Whether the daemon is stopping; each running job holds a receiver
    /// until its worker has exited.
    stopping: Arc<watch::Sender<bool>>,
}

/// How long a stopped worker gets between SIGTERM and SIGKILL, unless
/// [`WorkerEngine::with_stop_grace`] says otherwise: past the worker's own
/// wind-down limit, so a backup stopped while it archives can save the
/// index of the packs it wrote (decisions 50 and 55, #321).
pub const STOP_GRACE: Duration = Duration::from_secs(35);

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
            stop_grace: STOP_GRACE,
            watchdog_interval: Duration::from_secs(5),
            start_timeout: START_TIMEOUT,
            exit_timeout: EXIT_TIMEOUT,
            slots: Arc::new(CacheSlots::default()),
            stopping: Arc::new(watch::channel(false).0),
        }
    }

    /// Gives each worker of a repository with a known ID its own rustic cache
    /// under `root/<repository id>/<slot>`, where `root` is Ladderback's own
    /// cache folder. Without a root, workers run without a cache.
    #[must_use]
    pub fn with_cache_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.cache_root = Some(root.into());
        self
    }

    /// How long [`JobControl::stop`] and [`Workers::stop`] wait after SIGTERM
    /// before SIGKILL: [`STOP_GRACE`] unless this says otherwise.
    ///
    /// [`JobControl::stop`]: app::ports::JobControl::stop
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
    ///
    /// [`Engine::run`]: app::ports::Engine::run
    /// [`EngineError::StartTimedOut`]: app::ports::EngineError::StartTimedOut
    /// [`EngineError::Unavailable`]: app::ports::EngineError::Unavailable
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
    ///
    /// [`environment`]: crate::environment
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

/// The workers of one [`WorkerEngine`], which the daemon stops when it
/// stops.
#[derive(Clone, Debug)]
pub struct Workers {
    stopping: Arc<watch::Sender<bool>>,
}

impl WorkerEngine {
    /// The handle that stops every worker this engine runs.
    pub fn workers(&self) -> Workers {
        Workers {
            stopping: Arc::clone(&self.stopping),
        }
    }
}

impl Workers {
    /// Stops every worker running as [`JobControl::stop`] stops one, SIGTERM
    /// and SIGKILL after the grace, and returns once each has exited. The
    /// stop is asked for at the call, not when the answer is first polled,
    /// so the daemon can let go of the jobs straight after and a worker let
    /// go of so still gets its SIGTERM and its grace. The events of a job
    /// stopped so never end: what runs it ends with the daemon and records
    /// no end, which the daemon's stop records for it (decision 92). A job
    /// asked for from then on is never started nor answered: nothing would
    /// be left to stop it.
    ///
    /// [`JobControl::stop`]: app::ports::JobControl::stop
    pub fn stop(&self) -> impl Future<Output = ()> + Send + 'static {
        self.stopping.send_replace(true);
        let stopping = Arc::clone(&self.stopping);
        async move { stopping.closed().await }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backup stopped while it archives winds down for up to the worker's
    /// own limit; the daemon's SIGKILL must not land first (decision 50,
    /// #321).
    #[test]
    fn the_stop_grace_outlasts_the_workers_own_wind_down() {
        let engine = WorkerEngine::new("sh", ["-c", "exit 0"]);
        assert!(
            engine.stop_grace > crate::worker::WIND_DOWN_LIMIT,
            "a grace of {:?} cuts a wind-down of up to {:?} short",
            engine.stop_grace,
            crate::worker::WIND_DOWN_LIMIT
        );
    }
}
