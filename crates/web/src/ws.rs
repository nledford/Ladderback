//! The DOM glue around the live pages' connections (technical design §4.4):
//! the fetch of the initial history and the events WebSocket, and their
//! reconnects. Every decision — what the state is, how an event changes it,
//! when to reconnect — lives in `backend.rs`, host-tested; this module only
//! touches the browser's types, so a host run of mutants skips it and the
//! browser journeys (`@ui`) cover it instead, the `live.rs` precedent.

use std::time::Duration;

use crate::backend::{OperationsState, ended, events_url, reconnect_backoff, succeeded};
use crate::maintenance::{self, Kind, Next, Watch, Watched};
use api::{ErrorCode, MsgArg, OperationDetailDto, OperationsPageDto, UiEvent};
use futures::StreamExt;
use gloo_net::http::Request;
use gloo_net::websocket::Message;
use gloo_net::websocket::futures::WebSocket;
use leptos::prelude::{RwSignal, Set, Update};
use leptos::task::spawn_local;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Where a live page's reading of the daemon stands: still loading its first
/// answer, ready (the table is the history), or unable to reach the daemon at
/// all. The page's heading follows this, so the reader is told before any
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Loading,
    Ready,
    Unreachable,
}

/// The body `POST /api/v1/operations` answers with: the started operation's
/// ID, unenveloped.
#[derive(Deserialize)]
struct StartedOperation {
    operation: String,
}

/// The operation a start's answer names, for the settle-poll that follows it.
pub(crate) fn started_operation(answer: &str) -> Option<String> {
    serde_json::from_str::<StartedOperation>(answer)
        .ok()
        .map(|started| started.operation)
}

/// A typed refusal, as the daemon writes it: the code, the arguments its
/// message takes, and sometimes the engine's own words. The page renders these
/// through the catalog (§8.4), so no refusal is ever shown as a status code.
#[derive(Deserialize)]
struct Refusal {
    code: ErrorCode,
    #[serde(default)]
    args: std::collections::BTreeMap<String, MsgArg>,
    #[serde(default)]
    detail: Option<String>,
}

/// What a POST came back with: `Done` (with the raw answer, for whoever wants
/// its body), a typed refusal to render (its code, arguments and the engine's
/// words, if it gave any), or `None` — the daemon never answered, which the
/// page's phase says its own way.
pub(crate) enum Posted {
    Done(String),
    Refused(ErrorCode, Vec<(String, MsgArg)>, Option<String>),
}

/// The page's own origin, where the daemon that served the page listens.
pub(crate) fn origin() -> String {
    web_sys::window()
        .map(|window| window.location().origin().unwrap_or_default())
        .unwrap_or_default()
}

/// Reads `url` as typed JSON. `None` is anything but the answer: the daemon
/// not answering, a refusal, or a body that isn't what was asked for — the
/// caller's phase decides what that means.
pub(crate) async fn read<T: DeserializeOwned>(url: &str) -> Option<T> {
    let response = Request::get(url).send().await.ok()?;
    if response.status() != 200 {
        return None;
    }
    response.json::<T>().await.ok()
}

/// What a GET came back with: the typed answer, or a typed refusal, which
/// the page renders as it renders a refused POST.
pub(crate) enum Got<T> {
    Read(T),
    Refused(ErrorCode, Vec<(String, MsgArg)>, Option<String>),
}

/// Reads `url` as typed JSON like `read`, but tells a refusal from a daemon
/// that did not answer: `None` is the daemon not answering, or an answer that
/// is neither the JSON asked for nor a typed refusal.
pub(crate) async fn get<T: DeserializeOwned>(url: &str) -> Option<Got<T>> {
    let response = Request::get(url).send().await.ok()?;
    let status = response.status();
    let text = response.text().await.ok()?;
    if succeeded(status) {
        return serde_json::from_str::<T>(&text).ok().map(Got::Read);
    }
    let refusal = serde_json::from_str::<Refusal>(&text).ok()?;
    Some(Got::Refused(
        refusal.code,
        refusal.args.into_iter().collect::<Vec<_>>(),
        refusal.detail,
    ))
}

/// Posts `body` as JSON to `url` (the JSON-body rule applies to every
/// mutating method, §4.2) and sorts the answer.
pub(crate) async fn post<S: Serialize>(url: &str, body: &S) -> Option<Posted> {
    let request = Request::post(url).json(body).ok()?;
    let response = request.send().await.ok()?;
    let status = response.status();
    let text = response.text().await.ok()?;
    if succeeded(status) {
        return Some(Posted::Done(text));
    }
    let refusal = serde_json::from_str::<Refusal>(&text).ok()?;
    Some(Posted::Refused(
        refusal.code,
        refusal.args.into_iter().collect::<Vec<_>>(),
        refusal.detail,
    ))
}

/// Reads the operations history from `base` (the page's own origin) into
/// `state`, then opens the events stream and applies each event to `state`.
/// When the stream drops, it waits `backend::reconnect_backoff` and starts
/// over with the fetch: the history is the source of truth a reconnect
/// returns to, never a guess at what was missed. A fetch that fails at any
/// point of the loop leaves the page in `Phase::Unreachable`, and the loop
/// keeps trying with the same backoff, so a daemon that comes back is a page
/// that heals.
pub(crate) fn watch(base: &str, state: RwSignal<OperationsState>, phase: RwSignal<Phase>) {
    let base = base.to_owned();
    spawn_local(async move {
        let mut attempt = 0u32;
        // The first answer builds the state; every answer after a disconnect
        // merges into it (§4.4): a status an event advanced is never handed
        // back to the store's lagging write, and a live row the store has
        // not written yet stays.
        let mut first = true;
        loop {
            match read::<OperationsPageDto>(&format!("{base}/api/v1/operations")).await {
                Some(page) => {
                    attempt = 0;
                    phase.set(Phase::Ready);
                    if first {
                        first = false;
                        state.set(OperationsState::from_history(page.operations, page.next));
                    } else {
                        state.update(|state| state.merge(page));
                    }
                    stream(&base, &state).await;
                    // The stream ended: a closed socket is a failure like any
                    // other, and the next round waits for it.
                    attempt += 1;
                    sleep(reconnect_backoff(attempt)).await;
                }
                None => {
                    attempt += 1;
                    phase.set(Phase::Unreachable);
                    sleep(reconnect_backoff(attempt)).await;
                }
            }
        }
    });
}

/// Applies the events stream to `state` until it ends. A `UiEvent` the state
/// holds no operation for is ignored here too: the reconnect's re-fetch is
/// what brings the whole story, never a half-invented row.
async fn stream(base: &str, state: &RwSignal<OperationsState>) {
    let Ok(mut socket) = WebSocket::open(&events_url(base)) else {
        return;
    };
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Text(text) = message
            && let Ok(event) = serde_json::from_str::<UiEvent>(&text)
        {
            state.update(|state| state.apply(&event));
        }
    }
}

/// After a start or a cancel, polls the history into `state` until
/// `operation` has ended: a start announces before the first event can
/// arrive, and a cancelled run emits no `UiEvent` at all, so the page does
/// not guess — it reads the store until the store says the run settled.
/// `false` is the budget running out with no end in sight: the caller says
/// so, rather than leave the reader watching a row that stopped moving.
pub(crate) async fn settle(base: &str, state: RwSignal<OperationsState>, operation: &str) -> bool {
    let url = format!("{base}/api/v1/operations");
    for _ in 0..(SETTLE_FOR_MS / POLL_EVERY_MS) {
        if let Some(page) = read::<OperationsPageDto>(&url).await {
            let settled = page
                .operations
                .iter()
                .any(|summary| summary.id == operation && ended(&summary.status));
            state.update(|state| state.merge(page));
            if settled {
                return true;
            }
        }
        sleep(POLL_EVERY).await;
    }
    false
}

/// Reads the operation of a maintenance run of `kind` until it has ended. The
/// pace of the reads and the patience with a silent daemon are
/// `maintenance::Watch`'s; `alive` says whether the page that waits is still
/// there.
pub(crate) async fn await_end(
    base: &str,
    operation: &str,
    kind: Kind,
    alive: impl Fn() -> bool,
) -> Watched {
    let url = format!("{base}{}", maintenance::operation_path(operation));
    let mut watch = Watch::new(kind);
    while alive() {
        let answer = read::<OperationDetailDto>(&url).await;
        match watch.after(answer.as_ref()) {
            Next::Again(delay) => sleep(delay).await,
            Next::Ended(end, findings) => return Watched::Ended(end, findings),
            Next::GaveUp => return Watched::GaveUp,
        }
    }
    Watched::Gone
}

/// How long a settle-poll keeps reading, and how often.
const SETTLE_FOR_MS: u32 = 120_000;
const POLL_EVERY_MS: u32 = 250;
const POLL_EVERY: Duration = Duration::from_millis(POLL_EVERY_MS as u64);

/// Waits `for_`, in the browser's own time.
async fn sleep(for_: Duration) {
    gloo_timers::future::sleep(for_).await;
}
