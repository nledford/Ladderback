//! The daemon's wiring (technical design §3.2, §4.2): the one place beside
//! `main` that knows every adapter. `serve` opens the store and the config,
//! builds the engine, secrets and the event bus, and hands the result to the
//! HTTP router. Adapters stay constructor-injected; nothing else sees them.
//!
//! A startup that cannot happen is a typed exit: [`Refused`] carries the
//! catalog message and its arguments, and `serve` prints it and fails — never
//! a silent empty configuration or a panic.

use std::collections::HashMap;
use std::sync::Arc;

use api::{ErrorCode, MsgArg, UiEvent, WarningCode};
use app::event_bus::BroadcastEventBus;
use app::execution::RunOperation;
use app::history::{GetOperation, ListOperations};
use app::interop::{ExportError, ImportError};
use app::maintenance::{Adoption, AdoptionSource, MaintenanceKind, RunMaintenance};
use app::ports::Retention;
use app::ports::SecretResolver as _;
use app::ports::{
    BackupJob, Clock, ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult,
    EventBus, ExportRefusal, HookRunner, HookSetup, Job, OperationRecord, OperationStore,
    ProfileError, ProfileNotice, ProgressEvent, RunningJob, SnapshotSummary,
};
use app::recovery::RecoverOperations;
use app::repositories::{ConnectRepository, InitRepository, RepositoryError, TestRepository};
use app::secrets::{Cause, Field, ResolveError, ResolveRepository};
use app::snapshots::{ListSnapshots, SnapshotError};
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::RepositoryAlias;
use domain::execution::FailureReason;
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{OperationKind, OperationStatus, Trigger, WarningKind};
use domain::planning::RepositorySettings;
use domain::secrets::{
    FileRead, KeyringEntry, Part, Redactor, RepositorySecrets, SecretRef, SecretValue, SourceName,
    Template, TemplateError,
};
use domain::{AgentId, OperationId, PlanId};
use engine::WorkerEngine;
use futures::stream::StreamExt;
use hooks::ProcessHookRunner;
use http_adapter::{
    AppState, Catalog, CatalogError, Operations, Repositories, RepositoriesError, SecretRefusal,
    SnapshotsError, StartError,
};
use l10n::{MessageId, msg};
use platform::SystemClock;
use platform::data_dir_lock::{DataDirLock, LOCK_FILE, LockError};
use platform::{agent_id, dirs};
use secrets::SystemSecrets;
use store::SqliteStore;
use uuid::Uuid;

/// Why the daemon cannot serve, as a message for the person who started it.
pub(crate) struct Refused {
    pub(crate) message: MessageId,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
}

/// A refusal with no arguments.
fn plain(message: MessageId) -> Refused {
    Refused {
        message,
        args: Vec::new(),
    }
}

/// A text argument, the kind identifiers, paths and versions are.
fn text(value: impl std::fmt::Display) -> MsgArg {
    MsgArg::Text(value.to_string())
}

/// Says in the log, once at load, each secret the configuration holds as
/// text (§10.2): the repository and the setting, in the catalog's words,
/// never the secret (I4).
fn log_inline_secrets(config: &domain::planning::AgentConfig) {
    let localizer = l10n::Localizer::new(&l10n::PosixEnv::from_process().preferences());
    for (alias, repository) in config.repositories() {
        for secret in repository.settings.inline_secrets() {
            let message = localizer.warning(
                WarningCode::ConfigInlineSecret,
                &[("field", MsgArg::Text(secret.field()))],
                None,
            );
            log::warn!("repository {}: {message}", alias.as_str());
        }
    }
}

/// What `serve` runs: the router's state, and the hold on the data folder that
/// keeps every other process off it. The caller keeps the lock until the
/// daemon is gone.
pub(crate) struct Daemon {
    pub(crate) state: AppState,
    pub(crate) lock: DataDirLock,
}

/// What the daemon's router runs against (§4.2), or why it cannot.
pub(crate) async fn daemon() -> Result<Daemon, Refused> {
    daemon_at(dirs::config_dir(), dirs::data_dir()).await
}

/// [`daemon`] with the folders supplied, for tests. The data folder is locked
/// before anything else is read or opened (§9), and what an earlier run left
/// unfinished is ended before the state is handed over, so nothing serves or
/// listens until both are done.
async fn daemon_at(
    config_dir: Option<std::path::PathBuf>,
    data_dir: Option<std::path::PathBuf>,
) -> Result<Daemon, Refused> {
    let config_dir = config_dir.ok_or_else(|| plain(msg::CLI_NO_CONFIG_DIR))?;
    let data_dir = data_dir.ok_or_else(|| plain(msg::CLI_NO_DATA_DIR))?;
    // The data dir must be there before it is locked and the store opens in it.
    std::fs::create_dir_all(&data_dir)
        .map_err(|detail| store_refused(&data_dir, &detail.to_string()))?;
    // One daemon per data folder, before anything is read or opened: a second
    // process must not migrate the configuration the first is running on, read
    // what it runs, or end it as the leftover of a crash (§9).
    let lock = DataDirLock::acquire(&data_dir).map_err(|error| lock_refused(&data_dir, error))?;
    // The configuration next: a file this build cannot read is a refusal,
    // whatever the rest of the machine says.
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_dir.join("config.toml"),
        Arc::new(SystemClock),
    ));
    let loaded = config.load().await.map_err(config_refused)?;
    log_inline_secrets(&loaded);
    let database = data_dir.join("ladderback.db");
    let store: Arc<dyn OperationStore> = Arc::new(
        SqliteStore::open(&database)
            .await
            .map_err(|detail| store_refused(&database, &detail.to_string()))?,
    );
    // Nothing has run yet, so an operation the log still shows as queued or
    // running was left by a process that stopped. It ends as failed now, which
    // also lets go of the repository a running maintenance operation held
    // (I7). A log that cannot be changed is a refusal: serving over it would
    // keep that repository held for good.
    let ended = RecoverOperations::new(Arc::clone(&store))
        .recover()
        .await
        .map_err(|error| recovery_refused(&database, &error))?;
    if ended > 0 {
        log::warn!(
            "ended {ended} operation(s) the operation log still showed as queued or running: the daemon that ran them stopped before they finished"
        );
    }
    let agent_id = agent_id::load_or_create(&data_dir)
        .map_err(|detail| store_refused(&data_dir, &detail.to_string()))?;
    // Workers are this program: `ladderback worker` (§4.1).
    let program = std::env::current_exe().map_err(|detail| Refused {
        message: msg::CLI_CANNOT_FIND_OWN_PATH,
        args: vec![("detail", text(detail))],
    })?;
    let bus: Arc<dyn EventBus> = Arc::new(BroadcastEventBus::new());
    let engine: Arc<dyn Engine> = Arc::new(EventEngine {
        inner: Arc::new(WorkerEngine::new(program, ["worker"])),
        bus: Arc::clone(&bus),
        clock: Arc::new(SystemClock),
    });
    let resolve = Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default())));
    let operations: Arc<dyn Operations> = Arc::new(PlanRunner {
        config: Arc::clone(&config),
        resolve: Arc::clone(&resolve),
        agent_id,
        engine: Arc::clone(&engine),
        store,
        retention: Arc::new(engine::RusticRetention),
        registry: Arc::new(CancelRegistry::new()),
        hooks: Arc::new(ProcessHookRunner::new()),
    });
    let repositories: Arc<dyn Repositories> = Arc::new(RepositoryService::new(
        Arc::new(SystemClock),
        Arc::clone(&engine),
        resolve.as_ref().clone(),
        Arc::clone(&config),
    ));
    let catalog: Arc<dyn Catalog> = Arc::new(ConfigCatalog { config, engine });
    Ok(Daemon {
        state: AppState {
            events: bus,
            operations,
            catalog,
            repositories,
        },
        lock,
    })
}

/// A data folder another process holds, or a lock that couldn't be taken, as
/// the message for whoever started the daemon.
fn lock_refused(data_dir: &std::path::Path, error: LockError) -> Refused {
    match error {
        LockError::InUse { holder: Some(pid) } => Refused {
            message: msg::CLI_DATA_DIR_IN_USE,
            args: vec![("path", text(data_dir.display())), ("pid", text(pid))],
        },
        LockError::InUse { holder: None } => Refused {
            message: msg::CLI_DATA_DIR_IN_USE_UNNAMED,
            args: vec![("path", text(data_dir.display()))],
        },
        LockError::Unavailable { detail } => Refused {
            message: msg::CLI_CANNOT_LOCK_DATA_DIR,
            args: vec![
                ("path", text(data_dir.join(LOCK_FILE).display())),
                ("detail", text(detail)),
            ],
        },
    }
}

/// An operation log that couldn't end what an earlier run left unfinished, as
/// the message for whoever started the daemon.
fn recovery_refused(database: &std::path::Path, error: &app::ports::StoreError) -> Refused {
    let (app::ports::StoreError::Unavailable { detail }
    | app::ports::StoreError::Corrupt { detail }) = error;
    Refused {
        message: msg::CLI_CANNOT_RECOVER_OPERATIONS,
        args: vec![("path", text(database.display())), ("detail", text(detail))],
    }
}

/// A configuration the daemon cannot use, as the message for whoever wrote it.
fn config_refused(error: ConfigError) -> Refused {
    fn refused(message: MessageId, args: Vec<(&'static str, MsgArg)>) -> Refused {
        Refused { message, args }
    }
    match error {
        ConfigError::Unavailable { file, detail } => refused(
            msg::CLI_CONFIG_UNAVAILABLE,
            vec![("file", text(file)), ("detail", text(detail))],
        ),
        ConfigError::TooLarge { file, limit } => refused(
            msg::CLI_CONFIG_TOO_LARGE,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
        ),
        ConfigError::SchemaMissing { file } => {
            refused(msg::CLI_CONFIG_SCHEMA_MISSING, vec![("file", text(file))])
        }
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => refused(
            msg::CLI_CONFIG_SCHEMA_NEWER,
            vec![
                ("file", text(file)),
                ("found", text(found)),
                ("supported", text(supported)),
            ],
        ),
        ConfigError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        } => {
            let mut where_ = key.unwrap_or_else(|| "an earlier rule".to_owned());
            if let Some(at) = at {
                where_ = format!("{where_} (line {}, column {})", at.line, at.column);
            }
            refused(
                msg::CLI_CONFIG_INVALID,
                vec![
                    ("file", text(file)),
                    ("where", text(where_)),
                    ("detail", text(format!("{problem:?}: {detail}"))),
                ],
            )
        }
    }
}

/// A folder or database the daemon cannot open.
fn store_refused(path: &std::path::Path, detail: &str) -> Refused {
    Refused {
        message: msg::CLI_CANNOT_OPEN_STORE,
        args: vec![("path", text(path.display())), ("detail", text(detail))],
    }
}

/// The engine as the daemon runs it (§4.2): every job's events go to the
/// event bus as [`UiEvent`]s on their way through, in the order the engine
/// reported them. The daemon never coalesces: that is the worker's, once
/// (§4.1).
struct EventEngine {
    inner: Arc<dyn Engine>,
    bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
}

#[async_trait]
impl Engine for EventEngine {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        // What the started event can name: the repository's ID, when the job
        // knows it already.
        let repository = match &job {
            Job::Backup(backup) => backup.repository.id.clone(),
            _ => None,
        }
        .map(|id| id.as_str().to_owned());
        let operation = op.as_uuid().hyphenated().to_string();
        let running = self.inner.run(op, job).await?;
        let mut publish = Publish {
            operation,
            repository,
            progress: HashMap::new(),
            clock: Arc::clone(&self.clock),
        };
        let bus = Arc::clone(&self.bus);
        Ok(RunningJob {
            // `then` keeps the engine's order: an event is published before
            // the runner sees it.
            events: running
                .events
                .then(move |event| {
                    let bus = Arc::clone(&bus);
                    let ui = publish.ui(&event);
                    async move {
                        if let Some(ui) = ui {
                            bus.publish(ui).await;
                        }
                        event
                    }
                })
                .boxed(),
            control: running.control,
        })
    }
}

/// Turns one run's [`EngineEvent`]s into [`UiEvent`]s. Progress bars
/// are stateful: each key remembers where it is, so an `Advance` is a number
/// the page can show.
struct Publish {
    operation: String,
    repository: Option<String>,
    progress: HashMap<u32, (u64, Option<u64>)>,
    clock: Arc<dyn Clock>,
}

impl Publish {
    fn ui(&mut self, event: &EngineEvent) -> Option<UiEvent> {
        use EngineEvent as E;
        match event {
            E::Started { index_read_at } => Some(UiEvent::OperationStarted {
                operation: self.operation.clone(),
                repository: self.repository.clone(),
                started_at: index_read_at.to_string(),
            }),
            E::Progress(progress) => self.progress_ui(progress),
            E::Warning(warning) => Some(UiEvent::OperationWarning {
                operation: self.operation.clone(),
                code: match warning.kind {
                    WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
                    WarningKind::Hook => WarningCode::HookWarning,
                    WarningKind::MaintenancePartial => WarningCode::MaintenancePartial,
                    WarningKind::Other => WarningCode::Unknown,
                },
                args: warning
                    .item
                    .as_ref()
                    .map(|item| vec![MsgArg::Text(item.clone())])
                    .unwrap_or_default(),
            }),
            E::Succeeded(_) => Some(UiEvent::OperationSucceeded {
                operation: self.operation.clone(),
                finished_at: self.clock.now().to_string(),
            }),
            E::Failed(reason) => {
                let (code, args) = failure(reason);
                Some(UiEvent::OperationFailed {
                    operation: self.operation.clone(),
                    code,
                    args,
                    finished_at: self.clock.now().to_string(),
                })
            }
        }
    }

    /// A progress bar's change as a `current` of `total` event, when it is
    /// one: a bar beginning, growing or learning its length. A title or a
    /// bar finishing is nothing a page needs.
    fn progress_ui(&mut self, progress: &ProgressEvent) -> Option<UiEvent> {
        use ProgressEvent as P;
        match progress {
            P::Begin { key, .. } => {
                self.progress.insert(*key, (0, None));
            }
            P::Length { key, len } => {
                let bar = self.progress.entry(*key).or_insert((0, None));
                bar.1 = Some(*len);
            }
            P::Advance { key, by } => {
                let bar = self.progress.entry(*key).or_insert((0, None));
                bar.0 = bar.0.saturating_add(*by);
            }
            P::Title { key, .. } | P::Finish { key } => {
                self.progress.remove(key);
                return None;
            }
        }
        let (current, total) = *self.progress.get(match progress {
            P::Begin { key, .. } | P::Length { key, .. } | P::Advance { key, .. } => key,
            P::Title { key, .. } | P::Finish { key } => key,
        })?;
        Some(UiEvent::OperationProgress {
            operation: self.operation.clone(),
            current,
            total,
        })
    }
}

/// A failure the runner reports, as the code and arguments the page renders.
fn failure(reason: &FailureReason) -> (ErrorCode, Vec<MsgArg>) {
    use FailureReason as F;
    fn with_detail(text: &str) -> Vec<MsgArg> {
        vec![MsgArg::Text(text.to_owned())]
    }
    match reason {
        F::EngineUnavailable { detail } | F::JobRejected { detail } => {
            (ErrorCode::EngineUnavailable, with_detail(detail))
        }
        F::EngineFailed { detail } => (ErrorCode::OperationFailed, with_detail(detail)),
        F::HookFailed { .. } => (ErrorCode::OperationFailed, Vec::new()),
        F::EngineExited { exit_code } => (
            ErrorCode::EngineExited,
            exit_code
                .map(|code| MsgArg::Text(code.to_string()))
                .into_iter()
                .collect(),
        ),
        F::ProtocolViolated { transition } => (
            ErrorCode::ProtocolViolated,
            vec![MsgArg::Text(format!("{transition:?}"))],
        ),
        F::DeadlinePassed => (ErrorCode::DeadlinePassed, Vec::new()),
        F::EngineStartTimedOut => (ErrorCode::EngineStartTimedOut, Vec::new()),
        F::EngineExitTimedOut => (ErrorCode::EngineExitTimedOut, Vec::new()),
        F::RepositoryExists => (ErrorCode::RepositoryExists, Vec::new()),
        F::DaemonStopped => (ErrorCode::DaemonStopped, Vec::new()),
    }
}

/// Starts the operations the API asks for: a backup of a named plan, run to
/// its end on a task of its own. What happens to the run arrives over the
/// events WebSocket, so the route answers with the operation's ID alone.
struct PlanRunner {
    config: Arc<dyn ConfigStore>,
    resolve: Arc<ResolveRepository>,
    agent_id: AgentId,
    engine: Arc<dyn Engine>,
    store: Arc<dyn OperationStore>,
    /// Evaluates each plan's retention for a forget run (§7.3).
    retention: Arc<dyn Retention>,
    /// The running operations' cancel switches, shared with the routes.
    registry: Arc<CancelRegistry>,
    /// Runs the agent, repository and plan hooks around every run (§6.1).
    hooks: Arc<dyn HookRunner>,
}

#[async_trait]
impl Operations for PlanRunner {
    async fn start_backup(&self, plan: String) -> Result<String, StartError> {
        let id = PlanId::try_from(plan).map_err(|_| StartError::UnknownPlan)?;
        let config = self
            .config
            .load()
            .await
            .map_err(|_| StartError::Unavailable)?;
        let plan = config.plan(&id).ok_or(StartError::UnknownPlan)?;
        let repository = config
            .repository(&plan.repository)
            .ok_or(StartError::UnknownPlan)?;
        let resolved = self
            .resolve
            .resolve(&repository.settings)
            .await
            .map_err(|_| StartError::Unavailable)?;
        let job = Job::Backup(BackupJob {
            repository: resolved.access,
            sources: plan.sources.iter().map(|s| s.as_str().to_owned()).collect(),
            tags: PlanTags::new(plan.id.clone(), self.agent_id),
            extra_tags: plan.tags.clone(),
            label: plan.label.clone(),
            deadline_budget: backup_budget(None, None),
        });
        // A UUIDv7: the log orders by ID, so the newest run is the highest.
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let id_text = operation.as_uuid().hyphenated().to_string();
        // The cancel switch is parked before the run starts — not lazily
        // when the run first polls it — so a cancel that arrives the
        // instant the route answers finds it (§4.2). A cancel that fires
        // only resolves; whether the receiver was still listening is
        // nobody's business here.
        let cancel = self.registry.register(operation);
        let cancel = async move {
            let _ = cancel.await;
        };
        // The agent, repository and plan hooks of §6.1 wrap the run, as the
        // configuration stands now: a rewrite between runs is picked up.
        let setup = HookSetup {
            agent: config.hooks().clone(),
            repository: config
                .repository(&plan.repository)
                .map(|repository| repository.hooks.clone())
                .unwrap_or_default(),
            plan: plan.hooks.clone(),
            plan_id: plan.id.clone(),
            repository_alias: plan.repository.clone(),
        };
        let use_case = RunOperation::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            Arc::clone(&self.store),
        )
        .with_hooks(Arc::clone(&self.hooks), setup);
        // Recorded as queued before the route answers, so a cancel that
        // arrives the instant it does finds the operation in the log. The
        // task does the recording and the run, so a handler that is dropped
        // meanwhile (the client went away) strands neither.
        let (recorded, wait_for_log) = futures::channel::oneshot::channel::<()>();
        let registry = Arc::clone(&self.registry);
        tokio::spawn(async move {
            let queued = use_case.queue(operation, Trigger::Manual, job).await;
            let _ = recorded.send(());
            // Nothing waits on the run: it reports through the event bus.
            let finished = use_case.run_queued(queued, cancel).await;
            registry.finish(&operation);
            log::info!(
                "operation {} ended: {:?}",
                finished.operation.id().as_uuid().hyphenated(),
                finished.operation.status()
            );
        });
        // A task that ended without recording has nothing to wait for.
        let _ = wait_for_log.await;
        Ok(id_text)
    }
    async fn start_maintenance(
        &self,
        repository: String,
        request: http_adapter::MaintenanceRequest,
    ) -> Result<String, http_adapter::MaintenanceStartError> {
        let alias = RepositoryAlias::parse(&repository)
            .map_err(|_| http_adapter::MaintenanceStartError::UnknownRepository)?;
        let kind = match request {
            http_adapter::MaintenanceRequest::Forget => MaintenanceKind::Forget,
            http_adapter::MaintenanceRequest::Prune => MaintenanceKind::Prune,
            http_adapter::MaintenanceRequest::Check => MaintenanceKind::Check,
            http_adapter::MaintenanceRequest::Adopt { into, from } => {
                let into = PlanId::parse(&into)
                    .map_err(|_| http_adapter::MaintenanceStartError::UnknownPlan { plan: into })?;
                MaintenanceKind::Adopt(Adoption {
                    from: match from {
                        http_adapter::AdoptionFrom::Orphaned { agent, plan } => {
                            AdoptionSource::Orphaned { agent, plan }
                        }
                        http_adapter::AdoptionFrom::Unmanaged => AdoptionSource::Unmanaged,
                    },
                    into,
                })
            }
        };
        let use_case = RunMaintenance::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            Arc::clone(&self.store),
            self.resolve.as_ref().clone(),
            Arc::clone(&self.retention),
            Arc::clone(&self.config),
            self.agent_id,
            platform::hostname(),
            Arc::new(engine::RusticSources),
        );
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let id_text = operation.as_uuid().hyphenated().to_string();
        let prepared = use_case.prepare(operation, &alias, kind).await;
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(app::maintenance::MaintenanceError::UnknownRepository) => {
                return Err(http_adapter::MaintenanceStartError::UnknownRepository);
            }
            Err(app::maintenance::MaintenanceError::Busy { running }) => {
                return Err(http_adapter::MaintenanceStartError::Busy {
                    running: running.as_uuid().hyphenated().to_string(),
                });
            }
            Err(app::maintenance::MaintenanceError::KeepsNothing { plan }) => {
                return Err(http_adapter::MaintenanceStartError::KeepsNothing {
                    plan: plan.as_str().to_owned(),
                });
            }
            Err(app::maintenance::MaintenanceError::UnknownPlan { plan }) => {
                return Err(http_adapter::MaintenanceStartError::UnknownPlan {
                    plan: plan.as_str().to_owned(),
                });
            }
            Err(app::maintenance::MaintenanceError::AdoptionNoMatch) => {
                return Err(http_adapter::MaintenanceStartError::AdoptionNoMatch);
            }
            Err(app::maintenance::MaintenanceError::AdoptionAmbiguous { plans }) => {
                return Err(http_adapter::MaintenanceStartError::AdoptionAmbiguous {
                    plans: plans.iter().map(|plan| plan.as_str().to_owned()).collect(),
                });
            }
            Err(_) => return Err(http_adapter::MaintenanceStartError::Unavailable),
        };
        // The slot is held; the run happens on a task of its own, and its
        // cancel switch is parked before it starts (§4.2).
        let cancel = self.registry.register(operation);
        let cancel = async move {
            let _ = cancel.await;
        };
        let registry = Arc::clone(&self.registry);
        tokio::spawn(async move {
            let finished = use_case.execute(prepared, cancel).await;
            registry.finish(&operation);
            log::info!(
                "operation {} ended: {:?}",
                finished.operation.id().as_uuid().hyphenated(),
                finished.operation.status()
            );
        });
        Ok(id_text)
    }

    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: Option<u64>,
    ) -> Result<api::OperationsPageDto, http_adapter::HistoryError> {
        let empty = || api::OperationsPageDto {
            operations: Vec::new(),
            next: None,
        };
        // A filter that names no plan that could exist matches nothing.
        let plan = match plan.map(PlanId::try_from) {
            Some(Ok(plan)) => Some(plan),
            Some(Err(invalid)) => {
                log::warn!("a plan filter no plan could match: {invalid}");
                return Ok(empty());
            }
            None => None,
        };
        let after = match after {
            Some(cursor) => match cursor.parse::<Uuid>() {
                Ok(uuid) => Some(OperationId::from_uuid(uuid)),
                Err(_) => return Err(http_adapter::HistoryError::BadCursor),
            },
            None => None,
        };
        let limit = limit.map(|asked| usize::try_from(asked).unwrap_or(usize::MAX));
        let page = match ListOperations::new(Arc::clone(&self.store))
            .list(plan, after, limit)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                log::error!("the operation log could not be read: {error}");
                return Ok(empty());
            }
        };
        Ok(api::OperationsPageDto {
            operations: page.operations.iter().filter_map(summary).collect(),
            next: page.next.map(|id| id.as_uuid().hyphenated().to_string()),
        })
    }

    async fn operation(
        &self,
        id: String,
    ) -> Result<api::OperationDetailDto, http_adapter::LookupError> {
        let id = operation_of(&id).ok_or(http_adapter::LookupError::Unknown)?;
        let detail = GetOperation::new(Arc::clone(&self.store))
            .get(id)
            .await
            .map_err(|error| {
                log::error!("the operation log could not be read: {error}");
                http_adapter::LookupError::Unknown
            })?
            .ok_or(http_adapter::LookupError::Unknown)?;
        let Some(operation) = summary(&detail.record) else {
            log::error!(
                "a stored operation the page cannot show: {}",
                detail.record.id.as_uuid().hyphenated()
            );
            return Err(http_adapter::LookupError::Unknown);
        };
        Ok(api::OperationDetailDto {
            operation,
            warnings: detail
                .warnings
                .iter()
                .map(|warning| api::OperationWarningDto {
                    code: warning_code(warning.kind),
                    item: warning.item.clone(),
                    during: warning.during.clone(),
                    detail: warning.detail.clone(),
                })
                .collect(),
        })
    }

    async fn cancel(&self, id: String) -> Result<(), http_adapter::CancelError> {
        let Some(id) = operation_of(&id) else {
            return Err(http_adapter::CancelError::Unknown);
        };
        // The log says which of the two refusals this is: unknown, or known
        // but ended. Only a running operation has a handle to fire (§4.2).
        let running = match GetOperation::new(Arc::clone(&self.store)).get(id).await {
            Ok(Some(detail)) => !detail.record.status.is_finished(),
            Ok(None) => return Err(http_adapter::CancelError::Unknown),
            Err(error) => {
                log::error!("the operation log could not be read: {error}");
                return Err(http_adapter::CancelError::Unknown);
            }
        };
        if running && self.registry.cancel(&id) {
            Ok(())
        } else {
            Err(http_adapter::CancelError::NotRunning)
        }
    }
}

/// An operation's ID as the wire carries it, when the text is one.
fn operation_of(text: &str) -> Option<OperationId> {
    text.parse::<Uuid>().ok().map(OperationId::from_uuid)
}

/// One log record as the history page shows it, or `None` when it holds
/// something no page could show. The page carries the status; the warning
/// codes come with the detail, where the kept warnings are.
fn summary(record: &OperationRecord) -> Option<api::OperationSummaryDto> {
    let failure_code = match &record.status {
        OperationStatus::Failed { reason } => Some(failure(reason).0),
        _ => None,
    };
    api::OperationSummaryDto::new(
        record.id.as_uuid().hyphenated().to_string(),
        record
            .target
            .plan
            .as_ref()
            .map(|plan| plan.as_str().to_owned())
            .unwrap_or_default(),
        kind_text(record.kind).to_owned(),
        status_text(&record.status).to_owned(),
        record.queued_at.to_string(),
        record.started_at.map(|at| at.to_string()),
        record.finished_at.map(|at| at.to_string()),
        Vec::new(),
        failure_code,
        record
            .result
            .as_ref()
            .and_then(|result| result_dto(result, record)),
    )
    .map_err(|invalid| log::warn!("an operation the page cannot show: {invalid:?}"))
    .ok()
}

/// What a successful operation produced, as the page shows it: the small
/// typed summary of its kind, with the run's duration.
fn result_dto(result: &EngineResult, record: &OperationRecord) -> Option<api::OperationResultDto> {
    let duration_ms = record
        .started_at
        .zip(record.finished_at)
        .and_then(|(from, to)| u64::try_from(to.as_millisecond() - from.as_millisecond()).ok())
        .unwrap_or(0);
    match result {
        EngineResult::Backup(backup) => Some(api::OperationResultDto::Backup {
            files: backup.files_new + backup.files_changed + backup.files_unmodified,
            bytes: backup.data_added,
            duration_ms,
        }),
        EngineResult::Prune(prune) => Some(api::OperationResultDto::Prune {
            packs_deleted: prune.packs_deleted,
            duration_ms,
        }),
        EngineResult::Check(check) => Some(api::OperationResultDto::Check {
            errors: check.errors,
            warnings: check.warnings,
            duration_ms,
        }),
        EngineResult::Forget(forget) => Some(api::OperationResultDto::Forget {
            removed: u64::try_from(forget.removed.len()).unwrap_or(0),
            duration_ms,
        }),
        EngineResult::Rewrite(rewrite) => Some(api::OperationResultDto::Adopt {
            rewritten: u64::try_from(rewrite.created.len()).unwrap_or(0),
            duration_ms,
        }),
        EngineResult::Fake { .. }
        | EngineResult::Repository { .. }
        | EngineResult::Snapshots(_) => None,
    }
}

/// An operation's kind, as the wire names it.
fn kind_text(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Fake => "fake",
        OperationKind::Identify => "identify",
        OperationKind::Init => "init",
        OperationKind::Backup => "backup",
        OperationKind::Forget => "forget",
        OperationKind::Adopt => "adopt",
        OperationKind::Prune => "prune",
        OperationKind::Check => "check",
        OperationKind::Ls => "ls",
    }
}

/// An operation's status, as the wire names it.
fn status_text(status: &OperationStatus) -> &'static str {
    match status {
        OperationStatus::Queued => "queued",
        OperationStatus::Running => "running",
        OperationStatus::Succeeded => "succeeded",
        OperationStatus::SucceededWithWarnings => "succeeded-with-warnings",
        OperationStatus::Failed { .. } => "failed",
        OperationStatus::Cancelled => "cancelled",
    }
}

/// A warning's kind, as the wire names it.
fn warning_code(kind: WarningKind) -> WarningCode {
    match kind {
        WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
        WarningKind::Hook => WarningCode::HookWarning,
        WarningKind::MaintenancePartial => WarningCode::MaintenancePartial,
        WarningKind::Other => WarningCode::Unknown,
    }
}

/// An import that saved nothing, as the code, arguments and detail the
/// catalog renders (§8.4).
pub(crate) struct Refusal {
    pub(crate) code: ErrorCode,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
    pub(crate) detail: Option<String>,
}

/// Something an import did that the person should know, as the code and
/// arguments the catalog renders.
pub(crate) struct Notice {
    pub(crate) code: WarningCode,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
}

/// The `line` argument of a message about a place in a file, left out when
/// no place is known.
fn line_arg(at: Option<app::ports::Position>) -> Option<(&'static str, MsgArg)> {
    at.map(|at| ("line", text(at.line)))
}

/// What came of a refused import. `config_file` is the configuration's own
/// path, which a refusal about the merged configuration (rather than about a
/// file the store read) names.
pub(crate) fn import_refusal(error: ImportError, config_file: &std::path::Path) -> Refusal {
    match error {
        ImportError::Profile(error) => profile_refusal(error),
        ImportError::AliasTaken(alias) => Refusal {
            code: ErrorCode::RepositoryAliasTaken,
            args: vec![("alias", text(alias.as_str()))],
            detail: None,
        },
        ImportError::PlanTaken(plan) => Refusal {
            code: ErrorCode::PlanIdTaken,
            args: vec![("plan", text(plan.as_str()))],
            detail: None,
        },
        ImportError::Config(error) => profile_config_refusal(error, config_file),
    }
}

/// What came of a refused export: the code, arguments and detail the catalog
/// renders. `config_file` is as for [`import_refusal`].
pub(crate) fn export_refusal(error: ExportError, config_file: &std::path::Path) -> Refusal {
    match error {
        ExportError::PlanUnknown(plan) => plan_unknown(plan.as_str()),
        ExportError::Render(ExportRefusal::InlineSecret) => Refusal {
            code: ErrorCode::InlineSecret,
            args: Vec::new(),
            detail: None,
        },
        ExportError::Render(ExportRefusal::Reference { key }) => Refusal {
            code: ErrorCode::ProfileExportReference,
            args: vec![("key", text(key))],
            detail: None,
        },
        ExportError::Render(ExportRefusal::UrlPassword { key }) => Refusal {
            code: ErrorCode::ProfileExportUrlPassword,
            args: vec![("key", text(key))],
            detail: None,
        },
        ExportError::Render(ExportRefusal::TagComma { tag }) => Refusal {
            code: ErrorCode::ProfileExportTagComma,
            args: vec![("tag", text(tag))],
            detail: None,
        },
        ExportError::Config(error) => profile_config_refusal(error, config_file),
    }
}

/// A plan the configuration does not have, or a name no plan can have.
pub(crate) fn plan_unknown(plan: &str) -> Refusal {
    Refusal {
        code: ErrorCode::PlanUnknown,
        args: vec![("plan", text(plan))],
        detail: None,
    }
}

/// A name for a repository that is not an alias: the one a person asked for,
/// or what could be made of a profile's file name.
pub(crate) fn repository_alias_invalid() -> Refusal {
    Refusal {
        code: ErrorCode::RepositoryAliasInvalid,
        args: Vec::new(),
        detail: None,
    }
}

/// A command that has no folder to keep the configuration in: the platform
/// names no home folder and no configuration folder is set.
pub(crate) fn config_folder_unknown() -> Refusal {
    Refusal {
        code: ErrorCode::ConfigFolderUnknown,
        args: Vec::new(),
        detail: None,
    }
}

/// A command whose async runtime did not start. The detail is the operating
/// system's own message, which the command line shows as it came.
pub(crate) fn runtime_unavailable(error: &std::io::Error) -> Refusal {
    Refusal {
        code: ErrorCode::RuntimeUnavailable,
        args: Vec::new(),
        detail: Some(error.to_string()),
    }
}

/// What the reader said was wrong with a profile.
fn profile_refusal(error: ProfileError) -> Refusal {
    match error {
        ProfileError::Unreadable { file } => Refusal {
            code: ErrorCode::ProfileUnreadable,
            args: vec![("file", text(file))],
            detail: None,
        },
        ProfileError::TooLarge { file, limit } => Refusal {
            code: ErrorCode::ProfileTooLarge,
            args: vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            detail: None,
        },
        ProfileError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        } => {
            let mut args = vec![("file", text(file))];
            args.extend(key.map(|key| ("key", text(key))));
            args.extend(line_arg(at));
            Refusal {
                code: ErrorCode::ProfileInvalid,
                args,
                detail: Some(problem_detail(&format!("{problem:?}"), &detail)),
            }
        }
        ProfileError::Unsupported { file, key, at } => {
            let mut args = vec![("file", text(file)), ("key", text(key))];
            args.extend(line_arg(at));
            Refusal {
                code: ErrorCode::ProfileUnsupported,
                args,
                detail: None,
            }
        }
    }
}

/// What the store said was wrong with the configuration an import would have
/// written: the answer a person gets for the file they know.
fn profile_config_refusal(error: ConfigError, config_file: &std::path::Path) -> Refusal {
    fn refusal(
        code: ErrorCode,
        args: Vec<(&'static str, MsgArg)>,
        detail: Option<String>,
    ) -> Refusal {
        Refusal { code, args, detail }
    }
    match error {
        ConfigError::Unavailable { file, detail } => refusal(
            ErrorCode::ConfigUnavailable,
            vec![("file", text(file))],
            Some(detail),
        ),
        ConfigError::TooLarge { file, limit } => refusal(
            ErrorCode::ConfigTooLarge,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            None,
        ),
        ConfigError::SchemaMissing { file } => refusal(
            ErrorCode::ConfigSchemaMissing,
            vec![("file", text(file))],
            None,
        ),
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => refusal(
            ErrorCode::ConfigSchemaNewer,
            vec![
                ("file", text(file)),
                ("found", text(found)),
                ("supported", text(supported)),
            ],
            None,
        ),
        ConfigError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        } => {
            // The use case's own check of the merged configuration names no
            // file: it is the configuration the import would have written.
            let file = if file.is_empty() {
                config_file.display().to_string()
            } else {
                file
            };
            let mut args = vec![("file", text(file))];
            args.extend(line_arg(at));
            let what = problem_detail(&format!("{problem:?}"), &detail);
            refusal(
                ErrorCode::ConfigInvalid,
                args,
                Some(match key {
                    Some(key) => format!("{key}: {what}"),
                    None => what,
                }),
            )
        }
    }
}

/// What is wrong, as the problem's name and the parser's words when it had
/// any.
fn problem_detail(problem: &str, detail: &str) -> String {
    if detail.is_empty() {
        problem.to_owned()
    } else {
        format!("{problem}: {detail}")
    }
}

/// A notice of an import or export, as the warning the catalog renders.
pub(crate) fn profile_notice(notice: ProfileNotice) -> Notice {
    let (code, args) = match notice {
        ProfileNotice::KeyIgnored { key } => {
            (WarningCode::ProfileKeyIgnored, vec![("key", text(key))])
        }
        ProfileNotice::Prune => (WarningCode::ProfilePrune, Vec::new()),
        ProfileNotice::EnvIgnored => (WarningCode::ProfileEnvIgnored, Vec::new()),
        ProfileNotice::HooksPerPlan => (WarningCode::ProfileHooksPerPlan, Vec::new()),
        ProfileNotice::UsedProfileMissing { name } => {
            (WarningCode::ProfileUsedMissing, vec![("name", text(name))])
        }
        ProfileNotice::InlineSecret { field } => {
            (WarningCode::InlineSecret, vec![("field", text(field))])
        }
        ProfileNotice::TagDropped { tag } => {
            (WarningCode::ProfileTagDropped, vec![("tag", text(tag))])
        }
        ProfileNotice::OptionLeftOut { option, variable } => (
            WarningCode::ProfileOptionLeftOut,
            vec![("option", text(option)), ("variable", text(variable))],
        ),
        ProfileNotice::RetentionEmpty => (WarningCode::ProfileRetentionEmpty, Vec::new()),
    };
    Notice { code, args }
}

/// The running operations' cancel switches (§4.2): what the API cancels
/// through. A handle is registered when an operation starts running and
/// dropped when it ends, so only a running operation can be cancelled, once.
struct CancelRegistry {
    running: std::sync::Mutex<HashMap<OperationId, futures::channel::oneshot::Sender<()>>>,
}

/// The catalog the API asks of the daemon (§4.2): the configuration's plans
/// and repositories, and one repository's snapshots, listed live through the
/// engine (§9: a read stores nothing and takes no slot).
struct ConfigCatalog {
    config: Arc<dyn ConfigStore>,
    engine: Arc<dyn Engine>,
}

#[async_trait]
impl Catalog for ConfigCatalog {
    async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
        let config = self
            .config
            .load()
            .await
            .map_err(|_| CatalogError::Unavailable)?;
        Ok(api::PlansDto {
            plans: config
                .plans()
                .iter()
                .map(|plan| api::PlanSummaryDto {
                    id: plan.id.as_str().to_owned(),
                    repository: plan.repository.as_str().to_owned(),
                })
                .collect(),
        })
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
        let config = self
            .config
            .load()
            .await
            .map_err(|_| CatalogError::Unavailable)?;
        Ok(api::RepositoriesDto {
            repositories: config
                .repositories()
                .iter()
                .map(|(alias, repository)| repository_row(alias, &repository.settings))
                .collect(),
        })
    }

    async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
        // An alias no repository could have is as unknown as one the
        // configuration happens not to have.
        let alias = RepositoryAlias::parse(&alias).map_err(|_| SnapshotsError::Unknown)?;
        let config = self
            .config
            .load()
            .await
            .map_err(|_| SnapshotsError::Unavailable)?;
        let repository = config.repository(&alias).ok_or(SnapshotsError::Unknown)?;
        let listing = ListSnapshots::new(
            Arc::clone(&self.engine),
            ResolveRepository::new(Arc::new(SystemSecrets::default())),
        )
        .list(OperationId::from_uuid(Uuid::now_v7()), &repository.settings)
        .await;
        let listed = match listing {
            Ok(listed) => listed,
            // A secret: its code and the words a command or the platform
            // gave, masked of every secret of the repository, in the log
            // as in the answer (I4).
            Err(SnapshotError::Resolve(error)) => {
                let refusal = secret_refusal(&error, &repository.settings).await;
                log::warn!(
                    "the snapshots of {} could not be listed: {:?}{}",
                    alias.as_str(),
                    refusal.code,
                    refusal
                        .detail
                        .as_deref()
                        .map(|detail| format!(" ({detail})"))
                        .unwrap_or_default()
                );
                return Err(SnapshotsError::SecretRefused(refusal));
            }
            Err(error) => {
                let said = failure_redactor(&repository.settings)
                    .await
                    .redact(&format!("{error:?}"));
                log::warn!(
                    "the snapshots of {} could not be listed: {said}",
                    alias.as_str()
                );
                return Err(SnapshotsError::Unavailable);
            }
        };
        let snapshots = listed
            .iter()
            .filter_map(|snapshot| {
                snapshot_dto(snapshot)
                    .map_err(|problem| {
                        log::error!(
                            "a snapshot of {} the page cannot show: {problem:?}",
                            alias.as_str()
                        );
                    })
                    .ok()
            })
            .collect();
        Ok(api::SnapshotsDto { snapshots })
    }
}

/// A listed snapshot as the page is told of it, with what the engine cut
/// from it said.
fn snapshot_dto(snapshot: &SnapshotSummary) -> Result<api::SnapshotDto, api::InvalidSnapshot> {
    api::SnapshotDto::new(
        snapshot.id.as_str().to_owned(),
        snapshot.time.to_string(),
        snapshot.hostname.clone(),
        snapshot.label.clone(),
        snapshot.tags.clone(),
        snapshot.paths.clone(),
    )
    .map(|dto| dto.truncated(snapshot.paths_truncated, snapshot.tags_truncated))
}

/// Tests, connects and initialises repositories for the API (§4.2): each
/// request's settings become domain values, run through the use cases of
/// `app`, and every refusal becomes a typed one. Writes to the configuration
/// go one at a time, so two connections made at the same moment are both
/// kept.
struct RepositoryService {
    clock: Arc<dyn Clock>,
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
    config: Arc<dyn ConfigStore>,
    /// Held across a use case's read-modify-write of the configuration.
    writes: tokio::sync::Mutex<()>,
}

impl RepositoryService {
    fn new(
        clock: Arc<dyn Clock>,
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
    ) -> Self {
        Self {
            clock,
            engine,
            resolve,
            config,
            writes: tokio::sync::Mutex::new(()),
        }
    }
}

#[async_trait]
impl Repositories for RepositoryService {
    async fn test(
        &self,
        request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        let settings = settings_of(request.settings)?;
        let id = match TestRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
        )
        .test(new_operation(), &settings)
        .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryIdDto {
            id: id.as_str().to_owned(),
        })
    }

    async fn connect(
        &self,
        request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        // The name first: a repository that can't be saved isn't tested.
        let alias =
            RepositoryAlias::parse(&request.alias).map_err(|_| RepositoriesError::AliasInvalid)?;
        let settings = settings_of(request.settings)?;
        let _writing = self.writes.lock().await;
        let id = match ConnectRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        )
        .connect(new_operation(), alias, &settings)
        .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryIdDto {
            id: id.as_str().to_owned(),
        })
    }

    async fn init(
        &self,
        request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, RepositoriesError> {
        // The name first: a repository that was created and then can't be
        // saved is one the person has no way to find again.
        let alias = request
            .alias
            .as_deref()
            .map(RepositoryAlias::parse)
            .transpose()
            .map_err(|_| RepositoriesError::AliasInvalid)?;
        let settings = settings_of(request.settings)?;
        // Only a saving initialisation writes the configuration.
        let _writing = match alias {
            Some(_) => Some(self.writes.lock().await),
            None => None,
        };
        let initialised = match InitRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        )
        .init(new_operation(), &settings, alias)
        .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryInitDto {
            id: initialised.id.as_str().to_owned(),
            saved: initialised.saved,
        })
    }
}

/// A fresh operation ID: a UUIDv7, as every operation of the daemon has.
fn new_operation() -> OperationId {
    OperationId::from_uuid(Uuid::now_v7())
}

/// A request's settings as domain values, read as the configuration reader
/// reads the same keys: a password file is an absolute path, and a keyring
/// entry is `service/name`. A typed password and a location (hot ones too)
/// are not empty. What it can't take is an invalid body.
fn settings_of(dto: api::RepositorySettingsDto) -> Result<RepositorySettings, RepositoriesError> {
    let empty_password =
        matches!(&dto.password, api::PasswordDto::Inline { value } if value.is_empty());
    if empty_password || dto.repository.is_empty() || dto.repo_hot.as_deref() == Some("") {
        return Err(RepositoriesError::InvalidBody);
    }
    let password = match dto.password {
        api::PasswordDto::Inline { value } => SecretRef::Inline(SecretValue::new(value)),
        api::PasswordDto::File { path } if path.starts_with('/') => SecretRef::File {
            path,
            read: FileRead::FirstLine,
        },
        api::PasswordDto::File { .. } => return Err(RepositoriesError::InvalidBody),
        api::PasswordDto::Keyring { entry } => SecretRef::Keyring(
            KeyringEntry::parse(&entry).map_err(|_| RepositoriesError::InvalidBody)?,
        ),
        api::PasswordDto::Env { name } => SecretRef::Env(name),
    };
    Ok(RepositorySettings {
        repository: dto.repository,
        repo_hot: dto.repo_hot,
        password,
        options: dto.options,
        options_hot: dto.options_hot,
        options_cold: dto.options_cold,
    })
}

/// Why a repository use case failed, as the typed refusal the route answers
/// with. `settings` are the request's: a location the refusal names is shown
/// redacted.
async fn refusal_of(error: RepositoryError, settings: &RepositorySettings) -> RepositoriesError {
    match error {
        RepositoryError::Exists => exists(settings),
        RepositoryError::AliasTaken(alias) => RepositoriesError::AliasTaken {
            alias: alias.as_str().to_owned(),
        },
        RepositoryError::Resolve(error) => {
            RepositoriesError::SecretRefused(secret_refusal(&error, settings).await)
        }
        RepositoryError::Engine(EngineError::Unavailable { detail }) => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        RepositoryError::Engine(EngineError::StartTimedOut) => {
            unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None)
        }
        RepositoryError::Failed(reason) => failed_refusal(reason, settings),
        RepositoryError::Config(error) => config_refusal(error),
        RepositoryError::TimedOut => unavailable(ErrorCode::RepositoryTimedOut, Vec::new(), None),
    }
}

fn unavailable(
    code: ErrorCode,
    args: Vec<(&'static str, MsgArg)>,
    detail: Option<String>,
) -> RepositoriesError {
    RepositoriesError::Unavailable { code, args, detail }
}

fn exists(settings: &RepositorySettings) -> RepositoriesError {
    RepositoriesError::Exists {
        location: redacted_location(settings, &settings.repository),
    }
}

/// A secret that couldn't be resolved, as the typed refusal of its cause.
/// What a platform or a command said is masked of every secret of the
/// repository first (I4).
async fn secret_refusal(error: &ResolveError, settings: &RepositorySettings) -> SecretRefusal {
    use domain::secrets::Problem;
    let redactor = failure_redactor(settings).await;
    let field = || ("field", MsgArg::Text(field_name(&error.field)));
    let refusal = |code, args: Vec<(&'static str, MsgArg)>, detail: Option<String>| SecretRefusal {
        code,
        args,
        detail: detail.filter(|detail| !detail.is_empty()),
    };
    let bytes = |limit: usize| MsgArg::Bytes(u64::try_from(limit).unwrap_or(u64::MAX));
    match &error.cause {
        Cause::Unresolved(secret) => {
            let source = (
                "source",
                MsgArg::Text(redactor.redact(&source_text(&secret.source))),
            );
            let named = vec![field(), source.clone()];
            match &secret.problem {
                Problem::Missing => refusal(ErrorCode::SecretMissing, named, None),
                Problem::Unreachable { detail } => refusal(
                    ErrorCode::SecretUnreadable,
                    named,
                    Some(masked_detail(&redactor, detail)),
                ),
                Problem::NotUnicode => refusal(ErrorCode::SecretNotText, named, None),
                Problem::TooLarge { limit } => refusal(
                    ErrorCode::SecretTooLarge,
                    vec![field(), source, ("limit", bytes(*limit))],
                    None,
                ),
                Problem::CommandFailed { detail, .. } => refusal(
                    ErrorCode::SecretCommandFailed,
                    named,
                    Some(masked_detail(&redactor, detail)),
                ),
                // Only a command is run for a time; a file or a keyring that
                // doesn't answer is a source that can't be read.
                Problem::TimedOut { seconds } => match secret.source {
                    SourceName::Command { .. } => refusal(
                        ErrorCode::SecretCommandTimedOut,
                        vec![field(), source, ("seconds", MsgArg::Count(*seconds))],
                        None,
                    ),
                    SourceName::Keyring { .. }
                    | SourceName::File { .. }
                    | SourceName::Env { .. } => refusal(ErrorCode::SecretUnreadable, named, None),
                },
            }
        }
        Cause::ReferenceInProcessArguments => {
            refusal(ErrorCode::SecretInProcessArguments, vec![field()], None)
        }
        Cause::ExpansionTooLong { limit } => refusal(
            ErrorCode::SecretExpansionTooLong,
            vec![field(), ("limit", bytes(*limit))],
            None,
        ),
        Cause::Invalid(template) => {
            let mut args = vec![field()];
            if let Some(at) = template_error_place(template) {
                args.push(("at", MsgArg::Text(at.to_string())));
            }
            refusal(ErrorCode::TemplateInvalid, args, None)
        }
    }
}

/// What a command or a platform said, with the secrets of the repository
/// masked. A line that was cut at its cap (it ends in `…`) first loses as
/// many bytes of its end as the longest secret has, less one: a secret that
/// straddles the cut can't leave its first characters behind, for the mask
/// only finds what is whole.
fn masked_detail(redactor: &Redactor, detail: &str) -> String {
    let Some(cut) = detail.strip_suffix('…') else {
        return redactor.redact(detail);
    };
    let kept = cut
        .len()
        .saturating_sub(redactor.longest_match().saturating_sub(1));
    let end = (0..=kept)
        .rev()
        .find(|&at| cut.is_char_boundary(at))
        .unwrap_or_default();
    format!("{}…", redactor.redact(cut.get(..end).unwrap_or_default()))
}

/// Where in a setting a template goes wrong, when the fault has a place.
fn template_error_place(error: &TemplateError) -> Option<usize> {
    match error {
        TemplateError::TooLong { .. } | TemplateError::TooManyReferences { .. } => None,
        TemplateError::Unterminated { at }
        | TemplateError::EmptyReference { at }
        | TemplateError::InvalidName { at }
        | TemplateError::EmptyPath { at }
        | TemplateError::RelativePath { at }
        | TemplateError::NestedReference { at } => Some(*at),
    }
}

/// What names a source in a refusal: the variable, the path, the keyring
/// entry or the program, never what it gave.
fn source_text(source: &SourceName) -> String {
    match source {
        SourceName::Command { program } => program.clone(),
        SourceName::Keyring { entry } => entry.clone(),
        SourceName::File { path } => path.clone(),
        SourceName::Env { name } => name.clone(),
    }
}

/// How long a secret is read again for a redactor: a source that does not
/// answer in this time adds nothing, and a failure doesn't wait for it.
const REREAD_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// A secret read again, for a redactor: `None` when it can't be read, since
/// what can't be read can't show in what a failure says.
async fn read_again(resolver: &SystemSecrets, secret: &SecretRef) -> Option<SecretValue> {
    tokio::time::timeout(REREAD_LIMIT, resolver.resolve(secret))
        .await
        .ok()?
        .ok()
        .map(|found| found.value)
}

/// A redactor for what a failure of `settings` says: its inline password and
/// credential options, and what the references of its settings, and a
/// password read from the environment or a file, resolve to. A failed
/// resolution stops at the first fault, so the other secrets of the
/// repository were not read yet: they are read again here, so that a command
/// that prints one of them in its error doesn't show it. A command is not
/// run a second time.
async fn failure_redactor(settings: &RepositorySettings) -> Redactor {
    let resolver = SystemSecrets::default();
    let mut resolved = Vec::new();
    if matches!(
        settings.password,
        SecretRef::Env(_) | SecretRef::File { .. }
    ) && let Some(value) = read_again(&resolver, &settings.password).await
    {
        resolved.push(value);
    }
    let texts = std::iter::once(&settings.repository)
        .chain(&settings.repo_hot)
        .chain(settings.options.values())
        .chain(settings.options_hot.values())
        .chain(settings.options_cold.values());
    // Each distinct reference once, as the resolution reads it.
    let mut references = std::collections::BTreeSet::new();
    for text in texts {
        if let Ok(template) = Template::parse(text) {
            for part in template.parts() {
                if let Part::Reference(reference) = part {
                    references.insert(reference.clone());
                }
            }
        }
    }
    for reference in references {
        if let Some(value) = read_again(&resolver, &reference.secret_ref()).await {
            resolved.push(value);
        }
    }
    let none = SecretValue::new("");
    Redactor::for_repository(RepositorySecrets {
        repository: &settings.repository,
        repo_hot: settings.repo_hot.as_deref(),
        password: settings.password.inline().unwrap_or(&none),
        options: &settings.options,
        options_hot: &settings.options_hot,
        options_cold: &settings.options_cold,
        resolved: &resolved,
    })
}

/// Where a secret failed to resolve, as the catalog names it: the setting's
/// own name in the configuration.
fn field_name(field: &Field) -> String {
    match field {
        Field::Repository => "repository".to_owned(),
        Field::RepoHot => "repo-hot".to_owned(),
        Field::Password => "password".to_owned(),
        Field::Option(name) => format!("options.{name}"),
        Field::OptionHot(name) => format!("options-hot.{name}"),
        Field::OptionCold(name) => format!("options-cold.{name}"),
    }
}

/// A job that ran and failed: the engine's own words for a failure it
/// reports, a typed code for the rest. The engine redacts what it says.
fn failed_refusal(reason: FailureReason, settings: &RepositorySettings) -> RepositoriesError {
    use FailureReason as F;
    match reason {
        F::EngineFailed { detail } => RepositoriesError::OperationFailed { detail },
        F::EngineUnavailable { detail } | F::JobRejected { detail } => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        F::EngineExited { exit_code } => unavailable(
            ErrorCode::EngineExited,
            exit_code
                .map(|code| ("exit_code", text(code)))
                .into_iter()
                .collect(),
            None,
        ),
        F::ProtocolViolated { transition } => unavailable(
            ErrorCode::ProtocolViolated,
            vec![("transition", text(format!("{transition:?}")))],
            None,
        ),
        F::DeadlinePassed => unavailable(ErrorCode::DeadlinePassed, Vec::new(), None),
        F::EngineStartTimedOut => unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None),
        F::EngineExitTimedOut => unavailable(ErrorCode::EngineExitTimedOut, Vec::new(), None),
        // Only a backup's before-hook stops a job this way: a repository job
        // runs no hooks. It is still a failure, in the catalog's plainest words.
        F::HookFailed { .. } => unavailable(ErrorCode::OperationFailed, Vec::new(), None),
        F::RepositoryExists => exists(settings),
        F::DaemonStopped => unavailable(ErrorCode::DaemonStopped, Vec::new(), None),
    }
}

/// A configuration that couldn't be read or written, as the code and the
/// arguments its message declares. The store never puts a value in what it
/// reports (`ConfigError`), so its words can be shown.
fn config_refusal(error: ConfigError) -> RepositoriesError {
    match error {
        ConfigError::Unavailable { file, detail } => unavailable(
            ErrorCode::ConfigUnavailable,
            vec![("file", text(file))],
            Some(detail),
        ),
        ConfigError::TooLarge { file, limit } => unavailable(
            ErrorCode::ConfigTooLarge,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            None,
        ),
        ConfigError::SchemaMissing { file } => unavailable(
            ErrorCode::ConfigSchemaMissing,
            vec![("file", text(file))],
            None,
        ),
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => unavailable(
            ErrorCode::ConfigSchemaNewer,
            vec![
                ("file", text(file)),
                ("found", text(found)),
                ("supported", text(supported)),
            ],
            None,
        ),
        ConfigError::Invalid {
            file,
            key,
            at,
            detail,
            ..
        } => {
            let mut args = vec![("file", text(file))];
            if let Some(at) = at {
                args.push(("line", text(at.line)));
            }
            let detail = match (key, detail.is_empty()) {
                (Some(key), false) => Some(format!("{key}: {detail}")),
                (Some(key), true) => Some(key),
                (None, false) => Some(detail),
                (None, true) => None,
            };
            unavailable(ErrorCode::ConfigInvalid, args, detail)
        }
    }
}

/// One configured repository as a list shows it: its locations masked, and
/// where its password comes from, never the password (I4).
fn repository_row(alias: &RepositoryAlias, settings: &RepositorySettings) -> api::RepositoryDto {
    api::RepositoryDto {
        alias: alias.as_str().to_owned(),
        repository: redacted_location(settings, &settings.repository),
        repo_hot: settings
            .repo_hot
            .as_deref()
            .map(|hot| redacted_location(settings, hot)),
        password: match &settings.password {
            SecretRef::Inline(_) => api::PasswordSourceDto::Inline,
            SecretRef::File { path, .. } => api::PasswordSourceDto::File { path: path.clone() },
            SecretRef::Keyring(entry) => api::PasswordSourceDto::Keyring {
                entry: entry.to_string(),
            },
            SecretRef::Env(name) => api::PasswordSourceDto::Env { name: name.clone() },
            // The program only: its arguments may hold the secret itself.
            SecretRef::Command(command) => api::PasswordSourceDto::Command {
                program: command.program().to_owned(),
            },
        },
        warnings: settings
            .inline_secrets()
            .iter()
            .map(|secret| api::WarningDto {
                code: WarningCode::ConfigInlineSecret,
                args: [("field".to_owned(), MsgArg::Text(secret.field()))].into(),
            })
            .collect(),
    }
}

/// A location of `settings`, as it may be shown: with the password a URL
/// holds masked. The typed password is not masked: it isn't in a location,
/// and a location that happens to spell it is still a location.
fn redacted_location(settings: &RepositorySettings, text: &str) -> String {
    Redactor::for_repository(RepositorySecrets {
        repository: &settings.repository,
        repo_hot: settings.repo_hot.as_deref(),
        password: &SecretValue::new(""),
        options: &settings.options,
        options_hot: &settings.options_hot,
        options_cold: &settings.options_cold,
        resolved: &[],
    })
    .redact(text)
}

impl CancelRegistry {
    fn new() -> Self {
        Self {
            running: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The map, recovered even from a poisoned lock: the alternative —
    /// handing out a receiver whose sender is gone — would cancel every
    /// operation that starts next.
    fn running(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<OperationId, futures::channel::oneshot::Sender<()>>>
    {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers `id` as running, returning the future its run ends on when
    /// the API cancels it.
    fn register(&self, id: OperationId) -> futures::channel::oneshot::Receiver<()> {
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.running().insert(id, sender);
        receiver
    }

    /// Cancels `id`: `true` when it was running and is now cancelled, `false`
    /// when nothing by that ID is running, so there is nothing to cancel.
    /// A second cancel of the same operation is `false`: the one handle is
    /// spent.
    fn cancel(&self, id: &OperationId) -> bool {
        self.running()
            .remove(id)
            .is_some_and(|sender| sender.send(()).is_ok())
    }

    /// The operation ended, cancelled or not: its handle, fired or spent, is
    /// dropped, so a later cancel is a typed refusal.
    fn finish(&self, id: &OperationId) {
        let _ = self.running().remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app::ports::{HookContext, HookOutcome, HookStatus, JobControl};

    fn listed(paths_truncated: bool, tags_truncated: bool) -> SnapshotSummary {
        SnapshotSummary {
            id: domain::SnapshotId::parse(&"ab".repeat(32)).expect("a snapshot ID"),
            time: domain::Timestamp::from_second(1_800_000_000).expect("a time"),
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["demo".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated,
            tags_truncated,
        }
    }

    #[test]
    fn a_listed_snapshot_tells_the_page_what_was_cut_from_it() {
        let dto = snapshot_dto(&listed(true, false)).expect("a valid snapshot");
        assert!(dto.paths_truncated && !dto.tags_truncated);
        let dto = snapshot_dto(&listed(false, true)).expect("a valid snapshot");
        assert!(!dto.paths_truncated && dto.tags_truncated);
        let dto = snapshot_dto(&listed(false, false)).expect("a valid snapshot");
        assert!(!dto.paths_truncated && !dto.tags_truncated);
        assert_eq!(dto.label, "home");
    }

    fn demo_settings(password: SecretRef) -> RepositorySettings {
        RepositorySettings {
            repository: "s3:s3.example.net/demo".to_owned(),
            repo_hot: None,
            password,
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        }
    }

    fn field_warning(field: &str) -> api::WarningDto {
        api::WarningDto {
            code: WarningCode::ConfigInlineSecret,
            args: [("field".to_owned(), MsgArg::Text(field.to_owned()))].into(),
        }
    }

    #[test]
    fn a_listed_repository_with_secrets_in_the_config_carries_a_warning_for_each_without_the_value()
    {
        let (password, key) = ("not-a-real-password", "not-a-real-key");
        let mut settings = demo_settings(SecretRef::Inline(SecretValue::new(password)));
        settings
            .options_cold
            .insert("secret_access_key".to_owned(), key.to_owned());
        let alias = RepositoryAlias::parse("demo").expect("an alias");
        let row = repository_row(&alias, &settings);
        assert_eq!(
            row.warnings,
            [
                field_warning("password"),
                field_warning("options-cold.secret_access_key")
            ]
        );
        let wire = serde_json::to_string(&row).expect("a row is JSON");
        assert!(
            !wire.contains(password) && !wire.contains(key),
            "the row repeats a secret: {wire}"
        );
    }

    #[test]
    fn a_listed_repository_whose_secrets_are_read_from_elsewhere_has_no_warning() {
        let mut settings = demo_settings(SecretRef::Env("DEMO_PASSWORD".to_owned()));
        settings
            .options
            .insert("secret_access_key".to_owned(), "${DEMO_KEY}".to_owned());
        let alias = RepositoryAlias::parse("demo").expect("an alias");
        assert_eq!(repository_row(&alias, &settings).warnings, []);
    }

    /// Empty scratch folders the daemon is pointed at: no configuration has
    /// ever been written to the first.
    fn own_folders(scratch: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let config = scratch.join("config");
        let data = scratch.join("data");
        std::fs::create_dir_all(&config).expect("the config folder");
        std::fs::create_dir_all(&data).expect("the data folder");
        (config, data)
    }

    /// A backup that was running when the daemon that ran it stopped.
    fn running_backup(id: OperationId) -> domain::execution::Operation {
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

    #[tokio::test]
    async fn the_daemon_starts_with_no_configuration() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        // An agent that was never configured has nothing to do, and starts.
        let state = daemon_at(Some(config), Some(data)).await;
        assert!(
            state.is_ok(),
            "an empty configuration serves: {:?}",
            state.err().map(|refused| refused.message)
        );
    }

    #[tokio::test]
    async fn the_daemon_ends_what_a_crash_left_unfinished_before_it_hands_over_its_state() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        let database = data.join("ladderback.db");
        // The log an earlier run left behind: a backup that was running when
        // that run stopped.
        let earlier = SqliteStore::open(&database).await.expect("the log opens");
        let id = OperationId::from_uuid(Uuid::from_u128(1));
        earlier
            .save(&running_backup(id), None)
            .await
            .expect("the running backup is saved");
        earlier.close().await;

        let daemon = daemon_at(Some(config), Some(data))
            .await
            .map_err(|refused| refused.message)
            .expect("the daemon starts");
        drop(daemon);

        let log = SqliteStore::open(&database)
            .await
            .expect("the log opens again");
        let detail = log
            .get(id)
            .await
            .expect("the log reads")
            .expect("the backup is still in the log");
        assert_eq!(
            detail.record.status,
            OperationStatus::Failed {
                reason: FailureReason::DaemonStopped
            }
        );
    }

    #[tokio::test]
    async fn a_log_that_cannot_be_recovered_stops_the_start_and_keeps_what_it_held() {
        use sqlx::{ConnectOptions, Connection};
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        let database = data.join("ladderback.db");
        let id = OperationId::from_uuid(Uuid::from_u128(1));
        let earlier = SqliteStore::open(&database).await.expect("the log opens");
        earlier
            .save(&running_backup(id), None)
            .await
            .expect("the running backup is saved");
        earlier.close().await;
        // A log that refuses every change to its operations, as a failing disk
        // would.
        let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database)
            .connect()
            .await
            .expect("a raw connection to the log");
        sqlx::query(
            "CREATE TRIGGER refuse_changes BEFORE UPDATE ON operations
             BEGIN SELECT RAISE(ABORT, 'refused'); END",
        )
        .execute(&mut raw)
        .await
        .expect("the log is made to refuse changes");
        raw.close().await.expect("the connection closes");

        let refused = daemon_at(Some(config), Some(data))
            .await
            .err()
            .expect("a log that cannot be changed refuses the start");
        assert_eq!(refused.message, msg::CLI_CANNOT_RECOVER_OPERATIONS);

        let log = SqliteStore::open(&database)
            .await
            .expect("the log opens again");
        let detail = log
            .get(id)
            .await
            .expect("the log reads")
            .expect("the backup is still in the log");
        assert_eq!(
            detail.record.status,
            OperationStatus::Running,
            "the log is as it was"
        );
    }

    #[tokio::test]
    async fn a_data_folder_another_process_holds_is_refused_before_its_log_is_opened() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        // Held as another process holds it, before any log exists.
        let held = DataDirLock::acquire(&data).expect("the folder is free");
        let refused = daemon_at(Some(config), Some(data.clone()))
            .await
            .err()
            .expect("a data folder another process holds refuses");
        assert_eq!(refused.message, msg::CLI_DATA_DIR_IN_USE);
        assert!(
            !data.join("ladderback.db").exists(),
            "a refused daemon never opens the log"
        );
        drop(held);
    }

    #[tokio::test]
    async fn a_newer_schema_is_a_typed_refusal() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        std::fs::write(config.join("config.toml"), "schema = 999999\n").expect("the config file");
        let refused = daemon_at(Some(config), Some(data))
            .await
            .err()
            .expect("a newer schema refuses to start");
        assert_eq!(refused.message, msg::CLI_CONFIG_SCHEMA_NEWER);
        assert!(
            refused
                .args
                .iter()
                .any(|(name, value)| *name == "found" && *value == text(999_999u64))
        );
    }

    #[tokio::test]
    async fn a_second_daemon_on_a_data_folder_is_refused_naming_the_first() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        let first = daemon_at(Some(config.clone()), Some(data.clone()))
            .await
            .map_err(|refused| refused.message)
            .expect("the first daemon starts");
        let refused = daemon_at(Some(config), Some(data))
            .await
            .err()
            .expect("a data folder another daemon holds refuses");
        assert_eq!(refused.message, msg::CLI_DATA_DIR_IN_USE);
        assert!(
            refused
                .args
                .iter()
                .any(|(name, value)| *name == "pid" && *value == text(std::process::id())),
            "the refusal names the process that holds the folder"
        );
        drop(first);
    }

    #[tokio::test]
    async fn a_daemon_refused_the_data_folder_has_not_read_the_configuration() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        let first = daemon_at(Some(config.clone()), Some(data.clone()))
            .await
            .map_err(|refused| refused.message)
            .expect("the first daemon starts");
        // A configuration only a newer build reads: a daemon that read it
        // would refuse it for that, and one that migrated an older file would
        // rewrite what the first daemon runs on.
        std::fs::write(config.join("config.toml"), "schema = 999999\n").expect("the config file");
        let refused = daemon_at(Some(config), Some(data))
            .await
            .err()
            .expect("a data folder another daemon holds refuses");
        assert_eq!(refused.message, msg::CLI_DATA_DIR_IN_USE);
        drop(first);
    }

    #[tokio::test]
    async fn a_data_folder_is_free_again_once_its_daemon_is_gone() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        let first = daemon_at(Some(config.clone()), Some(data.clone()))
            .await
            .map_err(|refused| refused.message)
            .expect("the first daemon starts");
        drop(first);
        let second = daemon_at(Some(config), Some(data)).await;
        assert!(
            second.is_ok(),
            "a folder nobody holds is served: {:?}",
            second.err().map(|refused| refused.message)
        );
    }

    #[test]
    fn a_lock_that_cannot_be_taken_is_refused_by_what_is_known_of_it() {
        let folder = std::path::Path::new("/srv/demo/data");
        let named = lock_refused(folder, LockError::InUse { holder: Some(4321) });
        assert_eq!(named.message, msg::CLI_DATA_DIR_IN_USE);
        assert_eq!(
            named.args,
            vec![("path", text("/srv/demo/data")), ("pid", text(4321))]
        );
        let unnamed = lock_refused(folder, LockError::InUse { holder: None });
        assert_eq!(unnamed.message, msg::CLI_DATA_DIR_IN_USE_UNNAMED);
        assert_eq!(unnamed.args, vec![("path", text("/srv/demo/data"))]);
        let unavailable = lock_refused(
            folder,
            LockError::Unavailable {
                detail: "Permission denied (os error 13)".to_owned(),
            },
        );
        assert_eq!(unavailable.message, msg::CLI_CANNOT_LOCK_DATA_DIR);
        assert_eq!(
            unavailable.args,
            vec![
                ("path", text("/srv/demo/data/ladderback.lock")),
                ("detail", text("Permission denied (os error 13)"))
            ]
        );
    }

    #[test]
    fn a_log_that_cannot_be_recovered_is_refused_with_the_stores_own_words() {
        for error in [
            app::ports::StoreError::Unavailable {
                detail: "disk I/O error".to_owned(),
            },
            app::ports::StoreError::Corrupt {
                detail: "operation 5: unknown status `bogus`".to_owned(),
            },
        ] {
            let (app::ports::StoreError::Unavailable { detail }
            | app::ports::StoreError::Corrupt { detail }) = &error;
            let refused =
                recovery_refused(std::path::Path::new("/srv/demo/data/ladderback.db"), &error);
            assert_eq!(refused.message, msg::CLI_CANNOT_RECOVER_OPERATIONS);
            assert_eq!(
                refused.args,
                vec![
                    ("path", text("/srv/demo/data/ladderback.db")),
                    ("detail", text(detail))
                ]
            );
        }
    }

    #[tokio::test]
    async fn a_running_operation_is_cancelled_once() {
        let registry = CancelRegistry::new();
        let id = OperationId::from_uuid(Uuid::new_v4());
        let cancel = registry.register(id);
        assert!(registry.cancel(&id), "the running operation cancels");
        let sleep = tokio::time::sleep(std::time::Duration::from_secs(5));
        tokio::pin!(sleep);
        tokio::select! {
            _ = cancel => {}
            _ = &mut sleep => panic!("cancelling fired the run's cancel future"),
        }
        assert!(!registry.cancel(&id), "one handle, one cancel");
        registry.finish(&id);
        assert!(
            !registry.cancel(&id),
            "a finished operation no longer cancels"
        );
    }

    /// A hook runner that reports a run has entered its before-hooks and
    /// parks there until the test opens the gate.
    struct GateHooks {
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
    }

    /// An engine whose jobs report nothing: the run parks, as a real backup
    /// does while it works, and stopping it is a no-op.
    struct ParkedEngine;

    #[async_trait]
    impl Engine for ParkedEngine {
        async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
            Ok(RunningJob {
                events: futures::stream::pending().boxed(),
                control: Box::new(Parked),
            })
        }
    }

    struct Parked;

    #[async_trait]
    impl JobControl for Parked {
        async fn stop(&mut self) {}
    }

    /// A runner over scratch folders with one plan, "home", whose agent-level
    /// before-hook is the gate; its engine parks. Also what the gate says and
    /// opens, and the scratch folder, which must outlive the runner.
    async fn runner_at_a_gate() -> (
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
                    settings,
                    maintenance: domain::planning::Maintenance::None,
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
            retention: Arc::new(engine::RusticRetention),
            store: Arc::new(
                SqliteStore::open(&data_dir.join("ladderback.db"))
                    .await
                    .expect("the store opens"),
            ),
            registry: Arc::new(CancelRegistry::new()),
            hooks: Arc::new(hooks),
        };
        (runner, entered, open_gate, scratch)
    }

    /// A cancel that arrives the instant the route answers finds the
    /// operation, not an "already ended" refusal (§4.2): the cancel switch
    /// is parked before the run starts, while the run is still in its
    /// before-hooks.
    #[tokio::test]
    async fn a_cancel_in_the_start_window_stops_the_operation() {
        let (runner, mut entered, open_gate, _scratch) = runner_at_a_gate().await;
        let id = runner
            .start_backup("home".into())
            .await
            .expect("the plan starts");
        // The run is in its before-hooks now, parked at the gate and not yet
        // at the engine, so the switch must already be parked for a cancel.
        entered.changed().await.expect("the run reaches its hooks");
        // The cancel is answered while the gate is still closed: a switch
        // parked any later than the route's answer is a refusal here.
        let cancel = runner.cancel(id).await;
        open_gate.send(true).expect("the gate opens");
        cancel.expect("a cancel in the start window is accepted");
    }

    /// A client that cancels the instant it has the answer to its start is
    /// not told the operation is unknown or not running: the operation is
    /// recorded and its cancel switch parked before the route answers, and
    /// the run it cancels ends cancelled.
    #[tokio::test]
    async fn a_cancel_right_after_the_answer_is_accepted_and_ends_the_operation_cancelled() {
        let (runner, _entered, open_gate, _scratch) = runner_at_a_gate().await;
        open_gate.send(true).expect("the gate opens");
        let id = runner
            .start_backup("home".into())
            .await
            .expect("the plan starts");
        runner
            .cancel(id.clone())
            .await
            .expect("a cancel straight after the answer is accepted");
        let operation = operation_of(&id).expect("the answer is an operation ID");
        assert_eq!(ended(&runner, operation).await, OperationStatus::Cancelled);
    }

    /// The status the operation ends with, once it has.
    async fn ended(runner: &PlanRunner, operation: OperationId) -> OperationStatus {
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
            .expect("the cancelled operation ends")
    }

    /// An operation log whose writes wait until the test opens it.
    struct GatedSaves {
        inner: Arc<dyn OperationStore>,
        open: tokio::sync::watch::Receiver<bool>,
    }

    #[async_trait]
    impl OperationStore for GatedSaves {
        async fn fail_unfinished(
            &self,
            reason: &FailureReason,
        ) -> Result<u64, app::ports::StoreError> {
            self.inner.fail_unfinished(reason).await
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
    }

    /// A client that gives up on its start while the log is still being
    /// written (the connection drops, and the handler with it) leaves a run
    /// that goes on: nothing is left queued and never run, or registered and
    /// never cancellable.
    #[tokio::test]
    async fn a_start_whose_client_gave_up_still_runs_and_still_cancels() {
        let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
        open_hooks.send(true).expect("the gate opens");
        let (open_log, log_is_open) = tokio::sync::watch::channel(false);
        runner.store = Arc::new(GatedSaves {
            inner: Arc::clone(&runner.store),
            open: log_is_open,
        });
        let answered = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            runner.start_backup("home".into()),
        )
        .await;
        assert!(answered.is_err(), "the start waits for the log");
        open_log.send(true).expect("the log opens");
        let operation = *runner
            .registry
            .running()
            .keys()
            .next()
            .expect("the start parked a cancel switch");
        let id = operation.as_uuid().hyphenated().to_string();
        let accepted = async {
            loop {
                match runner.cancel(id.clone()).await {
                    Err(http_adapter::CancelError::Unknown) => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    other => return other,
                }
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), accepted)
            .await
            .expect("the operation ends up in the log")
            .expect("the operation that was started is cancellable");
        assert_eq!(ended(&runner, operation).await, OperationStatus::Cancelled);
    }

    /// An engine whose jobs play `events`, whatever the jobs are.
    struct Answers(Vec<EngineEvent>);

    #[async_trait]
    impl Engine for Answers {
        async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
            Ok(RunningJob {
                events: futures::stream::iter(self.0.clone()).boxed(),
                control: Box::new(Parked),
            })
        }
    }

    fn started() -> EngineEvent {
        EngineEvent::Started {
            index_read_at: domain::Timestamp::from_second(1_800_000_000).expect("a time"),
        }
    }

    /// An engine whose jobs start and succeed at once, each with what its
    /// kind reports: the ID of the repository a read opens, the outcome of
    /// a check.
    struct Completes;

    #[async_trait]
    impl Engine for Completes {
        async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            let repository =
                domain::RepositoryId::parse(&"ab".repeat(32)).expect("a repository ID");
            let result = match &job {
                Job::Identify(_) => EngineResult::Repository { id: repository },
                Job::Check(_) => EngineResult::Check(app::ports::CheckSummary {
                    repository,
                    errors: 0,
                    warnings: 0,
                }),
                _ => EngineResult::Fake { steps_completed: 1 },
            };
            Answers(vec![started(), EngineEvent::Succeeded(result)])
                .run(op, job)
                .await
        }
    }

    /// An operation log that takes every write but an operation's end at
    /// once, and holds that one until the test lets it through.
    struct HeldEnd {
        inner: Arc<dyn OperationStore>,
        arrived: tokio::sync::Notify,
        open: tokio::sync::watch::Sender<bool>,
    }

    impl HeldEnd {
        fn over(inner: Arc<dyn OperationStore>) -> Self {
            Self {
                inner,
                arrived: tokio::sync::Notify::new(),
                open: tokio::sync::watch::channel(false).0,
            }
        }

        /// Waits until a run has handed the log the write of its end.
        async fn end_arrived(&self) {
            tokio::time::timeout(std::time::Duration::from_secs(10), self.arrived.notified())
                .await
                .expect("the run reached the write of its end");
        }

        fn let_the_end_through(&self) {
            self.open.send_replace(true);
        }
    }

    #[async_trait]
    impl OperationStore for HeldEnd {
        async fn fail_unfinished(
            &self,
            reason: &FailureReason,
        ) -> Result<u64, app::ports::StoreError> {
            self.inner.fail_unfinished(reason).await
        }

        async fn save(
            &self,
            operation: &domain::execution::Operation,
            result: Option<&app::ports::EngineResult>,
        ) -> Result<(), app::ports::StoreError> {
            if operation.status().is_finished() {
                self.arrived.notify_one();
                let _ = self.open.subscribe().wait_for(|open| *open).await;
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
    }

    /// What a page that listens to the daemon hears.
    type Heard = futures::stream::BoxStream<'static, UiEvent>;

    /// Runs the daemon's engine and bus over `inner`'s jobs and `log` as the
    /// runner's own, and returns what a page that listens to the bus hears.
    async fn listen(
        runner: &mut PlanRunner,
        inner: impl Engine + 'static,
        log: Arc<dyn OperationStore>,
    ) -> Heard {
        let bus = Arc::new(BroadcastEventBus::new());
        let heard = bus.subscribe().await;
        runner.engine = Arc::new(EventEngine {
            inner: Arc::new(inner),
            bus,
            clock: Arc::new(SystemClock),
        });
        runner.store = log;
        heard
    }

    fn is_end(news: &UiEvent) -> bool {
        matches!(
            news,
            UiEvent::OperationSucceeded { .. } | UiEvent::OperationFailed { .. }
        )
    }

    /// How operations have ended, as far as the page has heard, without
    /// waiting for any more.
    fn ends_heard(heard: &mut Heard) -> Vec<UiEvent> {
        let mut ends = Vec::new();
        while let Some(Some(news)) = futures::FutureExt::now_or_never(heard.next()) {
            if is_end(&news) {
                ends.push(news);
            }
        }
        ends
    }

    /// Waits for the page to hear how an operation ended.
    async fn next_end(heard: &mut Heard) -> UiEvent {
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
    async fn history_of(runner: &PlanRunner, id: &str) -> app::ports::OperationDetail {
        GetOperation::new(Arc::clone(&runner.store))
            .get(operation_of(id).expect("an operation ID"))
            .await
            .expect("the log reads")
            .expect("the operation is in the log")
    }

    /// A page that reads the history on hearing a backup ended finds the end
    /// there (#134): the daemon announces it once the log has the write, not
    /// when the engine says it is done.
    #[tokio::test]
    async fn a_backup_is_announced_to_the_pages_once_the_log_has_its_end() {
        let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
        open_hooks.send(true).expect("the gate opens");
        let log = Arc::new(HeldEnd::over(Arc::clone(&runner.store)));
        let mut heard = listen(&mut runner, Completes, log.clone()).await;
        let id = runner
            .start_backup("home".into())
            .await
            .expect("the plan starts");

        log.end_arrived().await;
        assert_eq!(
            ends_heard(&mut heard),
            [],
            "the page heard the end before the log had it"
        );

        log.let_the_end_through();
        let end = next_end(&mut heard).await;
        assert!(
            matches!(&end, UiEvent::OperationSucceeded { operation, .. } if *operation == id),
            "{end:?}"
        );
        assert_eq!(
            history_of(&runner, &id).await.record.status,
            OperationStatus::Succeeded
        );
    }

    /// The same for maintenance: the reads that come before it (the
    /// repository's identity) are not how it ended, so the page hears of one
    /// end, once the log has it.
    #[tokio::test]
    async fn a_check_is_announced_to_the_pages_once_and_once_the_log_has_its_end() {
        let (mut runner, _entered, _open_hooks, _scratch) = runner_at_a_gate().await;
        let log = Arc::new(HeldEnd::over(Arc::clone(&runner.store)));
        let mut heard = listen(&mut runner, Completes, log.clone()).await;
        let id = runner
            .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check)
            .await
            .expect("the check starts");

        log.end_arrived().await;
        assert_eq!(
            ends_heard(&mut heard),
            [],
            "the page heard an end before the log had it"
        );

        log.let_the_end_through();
        let end = next_end(&mut heard).await;
        assert!(
            matches!(&end, UiEvent::OperationSucceeded { operation, .. } if *operation == id),
            "{end:?}"
        );
        assert_eq!(
            history_of(&runner, &id).await.record.status,
            OperationStatus::Succeeded
        );
        assert_eq!(ends_heard(&mut heard), [], "the end was announced twice");
    }

    /// The engine's own end is no news to a page: whoever records the
    /// operation announces it. The runner still sees every event, and the
    /// page still hears how a job started.
    #[tokio::test]
    async fn the_engine_tells_the_pages_how_a_job_started_and_never_how_it_ended() {
        let ends = [
            EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
            EngineEvent::Failed(FailureReason::EngineFailed {
                detail: "repository not found".to_owned(),
            }),
        ];
        for end in ends {
            let bus = Arc::new(BroadcastEventBus::new());
            let mut heard = bus.subscribe().await;
            let engine = EventEngine {
                inner: Arc::new(Answers(vec![started(), end.clone()])),
                bus,
                clock: Arc::new(SystemClock),
            };
            let job = Job::Fake(app::ports::FakeJob {
                steps: 1,
                step_delay: std::time::Duration::ZERO,
                warnings: Vec::new(),
                end: app::ports::FakeEnd::Succeed,
                deadline_budget: None,
            });
            let operation = OperationId::from_uuid(Uuid::from_u128(7));
            let running = engine
                .run(operation, job)
                .await
                .expect("the engine takes the job");
            let reported: Vec<EngineEvent> = running.events.collect().await;
            assert_eq!(reported, [started(), end.clone()], "the runner's events");

            let mut news = Vec::new();
            while let Some(Some(event)) = futures::FutureExt::now_or_never(heard.next()) {
                news.push(event);
            }
            assert_eq!(
                news,
                [UiEvent::OperationStarted {
                    operation: "00000000-0000-0000-0000-000000000007".to_owned(),
                    repository: None,
                    started_at: domain::Timestamp::from_second(1_800_000_000)
                        .expect("a time")
                        .to_string(),
                }],
                "the page's news of {end:?}"
            );
        }
    }

    fn english() -> l10n::Localizer {
        l10n::Localizer::new(&[])
    }

    fn said(refusal: &Refusal) -> String {
        let shown = english().error(refusal.code, &refusal.args, refusal.detail.as_deref());
        assert!(shown.problems().is_empty(), "{:?}", shown.problems());
        shown.into_text()
    }

    fn invalid(key: Option<&str>, at: Option<(usize, usize)>) -> ImportError {
        ImportError::Profile(ProfileError::Invalid {
            file: "nas.toml".to_owned(),
            key: key.map(str::to_owned),
            at: at.map(|(line, column)| app::ports::Position { line, column }),
            problem: app::ports::ProfileProblem::UnknownKey,
            detail: String::new(),
        })
    }

    #[test]
    fn an_invalid_profile_names_its_file_its_key_and_its_line() {
        let refusal = import_refusal(
            invalid(Some("forget.keep-lots"), Some((7, 3))),
            std::path::Path::new("/cfg/config.toml"),
        );
        assert_eq!(refusal.code, ErrorCode::ProfileInvalid);
        let said = said(&refusal);
        for part in ["nas.toml", "forget.keep-lots", "line 7", "UnknownKey"] {
            assert!(said.contains(part), "{part}: {said}");
        }
    }

    #[test]
    fn an_invalid_profile_with_no_place_does_not_say_none() {
        let refusal = import_refusal(invalid(None, None), std::path::Path::new("/cfg/c.toml"));
        assert!(refusal.args.iter().all(|(name, _)| *name == "file"));
        let said = said(&refusal);
        assert!(said.contains("nas.toml"), "{said}");
        assert!(!said.contains("none") && !said.contains("line"), "{said}");
    }

    #[test]
    fn an_unsupported_setting_names_the_key_and_the_line_when_known() {
        let unsupported = |at| {
            ImportError::Profile(ProfileError::Unsupported {
                file: "nas.toml".to_owned(),
                key: "forget.keep-tags".to_owned(),
                at,
            })
        };
        let path = std::path::Path::new("/cfg/c.toml");
        let placed = said(&import_refusal(
            unsupported(Some(app::ports::Position {
                line: 12,
                column: 1,
            })),
            path,
        ));
        assert!(placed.contains("forget.keep-tags") && placed.contains("line 12"));
        let unplaced = said(&import_refusal(unsupported(None), path));
        assert!(unplaced.contains("forget.keep-tags") && !unplaced.contains("line"));
    }

    #[test]
    fn a_profile_that_cannot_be_read_names_the_file_and_the_limit() {
        let path = std::path::Path::new("/cfg/c.toml");
        let unreadable = import_refusal(
            ImportError::Profile(ProfileError::Unreadable {
                file: "missing.toml".to_owned(),
            }),
            path,
        );
        assert_eq!(unreadable.code, ErrorCode::ProfileUnreadable);
        assert!(said(&unreadable).contains("missing.toml"));
        let large = import_refusal(
            ImportError::Profile(ProfileError::TooLarge {
                file: "big.toml".to_owned(),
                limit: 1 << 20,
            }),
            path,
        );
        assert_eq!(large.code, ErrorCode::ProfileTooLarge);
        let said = said(&large);
        assert!(
            said.contains("big.toml") && said.contains("1 MiB"),
            "{said}"
        );
    }

    #[test]
    fn an_export_that_cannot_be_made_says_which_plan_or_which_setting() {
        let path = std::path::Path::new("/cfg/c.toml");
        let unknown = export_refusal(
            ExportError::PlanUnknown(PlanId::parse("ghost").expect("a slug")),
            path,
        );
        assert_eq!(unknown.code, ErrorCode::PlanUnknown);
        assert!(said(&unknown).contains("ghost"));
        let inline = export_refusal(ExportError::Render(ExportRefusal::InlineSecret), path);
        assert_eq!(inline.code, ErrorCode::InlineSecret);
        assert!(said(&inline).contains("password"));
        let reference = export_refusal(
            ExportError::Render(ExportRefusal::Reference {
                key: "repository".to_owned(),
            }),
            path,
        );
        assert_eq!(reference.code, ErrorCode::ProfileExportReference);
        assert!(said(&reference).contains("repository"));
        let url = export_refusal(
            ExportError::Render(ExportRefusal::UrlPassword {
                key: "repo-hot".to_owned(),
            }),
            path,
        );
        assert_eq!(url.code, ErrorCode::ProfileExportUrlPassword);
        assert!(said(&url).contains("repo-hot"));
        let tag = export_refusal(
            ExportError::Render(ExportRefusal::TagComma {
                tag: "a,b".to_owned(),
            }),
            path,
        );
        assert_eq!(tag.code, ErrorCode::ProfileExportTagComma);
        assert!(said(&tag).contains("a,b"));
        let config = export_refusal(
            ExportError::Config(ConfigError::SchemaMissing {
                file: "/cfg/c.toml".to_owned(),
            }),
            path,
        );
        assert_eq!(config.code, ErrorCode::ConfigSchemaMissing);
    }

    #[test]
    fn a_clash_names_the_alias_or_the_plan() {
        let path = std::path::Path::new("/cfg/c.toml");
        let alias = import_refusal(
            ImportError::AliasTaken(RepositoryAlias::parse("nas").expect("a slug")),
            path,
        );
        assert_eq!(alias.code, ErrorCode::RepositoryAliasTaken);
        assert!(said(&alias).contains("nas"));
        let plan = import_refusal(
            ImportError::PlanTaken(PlanId::parse("home").expect("a slug")),
            path,
        );
        assert_eq!(plan.code, ErrorCode::PlanIdTaken);
        assert!(said(&plan).contains("home"));
    }

    #[test]
    fn a_name_that_is_not_an_alias_is_refused_by_its_code_alone() {
        let refusal = repository_alias_invalid();
        assert_eq!(refusal.code, ErrorCode::RepositoryAliasInvalid);
        assert!(refusal.args.is_empty());
        assert_eq!(refusal.detail, None);
        assert!(said(&refusal).contains("lower-case letters"));
    }

    #[test]
    fn no_folder_for_the_configuration_is_refused_by_its_code_alone() {
        let refusal = config_folder_unknown();
        assert_eq!(refusal.code, ErrorCode::ConfigFolderUnknown);
        assert!(refusal.args.is_empty());
        assert_eq!(refusal.detail, None);
        let shown = said(&refusal);
        assert!(shown.contains("LADDERBACK_CONFIG_DIR"), "{shown}");
    }

    #[test]
    fn a_runtime_that_did_not_start_is_refused_with_the_operating_systems_words() {
        let refusal = runtime_unavailable(&std::io::Error::other("Too many open files"));
        assert_eq!(refusal.code, ErrorCode::RuntimeUnavailable);
        assert!(refusal.args.is_empty());
        assert_eq!(refusal.detail.as_deref(), Some("Too many open files"));
        let shown = said(&refusal);
        assert!(shown.contains("Too many open files"), "{shown}");
        assert!(shown.contains("open files and threads"), "{shown}");
    }

    #[test]
    fn a_merged_configuration_that_is_not_valid_names_the_configuration_file() {
        let refusal = import_refusal(
            ImportError::Config(ConfigError::Invalid {
                file: String::new(),
                key: None,
                at: None,
                problem: app::ports::Problem::Violation(domain::planning::Violation::NoSources {
                    index: 0,
                    plan: PlanId::parse("home").expect("a slug"),
                }),
                detail: String::new(),
            }),
            std::path::Path::new("/cfg/config.toml"),
        );
        assert_eq!(refusal.code, ErrorCode::ConfigInvalid);
        let said = said(&refusal);
        assert!(said.contains("/cfg/config.toml"), "{said}");
    }

    #[test]
    fn a_store_that_cannot_be_read_is_the_configuration_codes() {
        let path = std::path::Path::new("/cfg/config.toml");
        let unavailable = import_refusal(
            ImportError::Config(ConfigError::Unavailable {
                file: "/cfg/config.toml".to_owned(),
                detail: "permission denied".to_owned(),
            }),
            path,
        );
        assert_eq!(unavailable.code, ErrorCode::ConfigUnavailable);
        assert!(said(&unavailable).contains("permission denied"));
        let newer = import_refusal(
            ImportError::Config(ConfigError::SchemaNewer {
                file: "/cfg/config.toml".to_owned(),
                found: 9,
                supported: 1,
            }),
            path,
        );
        assert_eq!(newer.code, ErrorCode::ConfigSchemaNewer);
        assert!(said(&newer).contains('9'));
    }

    #[test]
    fn every_notice_is_a_warning_that_renders_what_it_names() {
        let notices = [
            (ProfileNotice::KeyIgnored { key: "copy".into() }, "copy"),
            (ProfileNotice::Prune, "Pruning will run from one place"),
            (ProfileNotice::EnvIgnored, "global.env"),
            (ProfileNotice::HooksPerPlan, "once per plan"),
            (
                ProfileNotice::UsedProfileMissing {
                    name: "ghost".into(),
                },
                "ghost",
            ),
            (
                ProfileNotice::InlineSecret {
                    field: "repository.password".into(),
                },
                "repository.password",
            ),
            (
                ProfileNotice::TagDropped {
                    tag: "ladderback-plan:x".into(),
                },
                "ladderback-plan:x",
            ),
            (
                ProfileNotice::OptionLeftOut {
                    option: "secret-key".into(),
                    variable: "OPENDAL_SECRET_KEY".into(),
                },
                "OPENDAL_SECRET_KEY",
            ),
            (ProfileNotice::RetentionEmpty, "keep-*"),
        ];
        for (notice, expected) in notices {
            let notice = profile_notice(notice);
            let shown = english().warning(notice.code, &notice.args, None);
            assert!(shown.problems().is_empty(), "{:?}", shown.problems());
            assert!(shown.text().contains(expected), "{expected}: {shown}");
        }
    }
}

#[cfg(test)]
mod repository_service_tests {
    use std::sync::{Mutex, PoisonError};

    use api::{ErrorCode, MsgArg, PasswordDto, RepositorySettingsDto};
    use app::fakes::{FakeSecrets, InMemoryConfigStore};
    use app::ports::JobControl;
    use domain::RepositoryId;

    use super::*;

    /// An engine that answers every job with the scripted outcome, and keeps
    /// the jobs it was given.
    struct Scripted {
        outcome: Mutex<Result<RepositoryId, FailureReason>>,
        refuses: Mutex<Option<EngineError>>,
        /// How many of the next jobs never answer.
        hangs: Mutex<usize>,
        seen: Mutex<Vec<Job>>,
    }

    struct Stop;

    #[async_trait]
    impl JobControl for Stop {
        async fn stop(&mut self) {}
    }

    #[async_trait]
    impl Engine for Scripted {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
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
            let event = match self
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
            {
                Ok(id) => EngineEvent::Succeeded(EngineResult::Repository { id }),
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
    struct Yielding(InMemoryConfigStore);

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
    }

    /// A configuration store that can't be read.
    struct Broken(ConfigError);

    #[async_trait]
    impl ConfigStore for Broken {
        async fn load(&self) -> Result<domain::planning::AgentConfig, ConfigError> {
            Err(self.0.clone())
        }

        async fn save(&self, _config: &domain::planning::AgentConfig) -> Result<(), ConfigError> {
            Err(self.0.clone())
        }
    }

    fn repository_id() -> RepositoryId {
        RepositoryId::parse(&"a".repeat(64)).expect("a repository ID")
    }

    fn engine() -> Arc<Scripted> {
        Arc::new(Scripted {
            outcome: Mutex::new(Ok(repository_id())),
            refuses: Mutex::new(None),
            hangs: Mutex::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn service(engine: &Arc<Scripted>, config: Arc<dyn ConfigStore>) -> RepositoryService {
        RepositoryService::new(
            clock(),
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config,
        )
    }

    /// A clock that jumps to the deadline of whoever sleeps on it, so a job
    /// that never answers is out of time the moment it is waited for.
    fn clock() -> Arc<dyn Clock> {
        Arc::new(app::fakes::FakeClock::auto_advancing(
            domain::Timestamp::from_second(1_800_000_000).expect("a timestamp"),
        ))
    }

    fn jobs(engine: &Scripted) -> usize {
        engine
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn settings_dto() -> RepositorySettingsDto {
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

    fn alias(name: &str) -> RepositoryAlias {
        RepositoryAlias::parse(name).expect("an alias")
    }

    fn connect(alias: &str, settings: RepositorySettingsDto) -> api::ConnectRepositoryDto {
        api::ConnectRepositoryDto {
            alias: alias.to_owned(),
            settings,
        }
    }

    fn init(alias: Option<&str>, settings: RepositorySettingsDto) -> api::InitRepositoryDto {
        api::InitRepositoryDto {
            alias: alias.map(str::to_owned),
            settings,
        }
    }

    async fn saved(config: &dyn ConfigStore) -> domain::planning::AgentConfig {
        config.load().await.expect("the configuration loads")
    }

    #[tokio::test]
    async fn a_test_reports_the_id_and_saves_nothing() {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let answer = service(&engine, config.clone())
            .test(api::TestRepositoryDto {
                settings: settings_dto(),
            })
            .await;
        assert_eq!(answer, Ok(api::RepositoryIdDto { id: "a".repeat(64) }));
        assert!(matches!(
            engine.seen.lock().unwrap_or_else(PoisonError::into_inner)[0],
            Job::Identify(_)
        ));
        assert!(saved(&*config).await.repositories().is_empty());
    }

    #[tokio::test]
    async fn a_connection_saves_the_settings_as_written() {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let mut settings = settings_dto();
        settings.repo_hot = Some("/srv/hot".to_owned());
        settings.password = PasswordDto::File {
            path: "/run/secrets/pw".to_owned(),
        };
        settings.options = [("a".to_owned(), "1".to_owned())].into();
        settings.options_hot = [("b".to_owned(), "2".to_owned())].into();
        settings.options_cold = [("c".to_owned(), "3".to_owned())].into();
        let secrets = FakeSecrets::new();
        secrets.write_file("/run/secrets/pw", b"demo-password\n", 0o600);
        let answer = RepositoryService::new(
            clock(),
            engine.clone(),
            ResolveRepository::new(Arc::new(secrets)),
            config.clone(),
        )
        .connect(connect("demo", settings))
        .await;
        assert_eq!(answer.map(|id| id.id), Ok("a".repeat(64)));
        let config = saved(&*config).await;
        let repository = config.repository(&alias("demo")).expect("saved");
        let settings = &repository.settings;
        assert_eq!(settings.repository, "/srv/demo/repo");
        assert_eq!(settings.repo_hot.as_deref(), Some("/srv/hot"));
        // A reference stays a reference: the file is read when a job needs it.
        assert_eq!(
            settings.password,
            domain::secrets::SecretRef::File {
                path: "/run/secrets/pw".to_owned(),
                read: domain::secrets::FileRead::FirstLine,
            }
        );
        assert_eq!(settings.options.get("a").map(String::as_str), Some("1"));
        assert_eq!(settings.options_hot.get("b").map(String::as_str), Some("2"));
        assert_eq!(
            settings.options_cold.get("c").map(String::as_str),
            Some("3")
        );
    }

    #[tokio::test]
    async fn the_other_password_sources_become_their_references() {
        use domain::secrets::{KeyringEntry, SecretRef, SecretValue};
        for (dto, expected) in [
            (
                PasswordDto::Inline {
                    value: "typed".into(),
                },
                SecretRef::Inline(SecretValue::new("typed")),
            ),
            (
                PasswordDto::Keyring {
                    entry: "ladderback/nas".into(),
                },
                SecretRef::Keyring(KeyringEntry::parse("ladderback/nas").expect("an entry")),
            ),
            (
                PasswordDto::Env {
                    name: "DEMO_PASSWORD".into(),
                },
                SecretRef::Env("DEMO_PASSWORD".into()),
            ),
        ] {
            let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
            let mut settings = settings_dto();
            settings.password = dto.clone();
            // Only a connection saves the reference, and testing it first
            // resolves it: the fake knows this variable and this entry.
            let secrets = FakeSecrets::new();
            secrets.set_env("DEMO_PASSWORD", "demo-password");
            secrets.set_keyring_entry(
                &KeyringEntry::parse("ladderback/nas").expect("an entry"),
                "demo-password",
            );
            let service = RepositoryService::new(
                clock(),
                engine.clone(),
                ResolveRepository::new(Arc::new(secrets)),
                config.clone(),
            );
            service
                .connect(connect("demo", settings))
                .await
                .unwrap_or_else(|e| panic!("{dto:?}: {e:?}"));
            let config = saved(&*config).await;
            let kept = &config.repository(&alias("demo")).expect("saved").settings;
            assert_eq!(kept.password, expected, "{dto:?}");
        }
    }

    #[tokio::test]
    async fn a_value_the_settings_cannot_take_is_an_invalid_body_and_nothing_runs() {
        for password in [
            PasswordDto::File {
                path: "password.txt".into(),
            },
            PasswordDto::File {
                path: String::new(),
            },
            PasswordDto::Keyring {
                entry: "no-slash".into(),
            },
            PasswordDto::Keyring {
                entry: "/name".into(),
            },
        ] {
            let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
            let service = service(&engine, config.clone());
            let mut settings = settings_dto();
            settings.password = password.clone();
            assert_eq!(
                service
                    .test(api::TestRepositoryDto {
                        settings: settings.clone()
                    })
                    .await,
                Err(RepositoriesError::InvalidBody),
                "{password:?}"
            );
            assert_eq!(
                service.connect(connect("demo", settings.clone())).await,
                Err(RepositoriesError::InvalidBody),
                "{password:?}"
            );
            assert_eq!(
                service.init(init(None, settings)).await,
                Err(RepositoriesError::InvalidBody),
                "{password:?}"
            );
            assert_eq!(jobs(&engine), 0, "{password:?}");
        }
    }

    #[tokio::test]
    async fn a_name_that_is_not_an_alias_is_refused_before_anything_runs() {
        let long = "a".repeat(65);
        for name in [
            "Not A Name",
            "",
            "-lead",
            "under_score",
            "demo/x",
            long.as_str(),
        ] {
            let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
            let service = service(&engine, config.clone());
            assert_eq!(
                service.connect(connect(name, settings_dto())).await,
                Err(RepositoriesError::AliasInvalid),
                "{name:?}"
            );
            // An initialisation that would be saved is refused before it
            // creates a repository it couldn't then keep.
            assert_eq!(
                service.init(init(Some(name), settings_dto())).await,
                Err(RepositoriesError::AliasInvalid),
                "{name:?}"
            );
            assert_eq!(jobs(&engine), 0, "{name:?} ran a job");
            assert!(saved(&*config).await.repositories().is_empty());
        }
    }

    #[tokio::test]
    async fn an_alias_already_used_is_refused_and_the_first_repository_stays() {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        service
            .connect(connect("demo", settings_dto()))
            .await
            .expect("the first connection");
        let mut other = settings_dto();
        other.repository = "/srv/other".to_owned();
        assert_eq!(
            service.connect(connect("demo", other)).await,
            Err(RepositoriesError::AliasTaken {
                alias: "demo".to_owned()
            })
        );
        let config = saved(&*config).await;
        assert_eq!(config.repositories().len(), 1);
        assert_eq!(
            config
                .repository(&alias("demo"))
                .expect("kept")
                .settings
                .repository,
            "/srv/demo/repo"
        );
    }

    #[tokio::test]
    async fn an_initialisation_is_saved_only_when_it_names_an_alias() {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        let answer = service.init(init(None, settings_dto())).await;
        assert_eq!(
            answer,
            Ok(api::RepositoryInitDto {
                id: "a".repeat(64),
                saved: false
            })
        );
        assert!(matches!(
            engine.seen.lock().unwrap_or_else(PoisonError::into_inner)[0],
            Job::Init(_)
        ));
        assert!(saved(&*config).await.repositories().is_empty());

        let answer = service.init(init(Some("fresh"), settings_dto())).await;
        assert_eq!(
            answer,
            Ok(api::RepositoryInitDto {
                id: "a".repeat(64),
                saved: true
            })
        );
        assert!(saved(&*config).await.repository(&alias("fresh")).is_some());
    }

    #[tokio::test]
    async fn an_empty_password_or_location_is_an_invalid_body_and_nothing_runs() {
        let mut empty_password = settings_dto();
        empty_password.password = PasswordDto::Inline {
            value: String::new(),
        };
        let mut empty_location = settings_dto();
        empty_location.repository = String::new();
        let mut empty_hot = settings_dto();
        empty_hot.repo_hot = Some(String::new());
        for (what, settings) in [
            ("password", empty_password),
            ("location", empty_location),
            ("hot location", empty_hot),
        ] {
            let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
            let service = service(&engine, config.clone());
            assert_eq!(
                service
                    .test(api::TestRepositoryDto {
                        settings: settings.clone()
                    })
                    .await,
                Err(RepositoriesError::InvalidBody),
                "an empty {what}"
            );
            assert_eq!(
                service.connect(connect("demo", settings.clone())).await,
                Err(RepositoriesError::InvalidBody),
                "an empty {what}"
            );
            assert_eq!(
                service.init(init(Some("demo"), settings)).await,
                Err(RepositoriesError::InvalidBody),
                "an empty {what}"
            );
            assert_eq!(jobs(&engine), 0, "an empty {what} ran a job");
            assert!(saved(&*config).await.repositories().is_empty());
        }
    }

    #[tokio::test]
    async fn an_initialisation_under_an_alias_already_used_runs_no_job() {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        service
            .connect(connect("demo", settings_dto()))
            .await
            .expect("the first connection");
        let jobs_before = jobs(&engine);
        let mut other = settings_dto();
        other.repository = "/srv/other".to_owned();
        assert_eq!(
            service.init(init(Some("demo"), other)).await,
            Err(RepositoriesError::AliasTaken {
                alias: "demo".to_owned()
            })
        );
        assert_eq!(jobs(&engine), jobs_before, "a repository was created");
        assert_eq!(saved(&*config).await.repositories().len(), 1);
    }

    #[tokio::test]
    async fn two_connections_made_at_the_same_moment_are_both_kept() {
        let engine = engine();
        let config = Arc::new(Yielding(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        let (first, second) = tokio::join!(
            service.connect(connect("first", settings_dto())),
            service.connect(connect("second", settings_dto())),
        );
        first.expect("the first connection");
        second.expect("the second connection");
        let config = saved(&*config).await;
        assert!(config.repository(&alias("first")).is_some());
        assert!(config.repository(&alias("second")).is_some());
    }

    #[tokio::test]
    async fn a_repository_that_never_answers_is_refused_and_the_lock_is_released() {
        // A job that is waited on for ever is a failure of the test, not a
        // stuck run.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let timed_out = || {
                Err(RepositoriesError::Unavailable {
                    code: ErrorCode::RepositoryTimedOut,
                    args: vec![],
                    detail: None,
                })
            };
            let engine = engine();
            *engine.hangs.lock().unwrap_or_else(PoisonError::into_inner) = 3;
            let config = Arc::new(InMemoryConfigStore::new());
            let service = service(&engine, config.clone());
            assert_eq!(
                service
                    .test(api::TestRepositoryDto {
                        settings: settings_dto()
                    })
                    .await
                    .map(|_| ()),
                timed_out()
            );
            assert_eq!(
                service
                    .connect(connect("first", settings_dto()))
                    .await
                    .map(|_| ()),
                timed_out()
            );
            assert_eq!(
                service
                    .init(init(Some("second"), settings_dto()))
                    .await
                    .map(|_| ()),
                timed_out()
            );
            assert!(saved(&*config).await.repositories().is_empty());
            // The lock was let go each time: the next connection takes it.
            service
                .connect(connect("third", settings_dto()))
                .await
                .expect("a connection after the timeouts");
            assert!(saved(&*config).await.repository(&alias("third")).is_some());
        })
        .await
        .expect("the service waited for ever");
    }

    #[tokio::test]
    async fn a_connection_waiting_for_the_lock_goes_on_when_the_one_holding_it_times_out() {
        // A job that is waited on for ever is a failure of the test, not a
        // stuck run.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let engine = engine();
            // Only the first job to reach the engine hangs.
            *engine.hangs.lock().unwrap_or_else(PoisonError::into_inner) = 1;
            let config = Arc::new(Yielding(InMemoryConfigStore::new()));
            let service = service(&engine, config.clone());
            let (first, second) = tokio::join!(
                service.connect(connect("first", settings_dto())),
                service.connect(connect("second", settings_dto())),
            );
            assert_eq!(
                first.map(|_| ()),
                Err(RepositoriesError::Unavailable {
                    code: ErrorCode::RepositoryTimedOut,
                    args: vec![],
                    detail: None,
                })
            );
            second.expect("the second connection, once the first let go");
            let config = saved(&*config).await;
            assert!(config.repository(&alias("first")).is_none());
            assert!(config.repository(&alias("second")).is_some());
        })
        .await
        .expect("the service waited for ever");
    }

    #[tokio::test]
    async fn a_failed_job_is_refused_by_the_kind_of_its_failure() {
        use FailureReason as F;
        let text = |value: &str| MsgArg::Text(value.to_owned());
        let unavailable = |code, args: Vec<(&'static str, MsgArg)>, detail: Option<&str>| {
            Err(RepositoriesError::Unavailable {
                code,
                args,
                detail: detail.map(str::to_owned),
            })
        };
        for (reason, expected) in [
            (
                F::EngineFailed {
                    detail: "wrong password".into(),
                },
                Err(RepositoriesError::OperationFailed {
                    detail: "wrong password".into(),
                }),
            ),
            (
                F::EngineUnavailable {
                    detail: "no worker".into(),
                },
                unavailable(ErrorCode::EngineUnavailable, vec![], Some("no worker")),
            ),
            (
                F::JobRejected {
                    detail: "refused".into(),
                },
                unavailable(ErrorCode::EngineUnavailable, vec![], Some("refused")),
            ),
            (
                F::EngineExited { exit_code: Some(3) },
                unavailable(
                    ErrorCode::EngineExited,
                    vec![("exit_code", text("3"))],
                    None,
                ),
            ),
            (
                F::EngineExited { exit_code: None },
                unavailable(ErrorCode::EngineExited, vec![], None),
            ),
            (
                F::DeadlinePassed,
                unavailable(ErrorCode::DeadlinePassed, vec![], None),
            ),
            (
                F::EngineStartTimedOut,
                unavailable(ErrorCode::EngineStartTimedOut, vec![], None),
            ),
            (
                F::EngineExitTimedOut,
                unavailable(ErrorCode::EngineExitTimedOut, vec![], None),
            ),
            (
                F::DaemonStopped,
                unavailable(ErrorCode::DaemonStopped, vec![], None),
            ),
            (
                F::RepositoryExists,
                Err(RepositoriesError::Exists {
                    location: "/srv/demo/repo".into(),
                }),
            ),
        ] {
            let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
            *engine
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Err(reason.clone());
            let service = service(&engine, config.clone());
            let tested = service
                .test(api::TestRepositoryDto {
                    settings: settings_dto(),
                })
                .await;
            assert_eq!(
                tested.map(|_| ()),
                expected.clone().map(|_: ()| ()),
                "{reason:?}"
            );
            assert_eq!(
                service
                    .connect(connect("demo", settings_dto()))
                    .await
                    .map(|_| ()),
                expected.map(|_: ()| ()),
                "{reason:?}"
            );
            assert!(
                saved(&*config).await.repositories().is_empty(),
                "{reason:?} saved something"
            );
        }
    }

    #[tokio::test]
    async fn the_engine_not_starting_is_unavailable() {
        for (error, expected) in [
            (
                EngineError::Unavailable {
                    detail: "no such program".into(),
                },
                RepositoriesError::Unavailable {
                    code: ErrorCode::EngineUnavailable,
                    args: vec![],
                    detail: Some("no such program".into()),
                },
            ),
            (
                EngineError::StartTimedOut,
                RepositoriesError::Unavailable {
                    code: ErrorCode::EngineStartTimedOut,
                    args: vec![],
                    detail: None,
                },
            ),
        ] {
            let engine = engine();
            *engine
                .refuses
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(error);
            let answer = service(&engine, Arc::new(InMemoryConfigStore::new()))
                .test(api::TestRepositoryDto {
                    settings: settings_dto(),
                })
                .await;
            assert_eq!(answer, Err(expected));
        }
    }

    #[tokio::test]
    async fn an_existing_repository_is_refused_with_its_location_redacted() {
        let engine = engine();
        *engine
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::RepositoryExists);
        let mut settings = settings_dto();
        settings.repository = "rest:https://backup:hunter2@nas.example/repo".to_owned();
        let Err(RepositoriesError::Exists { location }) =
            service(&engine, Arc::new(InMemoryConfigStore::new()))
                .init(init(None, settings))
                .await
        else {
            panic!("an existing repository is refused as one");
        };
        assert!(!location.contains("hunter2"), "{location}");
        assert!(location.contains("nas.example/repo"), "{location}");
    }

    #[tokio::test]
    async fn a_secret_that_cannot_be_read_names_where_it_is() {
        for (mut settings, field) in [
            (settings_dto(), "password"),
            (settings_dto(), "repository"),
            (settings_dto(), "repo-hot"),
            (settings_dto(), "options.token"),
        ] {
            match field {
                "password" => {
                    settings.password = PasswordDto::Env {
                        name: "NOT_SET_ANYWHERE".into(),
                    }
                }
                "repository" => settings.repository = "${NOT_SET_ANYWHERE}/repo".into(),
                "repo-hot" => settings.repo_hot = Some("${NOT_SET_ANYWHERE}/hot".into()),
                _ => {
                    settings.options =
                        [("token".to_owned(), "${NOT_SET_ANYWHERE}".to_owned())].into()
                }
            }
            let engine = engine();
            let answer = service(&engine, Arc::new(InMemoryConfigStore::new()))
                .test(api::TestRepositoryDto { settings })
                .await;
            assert_eq!(
                answer,
                Err(RepositoriesError::SecretRefused(SecretRefusal {
                    code: ErrorCode::SecretMissing,
                    args: vec![
                        ("field", MsgArg::Text(field.to_owned())),
                        ("source", MsgArg::Text("NOT_SET_ANYWHERE".to_owned())),
                    ],
                    detail: None,
                }))
            );
            assert_eq!(jobs(&engine), 0, "{field}");
        }
    }

    fn env_source(name: &str) -> domain::secrets::SourceName {
        domain::secrets::SourceName::Env {
            name: name.to_owned(),
        }
    }

    fn unresolved(
        field: Field,
        source: domain::secrets::SourceName,
        problem: domain::secrets::Problem,
    ) -> ResolveError {
        ResolveError {
            field,
            cause: Cause::Unresolved(domain::secrets::SecretError { source, problem }),
        }
    }

    /// Every cause a secret or a template can fail by has a code of its own,
    /// with the names the message reads: the setting, and the source.
    #[tokio::test]
    async fn a_secret_that_failed_to_resolve_is_refused_with_the_code_of_its_cause() {
        use domain::secrets::{Problem, SourceName, TemplateError};
        let text = |value: &str| MsgArg::Text(value.to_owned());
        let count = |value: u64| MsgArg::Count(value);
        let settings = settings_of(settings_dto()).expect("settings");
        let file = || SourceName::File {
            path: "/run/secrets/pw".to_owned(),
        };
        let command = || SourceName::Command {
            program: "pass".to_owned(),
        };
        let at = |field: &str, source: &str| vec![("field", text(field)), ("source", text(source))];
        type Case = (
            ResolveError,
            ErrorCode,
            Vec<(&'static str, MsgArg)>,
            Option<&'static str>,
        );
        let cases: Vec<Case> = vec![
            (
                unresolved(Field::Password, env_source("PW"), Problem::Missing),
                ErrorCode::SecretMissing,
                at("password", "PW"),
                None,
            ),
            (
                unresolved(
                    Field::Password,
                    file(),
                    Problem::Unreachable {
                        detail: "permission denied".to_owned(),
                    },
                ),
                ErrorCode::SecretUnreadable,
                at("password", "/run/secrets/pw"),
                Some("permission denied"),
            ),
            (
                unresolved(
                    Field::Option("token".to_owned()),
                    file(),
                    Problem::NotUnicode,
                ),
                ErrorCode::SecretNotText,
                at("options.token", "/run/secrets/pw"),
                None,
            ),
            (
                unresolved(
                    Field::OptionHot("token".to_owned()),
                    file(),
                    Problem::TooLarge { limit: 4096 },
                ),
                ErrorCode::SecretTooLarge,
                vec![
                    ("field", text("options-hot.token")),
                    ("source", text("/run/secrets/pw")),
                    ("limit", MsgArg::Bytes(4096)),
                ],
                None,
            ),
            (
                unresolved(
                    Field::Password,
                    command(),
                    Problem::CommandFailed {
                        status: Some(3),
                        detail: "no such entry".to_owned(),
                    },
                ),
                ErrorCode::SecretCommandFailed,
                at("password", "pass"),
                Some("no such entry"),
            ),
            (
                unresolved(
                    Field::Password,
                    command(),
                    Problem::TimedOut { seconds: 60 },
                ),
                ErrorCode::SecretCommandTimedOut,
                vec![
                    ("field", text("password")),
                    ("source", text("pass")),
                    ("seconds", count(60)),
                ],
                None,
            ),
            (
                ResolveError {
                    field: Field::Repository,
                    cause: Cause::ReferenceInProcessArguments,
                },
                ErrorCode::SecretInProcessArguments,
                vec![("field", text("repository"))],
                None,
            ),
            (
                ResolveError {
                    field: Field::RepoHot,
                    cause: Cause::ExpansionTooLong { limit: 4096 },
                },
                ErrorCode::SecretExpansionTooLong,
                vec![("field", text("repo-hot")), ("limit", MsgArg::Bytes(4096))],
                None,
            ),
            (
                ResolveError {
                    field: Field::OptionCold("token".to_owned()),
                    cause: Cause::Invalid(TemplateError::Unterminated { at: 3 }),
                },
                ErrorCode::TemplateInvalid,
                vec![("field", text("options-cold.token")), ("at", text("3"))],
                None,
            ),
            (
                ResolveError {
                    field: Field::Repository,
                    cause: Cause::Invalid(TemplateError::TooLong { limit: 4096 }),
                },
                ErrorCode::TemplateInvalid,
                vec![("field", text("repository"))],
                None,
            ),
        ];
        for (error, code, args, detail) in cases {
            assert_eq!(
                secret_refusal(&error, &settings).await,
                SecretRefusal {
                    code,
                    args,
                    detail: detail.map(str::to_owned),
                },
                "{error:?}"
            );
        }
    }

    /// What a command said, and what a platform said, is masked of every
    /// secret of the repository before it is an answer: the typed password,
    /// a credential option written inline, and a credential a reference in
    /// the settings resolves to (I4).
    #[tokio::test]
    async fn the_words_of_a_failed_secret_are_masked_of_every_secret_of_the_repository() {
        use domain::secrets::Problem;
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let referenced = scratch.path().join("token");
        std::fs::write(&referenced, "canary-from-a-file\n").expect("the file is written");
        let mut settings = settings_of(settings_dto()).expect("settings");
        settings.options = [
            ("secret_access_key".to_owned(), "canary-inline".to_owned()),
            (
                "application_key".to_owned(),
                format!("${{file:{}}}", referenced.display()),
            ),
        ]
        .into();
        let said = "wrote canary-inline, canary-from-a-file and demo-password to stderr";
        let error = unresolved(
            Field::Password,
            domain::secrets::SourceName::Command {
                program: "pass".to_owned(),
            },
            Problem::CommandFailed {
                status: Some(1),
                detail: said.to_owned(),
            },
        );
        let refusal = secret_refusal(&error, &settings).await;
        let detail = refusal.detail.expect("the command's words are kept");
        assert_eq!(
            detail,
            format!(
                "wrote {0}, {0} and {0} to stderr",
                domain::secrets::REDACTED
            )
        );
        // Not the name of an option or of a program, which are not secrets.
        assert_eq!(refusal.code, ErrorCode::SecretCommandFailed);
    }

    /// Captures what the daemon logs at warning level, for the tests that
    /// look at the log.
    struct Captured;

    static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    impl log::Log for Captured {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            LOGGED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record.args().to_string());
        }
        fn flush(&self) {}
    }

    /// Starts capturing, before the code that logs runs.
    fn capture() {
        let _ = log::set_logger(&Captured);
        log::set_max_level(log::LevelFilter::Warn);
    }

    /// The lines logged so far about the snapshots of `alias`.
    fn logged_about(alias: &str) -> Vec<String> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|line| line.contains(&format!("snapshots of {alias} ")))
            .cloned()
            .collect()
    }

    /// A catalog whose repository `alias` has the password from `command`, a
    /// credential written inline, and one read from a file.
    async fn catalog_with_a_password_command(
        alias: &str,
        command: &str,
        engine: Arc<dyn Engine>,
    ) -> (ConfigCatalog, tempfile::TempDir) {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let referenced = scratch.path().join("token");
        std::fs::write(&referenced, "canary-from-a-file\n").expect("the file is written");
        let config = Arc::new(InMemoryConfigStore::new());
        let settings = RepositorySettings {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: SecretRef::Command(
                domain::secrets::CommandLine::parse(command).expect("a command"),
            ),
            options: [
                ("secret_access_key".to_owned(), "canary-inline".to_owned()),
                (
                    "application_key".to_owned(),
                    format!("${{file:{}}}", referenced.display()),
                ),
            ]
            .into(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        };
        config
            .save(
                &domain::planning::AgentConfig::new(
                    Default::default(),
                    Default::default(),
                    std::collections::BTreeMap::from([(
                        RepositoryAlias::parse(alias).expect("an alias"),
                        domain::planning::RepositoryConfig {
                            settings,
                            maintenance: domain::planning::Maintenance::None,
                            group_by: Default::default(),
                            hooks: Default::default(),
                        },
                    )]),
                    Vec::new(),
                )
                .expect("a valid configuration"),
            )
            .await
            .expect("the configuration saves");
        (ConfigCatalog { config, engine }, scratch)
    }

    /// A listing whose password command fails with the other secrets of the
    /// repository in its stderr is refused with the code of the failure,
    /// and neither the answer nor the log holds one of them (I4).
    #[tokio::test]
    async fn a_listing_whose_password_command_fails_is_refused_with_its_code_and_masks_the_secrets()
    {
        capture();
        let (catalog, _scratch) = catalog_with_a_password_command(
            "failing-pass",
            "sh -c 'echo canary-inline canary-from-a-file >&2; exit 3'",
            engine(),
        )
        .await;
        let refused = catalog
            .snapshots("failing-pass".to_owned())
            .await
            .expect_err("the listing is refused");
        let SnapshotsError::SecretRefused(refusal) = refused else {
            panic!("a secret refusal, not {refused:?}");
        };
        assert_eq!(refusal.code, ErrorCode::SecretCommandFailed);
        let said = format!("{refusal:?}");
        let logged = logged_about("failing-pass").join("\n");
        for line in [&said, &logged] {
            assert!(!line.contains("canary-inline"), "{line}");
            assert!(!line.contains("canary-from-a-file"), "{line}");
        }
        assert!(logged.contains("SecretCommandFailed"), "{logged}");
        assert!(logged.contains(domain::secrets::REDACTED), "{logged}");
    }

    /// An engine that can't be started says what it likes: what reaches the
    /// log of the listing has the secrets of the repository masked, and the
    /// debug text of the error is not logged as it is.
    #[tokio::test]
    async fn the_log_of_a_listing_the_engine_refused_masks_the_secrets_of_the_repository() {
        capture();
        let engine = engine();
        *engine
            .refuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::Unavailable {
            detail: "could not start with canary-inline and canary-from-a-file".to_owned(),
        });
        let (catalog, _scratch) =
            catalog_with_a_password_command("engine-down", "echo not-a-real-password", engine)
                .await;
        let refused = catalog.snapshots("engine-down".to_owned()).await;
        assert_eq!(refused, Err(SnapshotsError::Unavailable));
        let logged = logged_about("engine-down").join("\n");
        assert!(logged.contains(domain::secrets::REDACTED), "{logged}");
        assert!(!logged.contains("canary-inline"), "{logged}");
        assert!(!logged.contains("canary-from-a-file"), "{logged}");
    }

    /// A command's line cut at the cap can end in the beginning of a secret
    /// the mask would not find: the first characters of it are not shown
    /// either (I4).
    #[tokio::test]
    async fn the_words_of_a_failed_command_cut_at_the_cap_show_no_start_of_a_secret() {
        use domain::secrets::Problem;
        let mut settings = settings_of(settings_dto()).expect("settings");
        settings.options = [("secret_access_key".to_owned(), "canary-inline".to_owned())].into();
        let stderr = format!("{}canary-inline", "x".repeat(505));
        let error = unresolved(
            Field::Password,
            domain::secrets::SourceName::Command {
                program: "pass".to_owned(),
            },
            Problem::command_failed(Some(1), stderr.as_bytes()),
        );
        let detail = secret_refusal(&error, &settings)
            .await
            .detail
            .expect("the words are kept");
        assert!(detail.ends_with('…'), "{detail}");
        assert!(!detail.contains("canary"), "{detail}");
    }

    /// A file or a keyring that doesn't answer in time is a source that
    /// can't be read, not a command that ran for too long.
    #[tokio::test]
    async fn a_file_or_keyring_that_does_not_answer_in_time_is_unreadable_not_a_slow_command() {
        use domain::secrets::{Problem, SourceName};
        let settings = settings_of(settings_dto()).expect("settings");
        for (source, field) in [
            (
                SourceName::File {
                    path: "/mnt/nfs/pw".to_owned(),
                },
                "/mnt/nfs/pw",
            ),
            (
                SourceName::Keyring {
                    entry: "ladderback/demo".to_owned(),
                },
                "ladderback/demo",
            ),
        ] {
            let error = unresolved(Field::Password, source, Problem::TimedOut { seconds: 60 });
            let refusal = secret_refusal(&error, &settings).await;
            assert_eq!(refusal.code, ErrorCode::SecretUnreadable, "{field}");
            assert_eq!(
                refusal.args,
                vec![
                    ("field", MsgArg::Text("password".to_owned())),
                    ("source", MsgArg::Text(field.to_owned())),
                ]
            );
            assert_eq!(refusal.detail, None);
        }
    }

    #[tokio::test]
    async fn a_configuration_that_cannot_be_used_is_unavailable_with_its_code() {
        let text = |value: &str| MsgArg::Text(value.to_owned());
        for (error, code, args, detail) in [
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
        ] {
            let engine = engine();
            let service = service(&engine, Arc::new(Broken(error.clone())));
            let expected = Err(RepositoriesError::Unavailable {
                code,
                args,
                detail: detail.map(str::to_owned),
            });
            assert_eq!(
                service
                    .connect(connect("demo", settings_dto()))
                    .await
                    .map(|_| ()),
                expected.clone().map(|_: ()| ()),
                "{error:?}"
            );
            assert_eq!(
                service
                    .init(init(Some("demo"), settings_dto()))
                    .await
                    .map(|_| ()),
                expected.map(|_: ()| ()),
                "{error:?}"
            );
        }
    }

    #[tokio::test]
    async fn the_list_shows_where_each_password_comes_from_and_never_the_password() {
        use domain::secrets::{CommandLine, KeyringEntry};
        let config = Arc::new(InMemoryConfigStore::new());
        let sources = [
            (
                "inline",
                SecretRef::Inline(SecretValue::new("typed-secret")),
                api::PasswordSourceDto::Inline,
            ),
            (
                "file",
                SecretRef::File {
                    path: "/run/pw".to_owned(),
                    read: FileRead::FirstLine,
                },
                api::PasswordSourceDto::File {
                    path: "/run/pw".to_owned(),
                },
            ),
            (
                "keyring",
                SecretRef::Keyring(KeyringEntry::parse("ladderback/nas").expect("an entry")),
                api::PasswordSourceDto::Keyring {
                    entry: "ladderback/nas".to_owned(),
                },
            ),
            (
                "env",
                SecretRef::Env("PW".to_owned()),
                api::PasswordSourceDto::Env {
                    name: "PW".to_owned(),
                },
            ),
            (
                "command",
                SecretRef::Command(
                    CommandLine::parse("pass show --secret=hunter2 nas").expect("a command"),
                ),
                api::PasswordSourceDto::Command {
                    program: "pass".to_owned(),
                },
            ),
        ];
        let mut repositories = std::collections::BTreeMap::new();
        for (name, password, _) in &sources {
            repositories.insert(
                alias(name),
                domain::planning::RepositoryConfig {
                    settings: RepositorySettings {
                        repository: "rest:https://u:hunter2@nas.example/repo".to_owned(),
                        repo_hot: Some("rest:https://u:hunter3@hot.example/repo".to_owned()),
                        password: password.clone(),
                        options: Default::default(),
                        options_hot: Default::default(),
                        options_cold: Default::default(),
                    },
                    maintenance: Default::default(),
                    group_by: Default::default(),
                    hooks: Default::default(),
                },
            );
        }
        config
            .save(
                &domain::planning::AgentConfig::new(
                    Default::default(),
                    Default::default(),
                    repositories,
                    Vec::new(),
                )
                .expect("a valid configuration"),
            )
            .await
            .expect("the configuration saves");
        let catalog = ConfigCatalog {
            config,
            engine: engine(),
        };
        let listed = catalog.repositories().await.expect("the list");
        // In the order of the aliases, one row each.
        let mut expected: Vec<_> = sources
            .iter()
            .map(|(name, _, shown)| (*name, shown))
            .collect();
        expected.sort_by_key(|(name, _)| *name);
        assert_eq!(listed.repositories.len(), expected.len());
        for (row, (name, shown)) in listed.repositories.iter().zip(expected) {
            assert_eq!(row.alias, name);
            assert_eq!(&row.password, shown, "{name}");
        }
        let wire = serde_json::to_string(&listed).expect("the list serialises");
        for secret in ["typed-secret", "hunter2", "hunter3"] {
            assert!(!wire.contains(secret), "{secret} in {wire}");
        }
        let first = &listed.repositories[0];
        assert!(first.repository.contains("nas.example/repo"), "{first:?}");
        assert!(
            first
                .repo_hot
                .as_deref()
                .is_some_and(|hot| hot.contains("hot.example/repo")),
            "{first:?}"
        );
    }

    #[test]
    fn a_location_is_shown_without_the_password_in_it() {
        let mut settings = domain::planning::RepositorySettings {
            repository: "rest:https://backup:hunter2@nas.example/repo".to_owned(),
            repo_hot: Some("rest:https://backup:hunter3@hot.example/repo".to_owned()),
            password: domain::secrets::SecretRef::Inline(domain::secrets::SecretValue::new(
                "typed",
            )),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        };
        let shown = redacted_location(&settings, &settings.repository);
        assert!(
            !shown.contains("hunter2") && shown.contains("nas.example"),
            "{shown}"
        );
        let hot = settings.repo_hot.clone().expect("a hot half");
        let shown = redacted_location(&settings, &hot);
        assert!(
            !shown.contains("hunter3") && shown.contains("hot.example"),
            "{shown}"
        );
        // The typed password is not a needle: a location that happens to
        // hold the same text is still a location.
        settings.repository = "/srv/typed/repo".to_owned();
        assert_eq!(
            redacted_location(&settings, &settings.repository),
            "/srv/typed/repo"
        );
    }
}
