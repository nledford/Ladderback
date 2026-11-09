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
use app::ports::{
    BackupJob, Clock, ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EventBus, Job,
    OperationStore, ProgressEvent, RunningJob,
};
use app::secrets::ResolveRepository;
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::execution::FailureReason;
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{Trigger, WarningKind};
use domain::{AgentId, OperationId, PlanId};
use engine::WorkerEngine;
use futures::stream::StreamExt;
use http_adapter::{AppState, Operations, StartError};
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
    let config = Arc::new(FileConfigStore::new(
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
    let operations: Arc<dyn Operations> = Arc::new(PlanRunner {
        config,
        resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
        agent_id,
        engine,
        store,
    });
    Ok(AppState {
        events: bus,
        operations,
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
/// (§4.1). Hooks are not wired around API-started runs yet; they arrive with
/// the scheduler (E2).
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
        let operation = OperationId::from_uuid(Uuid::new_v4());
        let engine = Arc::clone(&self.engine);
        let store = Arc::clone(&self.store);
        let id_text = operation.as_uuid().hyphenated().to_string();
        tokio::spawn(async move {
            // Nothing waits on the run: it reports through the event bus.
            // Hooks are not part of an API-started run yet (E2 wires them).
            let finished = RunOperation::new(engine, Arc::new(SystemClock), store)
                .run(operation, Trigger::Manual, job, futures::future::pending())
                .await;
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
        todo!("E2: list the operation log, newest first, keyset by `after`")
    }

    async fn operation(
        &self,
        id: String,
    ) -> Result<api::OperationDetailDto, http_adapter::LookupError> {
        todo!("E2: read one operation from the log")
    }

    async fn cancel(&self, id: String) -> Result<(), http_adapter::CancelError> {
        todo!("E2: cancel through the registry, or refuse")
    }
}

/// The running operations' cancel switches (§4.2): what the API cancels
/// through. A handle is registered when an operation starts running and
/// dropped when it ends, so only a running operation can be cancelled, once.
struct CancelRegistry {
    running: std::sync::Mutex<HashMap<OperationId, futures::channel::oneshot::Sender<()>>>,
}

impl CancelRegistry {
    fn new() -> Self {
        Self {
            running: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Registers `id` as running, returning the future its run ends on when
    /// the API cancels it.
    #[allow(clippy::todo)] // commit 1 of the operation lifecycle: red first
    fn register(&self, id: OperationId) -> futures::channel::oneshot::Receiver<()> {
        todo!("E2: park the sender and hand out its receiver")
    }

    /// Cancels `id`: `true` when it was running and is now cancelled, `false`
    /// when nothing by that ID is running, so there is nothing to cancel.
    /// A second cancel of the same operation is `false`: the one handle is
    /// spent.
    #[allow(clippy::todo)] // commit 1 of the operation lifecycle: red first
    fn cancel(&self, id: &OperationId) -> bool {
        todo!("E2: fire the operation's one cancel handle, if it has one")
    }

    /// The operation ended, cancelled or not: its handle, fired or spent, is
    /// dropped, so a later cancel is a typed refusal.
    #[allow(clippy::todo)] // commit 1 of the operation lifecycle: red first
    fn finish(&self, id: &OperationId) {
        todo!("E2: forget the ended operation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
