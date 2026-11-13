//! The contract every [`EventBus`] keeps: delivery, order, fan-out and the
//! lagging-subscriber rule. The in-memory fake runs this suite in `app`'s
//! tests; the daemon's broadcast implementation runs it too.
//!
//! A bus that meets this suite is prompt: once `publish` has returned, every
//! event it handed out is already readable, so the suite polls without
//! waiting. A bus that needs to settle first delivers nothing here, which
//! fails the suite.

use std::sync::Arc;

use api::UiEvent;
use async_trait::async_trait;
use futures::StreamExt;
use futures::future::FutureExt;

use crate::ports::{EVENT_BUFFER, EventBus};

/// Lets the suite talk to a bus without knowing which one it is.
#[async_trait]
pub trait EventBusHarness: Send + Sync {
    fn bus(&self) -> Arc<dyn EventBus>;
}

/// Runs every check. Panics on the first violation.
pub async fn run(harness: &dyn EventBusHarness) {
    a_subscriber_receives_what_was_published_in_order(harness).await;
    every_subscriber_receives_the_same_events(harness).await;
    a_lagging_subscriber_loses_progress_first(harness).await;
    a_lagging_subscriber_never_gives_up_a_state_change_for_progress(harness).await;
}

fn operation(suffix: &str) -> String {
    format!("018f0000-0000-7000-8000-0000000000{suffix}")
}

fn started(operation: &str) -> UiEvent {
    UiEvent::OperationStarted {
        operation: operation.into(),
        repository: Some("home.demo".into()),
        started_at: "2026-11-09T10:00:00Z".into(),
    }
}

fn progress(operation: &str, current: u64) -> UiEvent {
    UiEvent::OperationProgress {
        operation: operation.into(),
        step: None,
        current,
        total: Some(current + 1),
    }
}

fn succeeded(operation: &str) -> UiEvent {
    UiEvent::OperationSucceeded {
        operation: operation.into(),
        finished_at: "2026-11-09T10:05:00Z".into(),
    }
}

/// Reads the next `count` events, or fewer when the stream ends or has
/// nothing ready. A bus may hold an event back only until its `publish` has
/// returned.
async fn take(stream: &mut (impl StreamExt<Item = UiEvent> + Unpin), count: usize) -> Vec<UiEvent> {
    let mut seen = Vec::with_capacity(count);
    for _ in 0..count {
        match stream.next().now_or_never() {
            Some(Some(event)) => seen.push(event),
            Some(None) => break,
            None => break,
        }
    }
    seen
}

async fn publish_all(bus: &Arc<dyn EventBus>, events: &[UiEvent]) {
    for event in events {
        bus.publish(event.clone()).await;
    }
}

async fn a_subscriber_receives_what_was_published_in_order(harness: &dyn EventBusHarness) {
    let bus = harness.bus();
    let mut stream = bus.subscribe().await;
    let operation = operation("01");
    let events = vec![
        started(&operation),
        progress(&operation, 1),
        progress(&operation, 2),
        succeeded(&operation),
    ];
    publish_all(&bus, &events).await;
    assert_eq!(take(&mut stream, events.len()).await, events);
}

async fn every_subscriber_receives_the_same_events(harness: &dyn EventBusHarness) {
    let bus = harness.bus();
    let mut first = bus.subscribe().await;
    let mut second = bus.subscribe().await;
    let operation = operation("02");
    let events = vec![
        started(&operation),
        progress(&operation, 1),
        succeeded(&operation),
    ];
    publish_all(&bus, &events).await;
    assert_eq!(take(&mut first, events.len()).await, events);
    assert_eq!(take(&mut second, events.len()).await, events);
}

async fn a_lagging_subscriber_loses_progress_first(harness: &dyn EventBusHarness) {
    let bus = harness.bus();
    let mut stalled = bus.subscribe().await;
    let operation = operation("03");

    // The subscriber reads nothing while far more than `EVENT_BUFFER` events
    // pile up, ending in state changes a page must never miss.
    let mut events = vec![started(&operation)];
    for current in 0..(EVENT_BUFFER as u64 * 2) {
        events.push(progress(&operation, current));
    }
    events.push(succeeded(&operation));
    publish_all(&bus, &events).await;

    let seen = take(&mut stalled, events.len()).await;
    assert_eq!(
        seen.len(),
        EVENT_BUFFER,
        "a stalled subscriber keeps at most one buffer of events"
    );
    assert_eq!(
        seen.first(),
        Some(&started(&operation)),
        "the start is a state change: it is never dropped for progress"
    );
    assert_eq!(
        seen.last(),
        Some(&succeeded(&operation)),
        "the finish is a state change: it is never dropped for progress"
    );
    let kept: Vec<u64> = seen
        .iter()
        .filter_map(|event| match event {
            UiEvent::OperationProgress { current, .. } => Some(*current),
            _ => None,
        })
        .collect();
    let newest = (EVENT_BUFFER as u64 * 2 - (kept.len() as u64))..EVENT_BUFFER as u64 * 2;
    assert_eq!(
        kept,
        newest.collect::<Vec<_>>(),
        "the events kept are the newest ones, in order"
    );
}

/// A queue full of state changes still takes one more state change (the
/// oldest leaves) and never takes a progress event at a state change's cost.
async fn a_lagging_subscriber_never_gives_up_a_state_change_for_progress(
    harness: &dyn EventBusHarness,
) {
    let bus = harness.bus();
    let mut stalled = bus.subscribe().await;

    // More state changes than one buffer: the newest state change still
    // arrives, displacing the oldest state change.
    let operations: Vec<String> = (0..(EVENT_BUFFER + 1))
        .map(|n| operation(&format!("{n:02}")))
        .collect();
    let finishes: Vec<UiEvent> = operations.iter().map(|op| succeeded(op)).collect();
    publish_all(&bus, &finishes).await;
    let seen = take(&mut stalled, operations.len()).await;
    assert_eq!(
        seen.last(),
        Some(&finishes[EVENT_BUFFER]),
        "a state change is never dropped, not even for other state changes"
    );

    // A progress event arrives while the queue holds nothing but state
    // changes: it is the progress that goes, not the state changes.
    publish_all(&bus, &finishes).await;
    publish_all(&bus, &[progress(&operations[0], 0)]).await;
    let seen = take(&mut stalled, operations.len()).await;
    assert_eq!(
        seen,
        finishes[1..],
        "progress is dropped before any state change is"
    );
}
