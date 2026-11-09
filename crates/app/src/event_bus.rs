//! The daemon's broadcast event bus: the real [`EventBus`] behind the port
//! (technical design §4.2, §4.4). Each subscriber holds its own queue of at
//! most [`EVENT_BUFFER`] events; a queue that fills loses the oldest progress
//! events first, so a lagging page's picture of *what happened* stays whole
//! even when its picture of *how far along* has holes.
//!
//! Publishing is synchronous: once `publish` has returned, every event it
//! handed out is already readable, so a subscriber never waits for the bus to
//! settle (the port contract checks this).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::task::Poll;

use api::UiEvent;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};

use crate::ports::{EVENT_BUFFER, EventBus};

/// Broadcasts live updates to any number of subscribers.
///
/// The bus never blocks a run: publishing pushes into every subscriber's
/// queue and wakes it, and a subscriber that stopped reading loses progress
/// events rather than holding the publisher up.
#[derive(Debug, Default)]
pub struct BroadcastEventBus {
    /// One queue per live subscriber; a subscriber that is dropped takes its
    /// queue with it, and publishing forgets it.
    subscribers: Mutex<Vec<Weak<Queue>>>,
}

impl BroadcastEventBus {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventBus for BroadcastEventBus {
    async fn publish(&self, event: UiEvent) {
        let mut subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        subscribers.retain(|queue| Weak::upgrade(queue).is_some());
        for queue in subscribers.iter().filter_map(Weak::upgrade) {
            queue.push(event.clone());
        }
    }

    async fn subscribe(&self) -> BoxStream<UiEvent> {
        let queue = Arc::new(Queue::default());
        let mut subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        subscribers.push(Arc::downgrade(&queue));
        Subscriber { queue }.boxed()
    }
}

/// One subscriber's events, and what wakes it.
#[derive(Debug, Default)]
struct Queue {
    events: Mutex<VecDeque<UiEvent>>,
    waker: futures::task::AtomicWaker,
}

impl Queue {
    /// Puts `event` at the end, keeping the queue at [`EVENT_BUFFER`] events
    /// by dropping the oldest progress first, and wakes the subscriber.
    fn push(&self, event: UiEvent) {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() >= EVENT_BUFFER {
            // A full queue keeps a state change (started, warning, ended, an
            // agent coming or going) at the cost of the oldest progress; a
            // progress event finds room only by displacing older progress,
            // and when there is none to displace it is dropped.
            let oldest_progress = events
                .iter()
                .position(|queued| matches!(queued, UiEvent::OperationProgress { .. }));
            match oldest_progress {
                Some(at) => {
                    events.remove(at);
                }
                None if matches!(event, UiEvent::OperationProgress { .. }) => return,
                None => {
                    events.pop_front();
                }
            }
        }
        events.push_back(event);
        drop(events);
        self.waker.wake();
    }

    /// The oldest event that is still there.
    fn pop(&self) -> Option<UiEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
    }
}

/// Reads one subscriber's events, in order, for as long as it is polled.
struct Subscriber {
    queue: Arc<Queue>,
}

impl Stream for Subscriber {
    type Item = UiEvent;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<UiEvent>> {
        if let Some(event) = self.queue.pop() {
            return Poll::Ready(Some(event));
        }
        self.queue.waker.register(cx.waker());
        // An event pushed between the empty check and the registration is
        // still delivered: waking a registered waker is what push does.
        if let Some(event) = self.queue.pop() {
            return Poll::Ready(Some(event));
        }
        Poll::Pending
    }
}
