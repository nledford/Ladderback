//! In-memory port fakes for use-case tests and acceptance scenarios. Each
//! one passes the same contract suite (`crate::contract`) as its real adapter.

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

pub use clock::FakeClock;
pub use config_store::InMemoryConfigStore;
pub use engine::SimulatedEngine;
pub use event_bus::InMemoryEventBus;
pub use hooks::{FakeHookRunner, HookCall, Scripted};
pub use notifier::RecordingNotifier;
pub use operation_store::InMemoryOperationStore;
pub use platform::{FakeNetwork, FakePower};
pub use profiles::{FakeProfileReader, FakeProfileWriter};
pub use random::SeededRandom;
pub use retention::FakeRetention;
pub use schedule_store::InMemoryScheduleStore;
pub use secrets::{FakeSecrets, Script};
pub use service::FakeServiceManager;
pub use sources::FakeSources;
