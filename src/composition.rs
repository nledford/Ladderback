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
use app::maintenance::{Adoption, AdoptionSource, MaintenanceKind, RunMaintenance};
use app::ports::Retention;
use app::ports::{
    BackupJob, Clock, ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult,
    EventBus, HookRunner, HookSetup, Job, OperationRecord, OperationStore, ProgressEvent,
    RunningJob, SnapshotSummary,
};
use app::secrets::ResolveRepository;
use app::snapshots::ListSnapshots;
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::RepositoryAlias;
use domain::execution::FailureReason;
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{OperationKind, OperationStatus, Trigger, WarningKind};
use domain::{AgentId, OperationId, PlanId};
use engine::WorkerEngine;
use futures::stream::StreamExt;
use hooks::ProcessHookRunner;
use http_adapter::{
    AppState, Catalog, CatalogError, Operations, Repositories, RepositoriesError, SnapshotsError,
    StartError,
};
use l10n::{MessageId, msg};
use platform::SystemClock;
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

/// What the daemon's router runs against (§4.2), or why it cannot.
pub(crate) async fn app_state() -> Result<AppState, Refused> {
    app_state_at(dirs::config_dir(), dirs::data_dir()).await
}

/// [`app_state`] with the folders supplied, for tests.
async fn app_state_at(
    config_dir: Option<std::path::PathBuf>,
    data_dir: Option<std::path::PathBuf>,
) -> Result<AppState, Refused> {
    // The configuration first: a file this build cannot read is a refusal,
    // whatever the rest of the machine says.
    let config_dir = config_dir.ok_or_else(|| plain(msg::CLI_NO_CONFIG_DIR))?;
    let config: Arc<dyn ConfigStore> = Arc::new(FileConfigStore::new(
        config_dir.join("config.toml"),
        Arc::new(SystemClock),
    ));
    config.load().await.map_err(config_refused)?;
    // The data dir must be there before the store opens in it.
    let data_dir = data_dir.ok_or_else(|| plain(msg::CLI_NO_DATA_DIR))?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|detail| store_refused(&data_dir, &detail.to_string()))?;
    let database = data_dir.join("ladderback.db");
    let store: Arc<dyn OperationStore> = Arc::new(
        SqliteStore::open(&database)
            .await
            .map_err(|detail| store_refused(&database, &detail.to_string()))?,
    );
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
        Arc::clone(&engine),
        resolve.as_ref().clone(),
        Arc::clone(&config),
    ));
    let catalog: Arc<dyn Catalog> = Arc::new(ConfigCatalog { config, engine });
    Ok(AppState {
        events: bus,
        operations,
        catalog,
        repositories,
    })
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
                .keys()
                .map(|alias| alias.as_str().to_owned())
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
        let listed = ListSnapshots::new(
            Arc::clone(&self.engine),
            ResolveRepository::new(Arc::new(SystemSecrets::default())),
        )
        .list(OperationId::from_uuid(Uuid::now_v7()), &repository.settings)
        .await
        .map_err(|error| {
            log::warn!(
                "the snapshots of {} could not be listed: {error:?}",
                alias.as_str()
            );
            SnapshotsError::Unavailable
        })?;
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
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
    config: Arc<dyn ConfigStore>,
    /// Held across a use case's read-modify-write of the configuration.
    writes: tokio::sync::Mutex<()>,
}

impl RepositoryService {
    fn new(
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
    ) -> Self {
        Self {
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
        _request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        let _ = (&self.engine, &self.resolve, &self.config, &self.writes);
        todo!("the red commit: tests first")
    }

    async fn connect(
        &self,
        _request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        todo!("the red commit: tests first")
    }

    async fn init(
        &self,
        _request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, RepositoriesError> {
        todo!("the red commit: tests first")
    }
}

/// A location of `settings`, as it may be shown: with the password a URL
/// holds masked.
fn redacted_location(_settings: &domain::planning::RepositorySettings, _text: &str) -> String {
    todo!("the red commit: tests first")
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

    /// Empty scratch folders the daemon is pointed at: no configuration has
    /// ever been written to the first.
    fn own_folders(scratch: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let config = scratch.join("config");
        let data = scratch.join("data");
        std::fs::create_dir_all(&config).expect("the config folder");
        std::fs::create_dir_all(&data).expect("the data folder");
        (config, data)
    }

    #[tokio::test]
    async fn the_daemon_starts_with_no_configuration() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        // An agent that was never configured has nothing to do, and starts.
        let state = app_state_at(Some(config), Some(data)).await;
        assert!(
            state.is_ok(),
            "an empty configuration serves: {:?}",
            state.err().map(|refused| refused.message)
        );
    }

    #[tokio::test]
    async fn a_newer_schema_is_a_typed_refusal() {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let (config, data) = own_folders(scratch.path());
        std::fs::write(config.join("config.toml"), "schema = 999999\n").expect("the config file");
        let refused = app_state_at(Some(config), Some(data))
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
            seen: Mutex::new(Vec::new()),
        })
    }

    fn service(engine: &Arc<Scripted>, config: Arc<dyn ConfigStore>) -> RepositoryService {
        RepositoryService::new(
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config,
        )
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
        let answer = service(&engine, config.clone())
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
                Err(RepositoriesError::SecretUnresolved {
                    field: field.to_owned()
                })
            );
            assert_eq!(jobs(&engine), 0, "{field}");
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
