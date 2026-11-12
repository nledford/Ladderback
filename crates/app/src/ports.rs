//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod admin_password;
mod clock;
mod config_store;
mod engine;
mod event_bus;
mod hooks;
mod notifier;
mod operation_store;
mod platform;
mod profiles;
mod random;
mod retention;
mod schedule_store;
mod secrets;
mod service;
mod sources;

pub use admin_password::{AdminPassword, AdminPasswordError};
pub use clock::Clock;
pub use config_store::{
    CadenceProblem, ConfigError, ConfigStore, NotInPlace, PasswordProblem, Position, Problem,
    RecordError,
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
pub use notifier::{
    Credentials, DELIVERY_TIMEOUT, Destination, Message, Notification, Notifier, NotifyError,
};
pub use operation_store::{
    DEFAULT_PAGE_SIZE, HistoryPage, HistoryQuery, MAX_PAGE_SIZE, OperationDetail, OperationRecord,
    OperationStore, PageSize, Recovered, Start, StoreError, maintained_repository,
};
pub use platform::{Network, PlatformError, PowerSupply};
pub use profiles::{
    ExportRefusal, ImportedProfile, PlanExport, ProfileError, ProfileNotice, ProfileProblem,
    ProfileReader, ProfileWriter, RenderedProfile,
};
pub use random::{Random, RandomUnavailable};
pub use retention::{Retention, RetentionError};
pub use schedule_store::{PendingRetry, PlannedRun, ScheduleStore};
pub use secrets::{Resolved, SecretResolver};
pub use service::{Installed, Removed, ServiceError, ServiceManager, ServiceProblem};
pub use sources::{NormalizeSources, SourcesError};
