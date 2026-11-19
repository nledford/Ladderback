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
        todo!()
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
        todo!()
    }

        pub(crate) fn bounded(pairing: Arc<HubPairing>, bounds: Bounds) -> Self {
        todo!()
    }
    }

    pub(super) fn routes<S>(hub: &SyncHub) -> Option<Router<S>>
    where
        S: Clone + Send + Sync + 'static,
    {
        todo!()
    }

    struct Served {
        pairing: Arc<HubPairing>,
        bounds: Bounds,
        waiting: Mutex<Waiting>,
    }

    impl std::fmt::Debug for Served {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
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
        todo!()
    }

        /// A place among the unauthenticated, evicting the oldest when the
        /// table is full.
        fn admit(&self) -> (u64, Arc<Notify>) {
        todo!()
    }

        fn leave(&self, id: u64) {
        todo!()
    }

        async fn converse(&self, socket: &mut WebSocket) -> End {
        todo!()
    }
    }

    async fn next(socket: &mut WebSocket) -> Next {
        todo!()
    }
}

// Two attributes, not `all(test, …)`: clippy's test allowances see only a
// bare `cfg(test)`.
#[cfg(test)]
#[cfg(feature = "hub")]
mod tests;
