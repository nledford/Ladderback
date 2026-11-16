use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use api::{ErrorCode, MsgArg, PasswordDto, RepositorySettingsDto};
use app::fakes::{FakeSecrets, InMemoryConfigStore};
use app::ports::JobControl;
use domain::RepositoryId;

use super::*;

/// An engine that answers every job with the scripted outcome, and keeps
/// the jobs it was given. A location in `ids` is a repository of its
/// own: a success there names that ID instead of the scripted one.
pub(super) struct Scripted {
    pub(super) outcome: Mutex<Result<RepositoryId, FailureReason>>,
    /// What a `Job::Identify` finds instead of the outcome, once set.
    pub(super) found: Mutex<Option<Result<RepositoryId, FailureReason>>>,
    pub(super) ids: Mutex<BTreeMap<String, RepositoryId>>,
    pub(super) refuses: Mutex<Option<EngineError>>,
    /// How many of the next jobs never answer.
    pub(super) hangs: Mutex<usize>,
    pub(super) seen: Mutex<Vec<Job>>,
}

pub(super) struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let own = match &job {
            Job::Identify(access) | Job::Init(access) => self
                .ids
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&access.repository)
                .cloned(),
            _ => None,
        };
        let found = match &job {
            Job::Identify(_) => self
                .found
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            _ => None,
        };
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(job);
        if let Some(error) = self
            .refuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Err(error);
        }
        {
            let mut hangs = self.hangs.lock().unwrap_or_else(PoisonError::into_inner);
            if *hangs > 0 {
                *hangs -= 1;
                return Ok(RunningJob {
                    events: futures::stream::pending().boxed(),
                    control: Box::new(Stop),
                });
            }
        }
        let outcome = found.unwrap_or_else(|| {
            self.outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        });
        let event = match outcome {
            Ok(id) => EngineEvent::Succeeded(EngineResult::Repository {
                id: own.unwrap_or(id),
            }),
            Err(reason) => EngineEvent::Failed(reason),
        };
        Ok(RunningJob {
            events: futures::stream::iter([event]).boxed(),
            control: Box::new(Stop),
        })
    }
}

/// A configuration store that lets the other task run between a read
/// and the write that follows it, as a slow disk does.
pub(super) struct Yielding(pub(super) InMemoryConfigStore);

#[async_trait]
impl ConfigStore for Yielding {
    async fn load(&self) -> Result<domain::planning::AgentConfig, ConfigError> {
        let loaded = self.0.load().await;
        tokio::task::yield_now().await;
        loaded
    }

    async fn save(&self, config: &domain::planning::AgentConfig) -> Result<(), ConfigError> {
        self.0.save(config).await
    }

    async fn record_id(
        &self,
        alias: &domain::RepositoryAlias,
        read: &domain::planning::RepositoryConfig,
        id: &RepositoryId,
    ) -> Result<(), app::ports::RecordError> {
        self.0.record_id(alias, read, id).await
    }
}

/// A configuration store that can't be read.
pub(super) struct Broken(pub(super) ConfigError);

#[async_trait]
impl ConfigStore for Broken {
    async fn load(&self) -> Result<domain::planning::AgentConfig, ConfigError> {
        Err(self.0.clone())
    }

    async fn save(&self, _config: &domain::planning::AgentConfig) -> Result<(), ConfigError> {
        Err(self.0.clone())
    }

    async fn record_id(
        &self,
        _alias: &domain::RepositoryAlias,
        _read: &domain::planning::RepositoryConfig,
        _id: &RepositoryId,
    ) -> Result<(), app::ports::RecordError> {
        Err(app::ports::RecordError::Store(self.0.clone()))
    }
}

pub(super) fn repository_id() -> RepositoryId {
    RepositoryId::parse(&"a".repeat(64)).expect("a repository ID")
}

pub(super) fn engine() -> Arc<Scripted> {
    Arc::new(Scripted {
        outcome: Mutex::new(Ok(repository_id())),
        found: Mutex::new(None),
        ids: Mutex::new(BTreeMap::new()),
        refuses: Mutex::new(None),
        hangs: Mutex::new(0),
        seen: Mutex::new(Vec::new()),
    })
}

/// The service over `config` and an operation log of its own, empty,
/// keeping passwords in a keyring of its own, never the OS's.
pub(super) fn service(engine: &Arc<Scripted>, config: Arc<dyn ConfigStore>) -> RepositoryService {
    RepositoryService::new(
        clock(),
        engine.clone(),
        Arc::new(app::fakes::FakeNetworkLink::new()),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        config,
        Arc::new(app::fakes::InMemoryOperationStore::new()),
        Arc::default(),
    )
    .with_keyring(Arc::new(FakeSecrets::new()))
}

/// A clock that jumps to the deadline of whoever sleeps on it, so a job
/// that never answers is out of time the moment it is waited for.
pub(super) fn clock() -> Arc<dyn Clock> {
    Arc::new(app::fakes::FakeClock::auto_advancing(
        domain::Timestamp::from_second(1_800_000_000).expect("a timestamp"),
    ))
}

pub(super) fn jobs(engine: &Scripted) -> usize {
    engine
        .seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len()
}

pub(super) fn settings_dto() -> RepositorySettingsDto {
    RepositorySettingsDto {
        repository: "/srv/demo/repo".to_owned(),
        repo_hot: None,
        password: PasswordDto::Inline {
            value: "demo-password".to_owned(),
        },
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    }
}

pub(super) fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("an alias")
}

pub(super) fn connect(alias: &str, settings: RepositorySettingsDto) -> api::ConnectRepositoryDto {
    api::ConnectRepositoryDto {
        alias: alias.to_owned(),
        settings,
        maintain_here: false,
        keep_in_keyring: false,
    }
}

pub(super) fn init(alias: Option<&str>, settings: RepositorySettingsDto) -> api::InitRepositoryDto {
    api::InitRepositoryDto {
        alias: alias.map(str::to_owned),
        settings,
        keep_in_keyring: false,
    }
}

pub(super) async fn saved(config: &dyn ConfigStore) -> domain::planning::AgentConfig {
    config.load().await.expect("the configuration loads")
}

/// Leaves the configuration as an imported profile's are before anything
/// reached their repositories: no entry records its repository's ID, and
/// the plan `demo-home` backs up to `demo`.
pub(super) async fn imported(config: &dyn ConfigStore) {
    let recorded = saved(config).await;
    let repositories = recorded
        .repositories()
        .iter()
        .map(|(alias, entry)| {
            let mut entry = entry.clone();
            entry.id = None;
            (alias.clone(), entry)
        })
        .collect();
    let mut plans = recorded.plans().to_vec();
    plans.push(domain::planning::Plan::new(
        domain::PlanId::parse("demo-home").expect("a plan ID"),
        alias("demo"),
        vec![domain::planning::Source::parse("/home/demo").expect("a source")],
    ));
    let forgotten = domain::planning::AgentConfig::new(
        recorded.agent().clone(),
        recorded.hooks().clone(),
        repositories,
        plans,
    )
    .expect("a valid configuration");
    config
        .save(&forgotten)
        .await
        .expect("the configuration saves");
}

/// Records in `operations` a backup of the plan `demo-home` that ended
/// as `end` has it, succeeded (`Ok`) or failed.
pub(super) async fn backed_up(operations: &dyn OperationStore, end: Result<(), FailureReason>) {
    let at = domain::Timestamp::UNIX_EPOCH;
    let mut backup = domain::execution::Operation::queue(
        OperationId::from_uuid(Uuid::now_v7()),
        domain::execution::OperationKind::Backup,
        domain::execution::Trigger::Manual,
        domain::execution::Target {
            plan: Some(domain::PlanId::parse("demo-home").expect("a plan ID")),
            repository: None,
        },
        at,
    );
    backup.start(at).expect("it starts");
    match end {
        Ok(()) => backup.succeed(at),
        Err(reason) => backup.fail(reason, at),
    }
    .expect("it ends");
    operations
        .save(&backup, None)
        .await
        .expect("the backup is recorded");
}

/// [`imported`], then a backup of `demo-home` that found no repository
/// at `demo`'s location: what makes its row offer Create (decision 53).
/// The engine finds the location empty now too, as Create looks first.
pub(super) async fn imported_and_found_none(
    engine: &Scripted,
    service: &RepositoryService,
    config: &dyn ConfigStore,
) {
    imported(config).await;
    backed_up(&*service.operations, Err(FailureReason::RepositoryNotFound)).await;
    *engine.found.lock().unwrap_or_else(PoisonError::into_inner) =
        Some(Err(FailureReason::RepositoryNotFound));
}

pub(super) fn unresolved(
    field: Field,
    source: domain::secrets::SourceName,
    problem: domain::secrets::Problem,
) -> ResolveError {
    ResolveError {
        field,
        cause: Cause::Unresolved(domain::secrets::SecretError { source, problem }),
    }
}

/// A configuration that can't be used, and the code, the arguments and
/// the detail a refusal of it carries.
pub(in crate::composition) type Unusable = (
    ConfigError,
    ErrorCode,
    Vec<(&'static str, MsgArg)>,
    Option<&'static str>,
);

/// Each way a configuration can't be used, as every route refuses it.
pub(in crate::composition) fn unusable_configurations() -> Vec<Unusable> {
    let text = |value: &str| MsgArg::Text(value.to_owned());
    vec![
        (
            ConfigError::Unavailable {
                file: "/c/config.toml".into(),
                detail: "permission denied".into(),
            },
            ErrorCode::ConfigUnavailable,
            vec![("file", text("/c/config.toml"))],
            Some("permission denied"),
        ),
        (
            ConfigError::TooLarge {
                file: "/c/config.toml".into(),
                limit: 1024,
            },
            ErrorCode::ConfigTooLarge,
            vec![
                ("file", text("/c/config.toml")),
                ("limit", MsgArg::Bytes(1024)),
            ],
            None,
        ),
        (
            ConfigError::SchemaMissing {
                file: "/c/config.toml".into(),
            },
            ErrorCode::ConfigSchemaMissing,
            vec![("file", text("/c/config.toml"))],
            None,
        ),
        (
            ConfigError::SchemaNewer {
                file: "/c/config.toml".into(),
                found: 9,
                supported: 1,
            },
            ErrorCode::ConfigSchemaNewer,
            vec![
                ("file", text("/c/config.toml")),
                ("found", text("9")),
                ("supported", text("1")),
            ],
            None,
        ),
        (
            ConfigError::Invalid {
                file: "/c/config.toml".into(),
                key: Some("plans[0].id".into()),
                at: Some(app::ports::Position { line: 7, column: 3 }),
                problem: app::ports::Problem::Syntax,
                detail: "expected a table".into(),
            },
            ErrorCode::ConfigInvalid,
            vec![("file", text("/c/config.toml")), ("line", text("7"))],
            Some("plans[0].id: expected a table"),
        ),
        (
            ConfigError::Invalid {
                file: "/c/config.toml".into(),
                key: None,
                at: None,
                problem: app::ports::Problem::Syntax,
                detail: String::new(),
            },
            ErrorCode::ConfigInvalid,
            vec![("file", text("/c/config.toml"))],
            None,
        ),
    ]
}

/// A listing the engine refused, told by its code alone.
pub(super) fn engine_unavailable() -> SnapshotsError {
    SnapshotsError::Unavailable {
        code: ErrorCode::EngineUnavailable,
        args: Vec::new(),
        detail: None,
    }
}
