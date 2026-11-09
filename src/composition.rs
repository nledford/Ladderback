//! The daemon's wiring (technical design §3.2, §4.2): the one place beside
//! `main` that knows every adapter. `serve` opens the store and the config,
//! builds the engine, secrets and the event bus, and hands the result to the
//! HTTP router. Adapters stay constructor-injected; nothing else sees them.
//!
//! This is commit 1 of the M1b event stream: the state the router needs is
//! wired with a stub bus so the daemon starts, and the real wiring (store,
//! config, engine, secrets, broadcast) lands in the implementation commits
//! (development.md §13.1).

use std::sync::Arc;

use api::UiEvent;
use app::ports::EventBus;
use async_trait::async_trait;
use futures::stream::BoxStream;

/// The bus until the real wiring lands: every method is a `todo!()`.
#[allow(clippy::todo)] // todo!() stubs are commit-1 only (development.md §13.1)
struct StubBus;

#[allow(clippy::todo)] // todo!() stubs are commit-1 only (development.md §13.1)
#[async_trait]
impl EventBus for StubBus {
    async fn publish(&self, _event: UiEvent) {
        todo!()
    }

    async fn subscribe(&self) -> BoxStream<UiEvent> {
        todo!()
    }
}

/// What the daemon's router runs against.
pub(crate) fn app_state() -> http_adapter::AppState {
    http_adapter::AppState {
        events: Arc::new(StubBus),
    }
}
