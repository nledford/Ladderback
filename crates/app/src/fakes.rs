//! In-memory port fakes for use-case tests and acceptance scenarios. Each
//! one passes the same contract suite (`crate::contract`) as its real adapter.

mod clock;
mod engine;
mod operation_store;
mod secrets;

pub use clock::FakeClock;
pub use engine::SimulatedEngine;
pub use operation_store::InMemoryOperationStore;
pub use secrets::{FakeSecrets, Script};
