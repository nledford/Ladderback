use super::*;
use app::ports::{HookContext, HookOutcome, HookStatus, JobControl};

pub(super) fn demo_settings(password: SecretRef) -> RepositorySettings {
    RepositorySettings {
        repository: "s3:s3.example.net/demo".to_owned(),
        repo_hot: None,
        password,
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    }
}

/// Empty scratch folders the daemon is pointed at: no configuration has
/// ever been written to the first.
pub(super) fn own_folders(scratch: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let config = scratch.join("config");
    let data = scratch.join("data");
    std::fs::create_dir_all(&config).expect("the config folder");
    std::fs::create_dir_all(&data).expect("the data folder");
    (config, data)
}

/// A backup that was running when the daemon that ran it stopped.
pub(super) fn running_backup(id: OperationId) -> domain::execution::Operation {
    let mut backup = domain::execution::Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Manual,
        domain::execution::Target {
            plan: None,
            repository: None,
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    );
    backup
        .start(domain::Timestamp::from_second(1_800_000_001).expect("a time"))
        .expect("the backup starts");
    backup
}

/// A hook runner that reports a run has entered its before-hooks and
/// parks there until the test opens the gate.
pub(super) struct GateHooks {
    entered: std::sync::Arc<tokio::sync::watch::Sender<u32>>,
    open: std::sync::Arc<tokio::sync::Mutex<tokio::sync::watch::Receiver<bool>>>,
}

impl GateHooks {
    fn new() -> (
        Self,
        tokio::sync::watch::Receiver<u32>,
        tokio::sync::watch::Sender<bool>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::watch::channel(0);
        let (open_tx, open_rx) = tokio::sync::watch::channel(false);
        let open_gate = open_tx.clone();
        let hooks = Self {
            entered: std::sync::Arc::new(entered_tx),
            open: std::sync::Arc::new(tokio::sync::Mutex::new(open_rx)),
        };
        (hooks, entered_rx, open_gate)
    }
}

#[async_trait]
impl HookRunner for GateHooks {
    async fn run(
        &self,
        _entry: &domain::planning::HookEntry,
        _context: &HookContext,
        _redactor: &domain::secrets::Redactor,
    ) -> HookOutcome {
        self.entered.send_modify(|seen| *seen += 1);
        let mut open = self.open.lock().await;
        while !*open.borrow_and_update() {
            open.changed().await.expect("the gate stays open");
        }
        HookOutcome {
            status: HookStatus::Succeeded,
            stdout: Default::default(),
            stderr: Default::default(),
        }
    }

    fn environment_secrets(&self) -> Vec<domain::secrets::SecretValue> {
        Vec::new()
    }
}

/// An engine whose jobs report nothing: the run parks, as a real backup
/// does while it works, and stopping it is a no-op.
pub(super) struct ParkedEngine;

#[async_trait]
impl Engine for ParkedEngine {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Ok(RunningJob {
            events: futures::stream::pending().boxed(),
            control: Box::new(Parked),
        })
    }
}

pub(super) struct Parked;

#[async_trait]
impl JobControl for Parked {
    async fn stop(&mut self) {}
}

/// A runner over scratch folders with one plan, "home", whose agent-level
/// before-hook is the gate; its engine parks. Also what the gate says and
/// opens, and the scratch folder, which must outlive the runner.
pub(super) async fn runner_at_a_gate() -> (
    PlanRunner,
    tokio::sync::watch::Receiver<u32>,
    tokio::sync::watch::Sender<bool>,
    tempfile::TempDir,
) {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config_dir, data_dir) = own_folders(scratch.path());
    let source = scratch.path().join("source");
    std::fs::create_dir_all(&source).expect("the source folder");
    let alias = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let settings = domain::planning::RepositorySettings {
        repository: scratch.path().join("repo").display().to_string(),
        repo_hot: None,
        password: domain::secrets::SecretRef::Inline(domain::secrets::SecretValue::new(
            "demo-password",
        )),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let hooks = domain::planning::Hooks {
        run_before: vec![domain::planning::HookEntry::parse("true").expect("a hook")],
        ..Default::default()
    };
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        hooks,
        std::collections::BTreeMap::from([(
            alias.clone(),
            domain::planning::RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings,
                // This agent maintains it, so a preview and a run reach
                // the engine.
                maintenance: domain::planning::Maintenance::Local,
                group_by: Default::default(),
                hooks: Default::default(),
            },
        )]),
        vec![domain::planning::Plan::new(
            domain::PlanId::parse("home").expect("a plan ID"),
            alias,
            vec![
                domain::planning::Source::parse(source.display().to_string().as_str())
                    .expect("a source"),
            ],
        )],
    )
    .expect("a valid configuration");
    let (hooks, entered, open_gate) = GateHooks::new();
    let config_store = Arc::new(FileConfigStore::new(
        config_dir.join("config.toml"),
        Arc::new(SystemClock),
    ));
    app::ports::ConfigStore::save(&*config_store, &config)
        .await
        .expect("the config saves");
    let runner = PlanRunner {
        config: config_store,
        resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
        agent_id: agent_id::load_or_create(&data_dir).expect("an agent ID"),
        engine: Arc::new(ParkedEngine),
        locations: None,
        retention: Arc::new(engine::RusticRetention),
        store: Arc::new(
            SqliteStore::open(&data_dir.join("ladderback.db"))
                .await
                .expect("the store opens"),
        ),
        registry: Arc::new(CancelRegistry::new()),
        hooks: Arc::new(hooks),
        events: Arc::new(BroadcastEventBus::new()),
        queue: Arc::new(RunQueue::new()),
        maintenance_queue: Arc::new(app::queue::MaintenanceQueue::new()),
        ends: app::retry::run_ends().0,
        runs: crate::composition::runs::Runs::default(),
        readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
        full_disk_access: None,
        machine: Arc::default(),
        leases: None,
        running: Arc::default(),
        left_out: KeyFilesLeftOut::default(),
        writes: Arc::default(),
    };
    (runner, entered, open_gate, scratch)
}

/// An engine for a repository that holds no snapshots: it names the
/// repository when asked to identify it, and lists nothing for any other
/// job.
pub(super) struct Empty;

#[async_trait]
impl Engine for Empty {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let result = match job {
            Job::Identify(_) => EngineResult::Repository {
                id: domain::RepositoryId::parse(&"a".repeat(64)).expect("a repository ID"),
            },
            _ => EngineResult::Snapshots(Vec::new()),
        };
        Ok(RunningJob {
            events: futures::stream::iter([EngineEvent::Succeeded(result)]).boxed(),
            control: Box::new(Parked),
        })
    }
}

/// The status the operation ends with, once it has.
pub(super) async fn ended(runner: &PlanRunner, operation: OperationId) -> OperationStatus {
    let ended = async {
        loop {
            let detail = GetOperation::new(Arc::clone(&runner.store))
                .get(operation)
                .await
                .expect("the log reads");
            if let Some(detail) = detail
                && detail.record.status.is_finished()
            {
                return detail.record.status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), ended)
        .await
        .expect("the canceled operation ends")
}

/// An engine whose jobs play `events`, whatever the jobs are.
pub(super) struct Answers(pub(super) Vec<EngineEvent>);

#[async_trait]
impl Engine for Answers {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Ok(RunningJob {
            events: futures::stream::iter(self.0.clone()).boxed(),
            control: Box::new(Parked),
        })
    }
}

pub(super) fn started() -> EngineEvent {
    EngineEvent::Started {
        index_read_at: domain::Timestamp::from_second(1_800_000_000).expect("a time"),
        repository: None,
    }
}

/// The demo repository's ID: what [`Completes`] reports for a read of it,
/// and what the tests that look at its slot in the log name.
pub(super) fn demo_repository() -> domain::RepositoryId {
    domain::RepositoryId::parse(&"ab".repeat(32)).expect("a repository ID")
}

/// An engine whose jobs start and succeed at once, each with what its
/// kind reports: the ID of the repository a read opens, the outcome of
/// a check, the stats of an empty repository.
pub(super) struct Completes;

#[async_trait]
impl Engine for Completes {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let repository = demo_repository();
        let result = match &job {
            Job::Identify(_) => EngineResult::Repository { id: repository },
            Job::Check(_) => EngineResult::Check(app::ports::CheckSummary {
                repository,
                errors: 0,
                warnings: 0,
            }),
            Job::Stats(_) => EngineResult::Stats(domain::maintenance::RepositoryStats::default()),
            _ => EngineResult::Fake { steps_completed: 1 },
        };
        Answers(vec![started(), EngineEvent::Succeeded(result)])
            .run(op, job)
            .await
    }
}

/// What a page that listens to the daemon hears.
pub(super) type Heard = futures::stream::BoxStream<'static, UiEvent>;

/// Runs the daemon's engine and bus over `inner`'s jobs and `log` as the
/// runner's own, and returns what a page that listens to the bus hears.
pub(super) async fn listen(
    runner: &mut PlanRunner,
    inner: impl Engine + 'static,
    log: Arc<dyn OperationStore>,
) -> Heard {
    let bus = Arc::new(BroadcastEventBus::new());
    let heard = bus.subscribe().await;
    runner.engine = Arc::new(EventEngine {
        inner: Arc::new(inner),
        bus: bus.clone(),
        clock: Arc::new(SystemClock),
    });
    runner.events = bus;
    runner.store = log;
    heard
}

pub(super) fn is_end(news: &UiEvent) -> bool {
    matches!(
        news,
        UiEvent::OperationSucceeded { .. } | UiEvent::OperationFailed { .. }
    )
}

/// How operations have ended, as far as the page has heard, without
/// waiting for any more.
pub(super) fn ends_heard(heard: &mut Heard) -> Vec<UiEvent> {
    let mut ends = Vec::new();
    while let Some(Some(news)) = futures::FutureExt::now_or_never(heard.next()) {
        if is_end(&news) {
            ends.push(news);
        }
    }
    ends
}

/// Waits for the page to hear how an operation ended.
pub(super) async fn next_end(heard: &mut Heard) -> UiEvent {
    let end = async {
        while let Some(news) = heard.next().await {
            if is_end(&news) {
                return news;
            }
        }
        panic!("the page's news ended");
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), end)
        .await
        .expect("the page hears how the operation ended")
}

/// The history as a page reads it, once it has heard the end.
pub(super) async fn history_of(runner: &PlanRunner, id: &str) -> app::ports::OperationDetail {
    GetOperation::new(Arc::clone(&runner.store))
        .get(operation_of(id).expect("an operation ID"))
        .await
        .expect("the log reads")
        .expect("the operation is in the log")
}

/// An operation log that refuses every save, in the words a database
/// uses for what it refuses, and counts them. Starts and reads go to the
/// log behind it.
pub(super) struct RefusingSaves {
    pub(super) inner: Arc<dyn OperationStore>,
    pub(super) refused: Arc<std::sync::atomic::AtomicUsize>,
}

/// What a refused save says for itself: none of it may reach the daemon's
/// log.
pub(super) const REFUSAL: &str = "canary: the disk of /srv/secret-folder is full";

#[async_trait]
impl OperationStore for RefusingSaves {
    async fn recover(
        &self,
        reason: &FailureReason,
    ) -> Result<app::ports::Recovered, app::ports::StoreError> {
        self.inner.recover(reason).await
    }

    async fn save(
        &self,
        _operation: &domain::execution::Operation,
        _result: Option<&app::ports::EngineResult>,
    ) -> Result<(), app::ports::StoreError> {
        self.refused
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(app::ports::StoreError::Unavailable {
            detail: REFUSAL.to_owned(),
        })
    }

    async fn try_start(
        &self,
        operation: &domain::execution::Operation,
    ) -> Result<app::ports::Start, app::ports::StoreError> {
        self.inner.try_start(operation).await
    }

    async fn get(
        &self,
        id: OperationId,
    ) -> Result<Option<app::ports::OperationDetail>, app::ports::StoreError> {
        self.inner.get(id).await
    }

    async fn list(
        &self,
        query: &app::ports::HistoryQuery,
    ) -> Result<app::ports::HistoryPage, app::ports::StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, app::ports::StoreError> {
        self.inner.ever_succeeded(plan).await
    }

    async fn running_maintenance(
        &self,
        repository: Option<&domain::RepositoryId>,
    ) -> Result<Option<OperationId>, app::ports::StoreError> {
        self.inner.running_maintenance(repository).await
    }
}

/// An operation log whose writes wait until the test opens it.
pub(super) struct GatedSaves {
    pub(super) inner: Arc<dyn OperationStore>,
    pub(super) open: tokio::sync::watch::Receiver<bool>,
}

#[async_trait]
impl OperationStore for GatedSaves {
    async fn recover(
        &self,
        reason: &FailureReason,
    ) -> Result<app::ports::Recovered, app::ports::StoreError> {
        self.inner.recover(reason).await
    }

    async fn save(
        &self,
        operation: &domain::execution::Operation,
        result: Option<&app::ports::EngineResult>,
    ) -> Result<(), app::ports::StoreError> {
        let mut open = self.open.clone();
        while !*open.borrow_and_update() {
            open.changed().await.expect("the gate stays open");
        }
        self.inner.save(operation, result).await
    }

    async fn try_start(
        &self,
        operation: &domain::execution::Operation,
    ) -> Result<app::ports::Start, app::ports::StoreError> {
        self.inner.try_start(operation).await
    }

    async fn get(
        &self,
        id: OperationId,
    ) -> Result<Option<app::ports::OperationDetail>, app::ports::StoreError> {
        self.inner.get(id).await
    }

    async fn list(
        &self,
        query: &app::ports::HistoryQuery,
    ) -> Result<app::ports::HistoryPage, app::ports::StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, app::ports::StoreError> {
        self.inner.ever_succeeded(plan).await
    }

    async fn running_maintenance(
        &self,
        repository: Option<&domain::RepositoryId>,
    ) -> Result<Option<OperationId>, app::ports::StoreError> {
        self.inner.running_maintenance(repository).await
    }
}

/// An engine that answers the identify a maintenance run begins with and
/// parks the job itself, as a real check does while it works.
pub(super) struct IdentifiesThenParks;

#[async_trait]
impl Engine for IdentifiesThenParks {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        match job {
            Job::Identify(_) => Completes.run(op, job).await,
            _ => ParkedEngine.run(op, job).await,
        }
    }
}

pub(super) fn english() -> l10n::Localizer {
    l10n::Localizer::new(&[])
}
