//! The DOM glue around the live pages' connections (technical design §4.4):
//! the fetch of the initial history and the events WebSocket, and their
//! reconnects. Every decision — what the state is, how an event changes it,
//! when to reconnect — lives in `backend.rs`, host-tested; this module only
//! touches the browser's types, so a host run of mutants skips it and the
//! browser journeys (`@ui`) cover it instead, the `live.rs` precedent.

use crate::backend::OperationsState;
use leptos::prelude::RwSignal;

/// Reads the operations history from `base` (the page's own origin) into
/// `state`, then opens the events stream and applies each event to `state`.
/// When the stream drops, it waits `backend::reconnect_backoff` and starts
/// over with the fetch: the history is the source of truth a reconnect
/// returns to, never a guess at what was missed.
#[allow(clippy::todo)]
pub(crate) fn watch(base: &str, state: RwSignal<OperationsState>) {
    let _ = (base, state);
    todo!("commit 1 stub: the browser journeys in ui-tests drive this")
}
