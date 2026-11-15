//! In-memory port fakes for use-case tests and acceptance scenarios. Each
//! one passes the same contract suite (`crate::contract`) as its real adapter.

mod admin_password;
mod api_token_store;
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
mod session_store;
mod sources;
mod stats_store;

pub use admin_password::InMemoryAdminPassword;
pub use api_token_store::InMemoryApiTokenStore;
pub use clock::FakeClock;
pub use config_store::InMemoryConfigStore;
pub use engine::SimulatedEngine;
pub use event_bus::InMemoryEventBus;
pub use hooks::{FakeHookRunner, HookCall, Scripted};
pub use notifier::RecordingNotifier;
pub use operation_store::InMemoryOperationStore;
pub use platform::{FakeNetwork, FakeNetworkLink, FakePower};
pub use profiles::{FakeProfileReader, FakeProfileWriter};
pub use random::SeededRandom;
pub use retention::FakeRetention;
pub use schedule_store::InMemoryScheduleStore;
pub use secrets::{FakeSecrets, Script};
pub use service::FakeServiceManager;
pub use session_store::InMemorySessionStore;
pub use sources::FakeSources;
pub use stats_store::InMemoryStatsStore;
