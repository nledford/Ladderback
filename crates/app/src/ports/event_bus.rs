//! The port the daemon's job runner tells the UI what happened through
//! (technical design §4.2, §4.4).
//!
//! A use case publishes one [`UiEvent`] per thing a page could see: an
//! operation starting, moving forward, warning, ending; an agent coming or
//! going. The daemon subscribes once per events-WebSocket client and forwards
//! what it reads unchanged — coalescing is the worker's, once (§4.1).

use api::UiEvent;
use async_trait::async_trait;
use futures::stream::BoxStream;

/// How many events each subscriber's buffer holds. A subscriber that reads
/// this far behind loses the oldest [`UiEvent::OperationProgress`] events
/// first; the state changes (started, warning, succeeded, failed, agent
/// online/offline) are kept, so a lagging page's picture of *what happened*
/// stays whole even when its picture of *how far along* has holes.
pub const EVENT_BUFFER: usize = 256;

/// Broadcasts live updates to any number of subscribers.
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Hands `event` to every current subscriber.
    async fn publish(&self, event: UiEvent);

    /// The events published from now on, in order, ending only when the bus
    /// is dropped. [`EVENT_BUFFER`] events can wait per subscriber.
    async fn subscribe(&self) -> BoxStream<UiEvent>;
}
