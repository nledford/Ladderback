//! In-memory port fakes for use-case tests and acceptance scenarios. Each
//! one passes the same contract suite (`crate::contract`) as its real adapter.

mod clock;
mod engine;

pub use clock::FakeClock;
pub use engine::SimulatedEngine;
