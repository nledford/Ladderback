//! The in-memory event bus for tests: the daemon's real
//! [`BroadcastEventBus`](crate::event_bus::BroadcastEventBus), which meets the
//! [`EventBus`](crate::ports::EventBus) contract and the port's drop rule, so
//! the suite covers both.

pub use crate::event_bus::BroadcastEventBus as InMemoryEventBus;
