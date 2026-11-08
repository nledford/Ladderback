//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod clock;
mod engine;

pub use clock::Clock;
pub use engine::{
    BackupJob, BackupSummary, CheckJob, CheckSummary, Engine, EngineError, EngineEvent,
    EngineResult, FakeEnd, FakeJob, Job, JobControl, ProgressEvent, ProgressKind, PruneJob,
    PruneSummary, RepositoryAccess, RunningJob,
};
