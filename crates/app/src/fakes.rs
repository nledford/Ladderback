//! In-memory port fakes for use-case tests and acceptance scenarios. Each
//! one passes the same contract suite (`crate::contract`) as its real adapter.

mod clock;
mod config_store;
mod engine;
mod event_bus;
mod hooks;
mod operation_store;
mod retention;
mod secrets;
mod sources;

pub use clock::FakeClock;
pub use config_store::InMemoryConfigStore;
pub use engine::SimulatedEngine;
pub use event_bus::InMemoryEventBus;
pub use hooks::{FakeHookRunner, HookCall, Scripted};
pub use operation_store::InMemoryOperationStore;
pub use retention::FakeRetention;
pub use secrets::{FakeSecrets, Script};
pub use sources::FakeSources;
