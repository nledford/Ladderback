//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod clock;
mod config_store;
mod engine;
mod event_bus;
mod hooks;
mod operation_store;
mod profiles;
mod retention;
mod secrets;
mod sources;

pub use clock::Clock;
pub use config_store::{
    ConfigError, ConfigStore, NotInPlace, PasswordProblem, Position, Problem, RecordError,
};
pub use engine::{
    BackupJob, BackupSummary, CheckJob, CheckSummary, Engine, EngineError, EngineEvent,
    EngineResult, FakeEnd, FakeJob, ForgetJob, ForgetSummary, Job, JobControl, MAX_JOB_IDS,
    ProgressEvent, ProgressKind, PruneJob, PrunePlanSummary, PruneSummary, RepositoryAccess,
    RewriteJob, RewriteSummary, RunningJob, SavedEntry, SnapshotSummary,
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
pub use profiles::{
    ExportRefusal, ImportedProfile, PlanExport, ProfileError, ProfileNotice, ProfileProblem,
    ProfileReader, ProfileWriter, RenderedProfile,
};
pub use retention::{Retention, RetentionError};
pub use secrets::{Resolved, SecretResolver};
pub use sources::{NormalizeSources, SourcesError};
