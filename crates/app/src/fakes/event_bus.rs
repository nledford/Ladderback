//! The in-memory event bus. This is commit 1 of the M1b event stream: the
//! type and constructor are real so tests can wire it, and the port methods
//! are `todo!()` stubs until the implementation commits (development.md §13.1).

use api::UiEvent;
use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::ports::EventBus;

/// A bus that holds each subscriber up to [`crate::ports::EVENT_BUFFER`]
/// events, dropping the oldest progress first, exactly as the contract
/// suite describes.
#[derive(Debug, Default)]
pub struct InMemoryEventBus;

impl InMemoryEventBus {
    pub fn new() -> Self {
        Self
    }
}

#[allow(clippy::todo)] // todo!() stubs are commit-1 only (development.md §13.1)
#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, _event: UiEvent) {
        todo!()
    }

    async fn subscribe(&self) -> BoxStream<UiEvent> {
        todo!()
    }
}
