//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod clock;
mod config_store;
mod engine;
mod event_bus;
mod hooks;
mod operation_store;
mod secrets;

pub use clock::Clock;
pub use config_store::{ConfigError, ConfigStore, PasswordProblem, Position, Problem};
pub use engine::{
    BackupJob, BackupSummary, CheckJob, CheckSummary, Engine, EngineError, EngineEvent,
    EngineResult, FakeEnd, FakeJob, Job, JobControl, ProgressEvent, ProgressKind, PruneJob,
    PruneSummary, RepositoryAccess, RunningJob, SnapshotSummary,
};
pub use event_bus::{EVENT_BUFFER, EventBus};
pub use hooks::{
    HOOK_TIMEOUT, HookContext, HookOutcome, HookOutput, HookPhase, HookRunner, HookSetup,
    HookStatus, MAX_HOOK_OUTPUT_BYTES,
};
pub use operation_store::{
    DEFAULT_PAGE_SIZE, HistoryPage, HistoryQuery, MAX_PAGE_SIZE, OperationDetail, OperationRecord,
    OperationStore, PageSize, Start, StoreError, maintained_repository,
};
pub use secrets::{Resolved, SecretResolver};
