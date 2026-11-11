//! Runs the `@e2e` scenarios in `features/` against the real `ladderback`
//! binary: operations run in real `ladderback worker` processes (§13.2).

use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app::execution::{Finished, RunOperation};
use app::ports::{
    Engine, EngineError, EngineEvent, FakeEnd, FakeJob, Job, OperationStore, RunningJob,
};
use async_trait::async_trait;
use cucumber::World;
use domain::execution::{Operation, Trigger};
use domain::{AgentId, OperationId, Uuid};
use engine::WorkerEngine;
use futures::StreamExt;
use futures::channel::oneshot;
use platform::SystemClock;
use store::SqliteStore;

#[path = "steps/backup.rs"]
mod backup_steps;
#[path = "../crates/app/tests/steps/operation.rs"]
mod operation_steps;

#[path = "steps/config.rs"]
mod config_steps;

#[path = "steps/hooks.rs"]
mod hooks_steps;

#[path = "steps/request_checks.rs"]
mod request_checks_steps;

#[path = "steps/events.rs"]
mod events_steps;

#[path = "steps/operations.rs"]
mod operations_steps;
#[path = "steps/secrets.rs"]
mod secrets_steps;

#[path = "steps/adopt.rs"]
mod adopt_steps;
#[path = "steps/cli_output.rs"]
mod cli_output_steps;
#[path = "steps/forget.rs"]
mod forget_steps;
#[path = "steps/interop.rs"]
mod interop_steps;
#[path = "steps/maintenance.rs"]
mod maintenance_steps;
#[path = "steps/plan_settings.rs"]
mod plan_settings_steps;
#[path = "steps/repositories.rs"]
mod repositories_steps;
#[path = "steps/restart.rs"]
mod restart_steps;
#[path = "steps/snapshots.rs"]
mod snapshots_steps;

const LADDERBACK: &str = env!("CARGO_BIN_EXE_ladderback");

#[derive(World)]
#[world(init = Self::new)]
struct Agent {
    engine: Arc<dyn Engine>,
    job: FakeJob,
    finished: Option<Finished>,
    /// The operation log: a SQLite database in the scratch dir, opened by the
    /// first run or history step.
    store: Option<Arc<SqliteStore>>,
    /// How many operations the scenario has run; each gets its own ID.
    runs: u128,
    scratch: tempfile::TempDir,
    agent_id: AgentId,
    backup: backup_steps::BackupSetup,
    web: request_checks_steps::Web,
    hooks: hooks_steps::HooksE2e,
    events: events_steps::Events,
    secrets: secrets_steps::SecretsSetup,
    ops: operations_steps::Ops,
    config: config_steps::ConfigSetup,
    repositories: repositories_steps::RepositoriesSetup,
    snapshots: snapshots_steps::SnapshotsSetup,
    forget: forget_steps::ForgetE2e,
    interop: interop_steps::InteropE2e,
    adopt: adopt_steps::AdoptE2e,
    maintenance: maintenance_steps::MaintenanceE2e,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("job", &self.job)
            .field("finished", &self.finished)
            .field("backup", &self.backup)
            .finish_non_exhaustive()
    }
}

impl Agent {
    fn new() -> Self {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let agent_id =
            platform::agent_id::load_or_create(&scratch.path().join("data")).expect("an agent ID");
        Self {
            engine: Arc::new(WorkerEngine::new(LADDERBACK, ["worker"])),
            job: FakeJob {
                steps: 1,
                step_delay: Duration::from_millis(10),
                warnings: Vec::new(),
                end: FakeEnd::Succeed,
                deadline_budget: None,
            },
            finished: None,
            store: None,
            runs: 0,
            scratch,
            agent_id,
            backup: backup_steps::BackupSetup::default(),
            hooks: hooks_steps::HooksE2e::new(),
            web: request_checks_steps::Web::default(),
            events: events_steps::Events::default(),
            secrets: secrets_steps::SecretsSetup::default(),
            ops: operations_steps::Ops::default(),
            config: config_steps::ConfigSetup::default(),
            repositories: repositories_steps::RepositoriesSetup::default(),
            snapshots: snapshots_steps::SnapshotsSetup::default(),
            forget: forget_steps::ForgetE2e::default(),
            interop: interop_steps::InteropE2e::default(),
            adopt: adopt_steps::AdoptE2e::default(),
            maintenance: maintenance_steps::MaintenanceE2e::default(),
        }
    }

    fn scratch(&self) -> &std::path::Path {
        self.scratch.path()
    }

    fn make_engine_unavailable(&mut self) {
        let missing = std::env::temp_dir().join("ladderback-e2e-no-such-binary");
        self.engine = Arc::new(WorkerEngine::new(missing, ["worker"]));
    }

    async fn run_operation(&mut self) {
        let job = Job::Fake(self.job.clone());
        self.run(job, futures::future::pending()).await;
    }

    async fn run_backup(&mut self) {
        let job = self.backup_job();
        self.run(job, futures::future::pending()).await;
    }

    /// Runs the backup and cancels it `after` its worker reported `Started`.
    async fn run_backup_cancelled_after(&mut self, after: Duration) {
        let (started, on_start) = oneshot::channel();
        let engine = Arc::new(Watched {
            inner: self.engine.clone(),
            started: Mutex::new(Some(started)),
        });
        let cancel = async move {
            if on_start.await.is_ok() {
                tokio::time::sleep(after).await;
            } else {
                futures::future::pending::<()>().await;
            }
        };
        let job = self.backup_job();
        self.run_on(engine, job, cancel).await;
    }

    async fn run(&mut self, job: Job, cancel: impl Future<Output = ()> + Send) {
        self.run_on(self.engine.clone(), job, cancel).await;
    }

    async fn run_on(
        &mut self,
        engine: Arc<dyn Engine>,
        job: Job,
        cancel: impl Future<Output = ()> + Send,
    ) {
        let store = self.store().await;
        self.runs += 1;
        let id = OperationId::from_uuid(Uuid::from_u128(self.runs));
        let mut use_case = RunOperation::new(engine, Arc::new(SystemClock), store);
        if !self.hooks.setup.is_empty() {
            use_case = use_case.with_hooks(self.hooks.runner.clone(), self.hooks.setup.clone());
        }
        self.finished = Some(use_case.run(id, Trigger::Manual, job, cancel).await);
    }

    fn finished(&self) -> &Finished {
        self.finished.as_ref().expect("the operation has run")
    }

    fn operation(&self) -> &Operation {
        &self.finished().operation
    }

    /// The operation log the runs write to.
    async fn store(&mut self) -> Arc<dyn OperationStore> {
        if self.store.is_none() {
            let path = self.scratch().join("data").join("ladderback.db");
            let opened = SqliteStore::open(&path)
                .await
                .expect("the operation log opens");
            self.store = Some(Arc::new(opened));
        }
        self.store.clone().expect("opened above")
    }
}

/// Passes jobs to `inner`, signalling once the first job reports `Started`.
struct Watched {
    inner: Arc<dyn Engine>,
    started: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl Engine for Watched {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let running = self.inner.run(op, job).await?;
        let mut started = self.started.lock().unwrap().take();
        let events = running
            .events
            .inspect(move |event| {
                if matches!(event, EngineEvent::Started { .. })
                    && let Some(started) = started.take()
                {
                    let _ = started.send(());
                }
            })
            .boxed();
        Ok(RunningJob {
            events,
            control: running.control,
        })
    }
}

#[tokio::main]
async fn main() {
    // See crates/app/tests/acceptance.rs: nothing to list for libtest.
    if std::env::args().any(|a| a == "--list") {
        return;
    }
    let features = concat!(env!("CARGO_MANIFEST_DIR"), "/features");
    let permissions_work = file_permissions_work();
    let ipv6_works = ipv6_works();
    // `--name` and `--tags` narrow this runner's scenarios rather than
    // replace its filter (see `operation_steps::picked_by_cli_tags`).
    let mut cli = cucumber::cli::Opts::<_, _, _>::parsed();
    let name = cli.re_filter.take();
    let tags = cli.tags_filter.take();
    let worst_stall = Cell::new(Duration::ZERO);
    let run = Agent::cucumber()
        .with_cli(cli)
        .fail_on_skipped()
        // Each backup runs a worker process that uses every core, so more
        // scenarios at once than a few only make the runner's own thread
        // wait for the processor: 64 at once (cucumber's default) held it
        // for 26 s here, 4 for under a second, and the run is faster.
        .max_concurrent_scenarios(4)
        .filter_run_and_exit(features, move |feature, rule, scenario| {
            if !operation_steps::tagged(feature, rule, scenario, "e2e")
                || !name.as_ref().is_none_or(|re| re.is_match(&scenario.name))
                || !operation_steps::picked_by_cli_tags(tags.as_ref(), feature, rule, scenario)
            {
                return false;
            }
            if !permissions_work
                && operation_steps::tagged(feature, rule, scenario, "needs-file-permissions")
            {
                testkit::record_skip(
                    module_path!(),
                    &format!(
                        "{}: a file with no permissions is still readable here (root?)",
                        scenario.name
                    ),
                );
                return false;
            }
            if !ipv6_works && operation_steps::tagged(feature, rule, scenario, "needs-ipv6") {
                testkit::record_skip(
                    module_path!(),
                    &format!("{}: this machine can't listen on IPv6", scenario.name),
                );
                return false;
            }
            true
        });
    // The runner polls every scenario from this one task: watching it from
    // the same task measures how long a step left it unpolled.
    let _report = ReportStall(&worst_stall);
    let watch = watch_for_stalls(&worst_stall);
    futures::pin_mut!(run, watch);
    futures::future::select(run, watch).await;
    assert!(
        worst_stall.get() <= MAX_STALL,
        "a step kept the runner's thread for {:?}, longer than the {MAX_STALL:?} allowed: \
         every other scenario waited that long, and the operation log gives up on a \
         connection after 10 s. A step that blocks for long (rustic_core, waiting for a daemon \
         to start, thousands of file writes) runs through `blocking(…)`; too many \
         scenarios at once starve it too.",
        worst_stall.get()
    );
}

/// Runs `work` on a blocking thread and waits for it without holding the
/// runner's thread. The runner polls every scenario from one task, so a step
/// that blocks, such as `rustic_core` deriving a repository's key (about a
/// second in a debug build) or a blocking socket, stops every other scenario
/// until it returns. Steps that block call this instead. A panic in `work` is
/// the step's panic.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    match tokio::task::spawn_blocking(work).await {
        Ok(done) => done,
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(error) => panic!("the blocking step's thread was cancelled: {error}"),
        },
    }
}

/// How long the runner's thread may go unpolled before the run fails: most of
/// the 10 s the operation log waits for a connection, and several times what
/// a run that blocks nothing and shares the machine with its workers sees (up
/// to 1.2 s on ten cores, with 4 scenarios at once).
const MAX_STALL: Duration = Duration::from_secs(8);

/// Says how long the runner's thread was unavailable at worst when the run
/// ends, also when a failed step ends it: the figure belongs to a failure
/// that looks like #92 as much as to a pass.
struct ReportStall<'a>(&'a Cell<Duration>);

impl Drop for ReportStall<'_> {
    fn drop(&mut self) {
        eprintln!(
            "e2e runner: its thread was unavailable for up to {:?} at once",
            self.0.get()
        );
    }
}

/// Records in `worst` the longest a timer was late being polled, forever.
/// Joined with the runner in one task, a late poll is a step that blocked the
/// thread every scenario shares.
async fn watch_for_stalls(worst: &Cell<Duration>) {
    const TICK: Duration = Duration::from_millis(50);
    loop {
        let before = std::time::Instant::now();
        tokio::time::sleep(TICK).await;
        worst.set(worst.get().max(before.elapsed().saturating_sub(TICK)));
    }
}

/// Whether this machine can listen on IPv6: containers often can't, and a
/// refusal to listen there would then prove nothing about the startup rule.
fn ipv6_works() -> bool {
    std::net::TcpListener::bind("[::]:0").is_ok()
}

/// Whether a file with mode 000 is unreadable here. Root and processes with
/// `CAP_DAC_OVERRIDE` read it anyway, and off Unix there are no such modes.
fn file_permissions_work() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let probe = tempfile::NamedTempFile::new().expect("a probe file");
        std::fs::set_permissions(probe.path(), std::fs::Permissions::from_mode(0o000))
            .expect("the probe's permissions change");
        std::fs::read(probe.path()).is_err()
    }
    #[cfg(not(unix))]
    {
        false
    }
}
