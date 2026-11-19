//! The agent routes under `/sync/` (technical design §7.1), which only a
//! build with the `hub` feature serves, and only `serve --hub` (§16, decision
//! 122). Anything else answers 404 on every path there, so a release never
//! shows the UI's page, or anything of the hub, at the hub's address.
//!
//! `/sync/v1` carries the handshake (decision 120, "The handshake") to
//! [`HubPairing`]'s steps, under its bounds before the proofs: a frame over
//! [`MAX_FRAME_BYTES`] closes the connection, as does a first frame that
//! isn't there within 2 s or a handshake not over within 15 s, and at most
//! 32 connections are unauthenticated at once, the oldest evicted for a
//! newcomer. This listener has no TLS of the hub's own, so each session's
//! exporter is `None`, and a `Connect` naming one closes
//! `binding-unavailable`.

use axum::Router;
use axum::http::StatusCode;
use axum::routing::any;

#[cfg(feature = "hub")]
pub use served::SyncHub;

/// What `/sync/` serves: nothing, in a build without the hub.
#[cfg(not(feature = "hub"))]
#[derive(Clone, Debug, Default)]
pub struct SyncHub {
    // Made only by `default`, as the hub's is.
    _nothing: (),
}

/// The routes under `/sync/`: the handshake's when `hub` serves it, 404 on
/// every path there otherwise.
pub(crate) fn routes<S>(hub: &SyncHub) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    #[cfg(feature = "hub")]
    if let Some(router) = served::routes(hub) {
        return router;
    }
    #[cfg(not(feature = "hub"))]
    let _ = hub;
    let not_found = any(|| async { StatusCode::NOT_FOUND });
    Router::new()
        .route("/sync", not_found.clone())
        .route("/sync/", not_found.clone())
        .route("/sync/{*rest}", not_found)
}

#[cfg(feature = "hub")]
mod served {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use api::sync::{AgentFrame, MAX_FRAME_BYTES, PATH};
    use app::pairing::{HubPairing, HubStep, PROTOCOL_ERROR};
    use axum::Router;
    use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
    use axum::routing::get;
    use tokio::sync::Notify;

    /// The handshake's bounds before the proofs (decision 120).
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Bounds {
        /// How long a connection has to send its first frame.
        pub(crate) connect_within: Duration,
        /// How long the handshake may take from the upgrade.
        pub(crate) handshake_within: Duration,
        /// How many connections may be unauthenticated at once.
        pub(crate) unauthenticated: usize,
    }

    impl Bounds {
        pub(crate) const DECIDED: Self = Self {
            connect_within: Duration::from_secs(2),
            handshake_within: Duration::from_secs(15),
            unauthenticated: 32,
        };
    }

    /// What `/sync/` serves: the handshake, under `serve --hub`, or
    /// nothing.
    #[derive(Clone, Debug, Default)]
    pub struct SyncHub {
        served: Option<Arc<Served>>,
    }

    impl SyncHub {
        /// `/sync/v1` carrying `pairing`'s handshake.
        pub fn serving(pairing: Arc<HubPairing>) -> Self {
            Self::bounded(pairing, Bounds::DECIDED)
        }

        pub(crate) fn bounded(pairing: Arc<HubPairing>, bounds: Bounds) -> Self {
            Self {
                served: Some(Arc::new(Served {
                    pairing,
                    bounds,
                    waiting: Mutex::default(),
                })),
            }
        }
    }

    pub(super) fn routes<S>(hub: &SyncHub) -> Option<Router<S>>
    where
        S: Clone + Send + Sync + 'static,
    {
        let served = hub.served.clone()?;
        Some(Router::new().route(
            PATH,
            get(move |upgrade: WebSocketUpgrade| async move {
                upgrade
                    .max_message_size(MAX_FRAME_BYTES)
                    .max_frame_size(MAX_FRAME_BYTES)
                    .on_upgrade(move |socket| served.run(socket))
            }),
        ))
    }

    struct Served {
        pairing: Arc<HubPairing>,
        bounds: Bounds,
        waiting: Mutex<Waiting>,
    }

    impl std::fmt::Debug for Served {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Served")
                .field("bounds", &self.bounds)
                .finish_non_exhaustive()
        }
    }

    /// The unauthenticated connections, oldest first, each with what tells
    /// it it was evicted.
    #[derive(Default)]
    struct Waiting {
        next: u64,
        open: VecDeque<(u64, Arc<Notify>)>,
    }

    /// How a connection ends.
    enum End {
        /// The pairing was answered.
        Answered,
        /// Closed with this reason, unanswered.
        Refused(&'static str),
        /// Closed with no reason: the agent left, the time ran out, it was
        /// evicted, or the hub couldn't take a step.
        Dropped,
    }

    /// What the agent sent next.
    enum Next {
        Frame(AgentFrame),
        Unreadable,
        Gone,
    }

    impl Served {
        async fn run(self: Arc<Self>, mut socket: WebSocket) {
            let (id, evicted) = self.admit();
            let end = tokio::select! {
                end = tokio::time::timeout(
                    self.bounds.handshake_within,
                    self.converse(&mut socket),
                ) => end.unwrap_or(End::Dropped),
                () = evicted.notified() => End::Dropped,
            };
            self.leave(id);
            let (code, reason) = match end {
                End::Answered => (close_code::NORMAL, ""),
                End::Refused(reason) => (close_code::POLICY, reason),
                End::Dropped => (close_code::POLICY, ""),
            };
            let close = CloseFrame {
                code,
                reason: reason.into(),
            };
            let _ = socket.send(Message::Close(Some(close))).await;
        }

        /// A place among the unauthenticated, evicting the oldest when the
        /// table is full.
        fn admit(&self) -> (u64, Arc<Notify>) {
            let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
            while waiting.open.len() >= self.bounds.unauthenticated {
                let Some((_, oldest)) = waiting.open.pop_front() else {
                    break;
                };
                oldest.notify_one();
            }
            let id = waiting.next;
            waiting.next = waiting.next.wrapping_add(1);
            let evicted = Arc::new(Notify::new());
            waiting.open.push_back((id, Arc::clone(&evicted)));
            (id, evicted)
        }

        fn leave(&self, id: u64) {
            self.waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .open
                .retain(|(open, _)| *open != id);
        }

        async fn converse(&self, socket: &mut WebSocket) -> End {
            let mut session = match self.pairing.session(None).await {
                Ok(session) => session,
                Err(error) => {
                    log::warn!("/sync/v1: the hub's keys couldn't be read: {error:?}");
                    return End::Dropped;
                }
            };
            let first = tokio::time::timeout(self.bounds.connect_within, next(socket)).await;
            let mut read = first.unwrap_or(Next::Gone);
            loop {
                let frame = match read {
                    Next::Frame(frame) => frame,
                    Next::Unreadable => return End::Refused(PROTOCOL_ERROR),
                    Next::Gone => return End::Dropped,
                };
                match self.pairing.step(&mut session, &frame).await {
                    HubStep::Reply(answer) => {
                        if socket.send(Message::text(answer.to_text())).await.is_err() {
                            return End::Dropped;
                        }
                    }
                    HubStep::Last(answer) => {
                        let _ = socket.send(Message::text(answer.to_text())).await;
                        return End::Answered;
                    }
                    HubStep::Close(reason) => return End::Refused(reason),
                    HubStep::Fail(error) => {
                        log::warn!("/sync/v1: the hub couldn't take a pairing step: {error:?}");
                        return End::Dropped;
                    }
                }
                read = next(socket).await;
            }
        }
    }

    async fn next(socket: &mut WebSocket) -> Next {
        loop {
            return match socket.recv().await {
                Some(Ok(Message::Text(text))) => match AgentFrame::from_text(text.as_str()) {
                    Ok(frame) => Next::Frame(frame),
                    Err(_) => Next::Unreadable,
                },
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Binary(_))) => Next::Unreadable,
                Some(Ok(Message::Close(_)) | Err(_)) | None => Next::Gone,
            };
        }
    }
}

// Two attributes, not `all(test, …)`: clippy's test allowances see only a
// bare `cfg(test)`.
#[cfg(test)]
#[cfg(feature = "hub")]
mod tests;
