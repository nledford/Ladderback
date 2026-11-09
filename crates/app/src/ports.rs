//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod clock;
mod config_store;
mod engine;
mod operation_store;
mod secrets;

pub use clock::Clock;
pub use config_store::{ConfigError, ConfigStore, PasswordProblem, Position, Problem};
pub use engine::{
    BackupJob, BackupSummary, CheckJob, CheckSummary, Engine, EngineError, EngineEvent,
    EngineResult, FakeEnd, FakeJob, Job, JobControl, ProgressEvent, ProgressKind, PruneJob,
    PruneSummary, RepositoryAccess, RunningJob,
};
pub use operation_store::{
    DEFAULT_PAGE_SIZE, HistoryPage, HistoryQuery, MAX_PAGE_SIZE, OperationDetail, OperationRecord,
    OperationStore, PageSize, Start, StoreError, maintained_repository,
};
pub use secrets::{Resolved, SecretResolver};
