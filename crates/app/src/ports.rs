//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod clock;
mod engine;

pub use clock::Clock;
pub use engine::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, ProgressEvent,
    ProgressKind,
};
