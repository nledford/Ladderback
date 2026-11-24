//! The agent routes under `/sync/` (technical design §7.1), which only a
//! build with the `hub` feature serves, and only `serve --hub` (§16, decision
//! 122). Anything else answers 404 on every path there, so a release never
//! shows the UI's page, or anything of the hub, at the hub's address.
//!
//! `/sync/v1` carries the handshake (decision 120, "The handshake") to
//! [`HubPairing`](app::pairing::HubPairing)'s steps, under its bounds before
//! the proofs: a frame over [`MAX_FRAME_BYTES`](api::sync::MAX_FRAME_BYTES)
//! closes the connection, as does a first frame that
//! isn't there within 2 s or a handshake not over within 15 s, and at most
//! 32 connections are unauthenticated at once, the oldest evicted for a
//! newcomer. Over the hub's built-in TLS each connection has its own
//! address, and one address may hold at most 4 of them, its oldest evicted
//! for its newcomer; behind a proxy only the total applies. A session's
//! exporter is there only when it came over the built-in TLS with the
//! hybrid key exchange, and a `Connect` naming one on any other closes
//! `binding-unavailable`. Once the hub welcomed an approved agent, which
//! then sends its reports (§7.2), the connection leaves the
//! unauthenticated and stays open until the agent closes it, sends
//! nothing for 90 seconds (three heartbeats missed; decision 159) or
//! connects again, since an agent holds one live connection (decision
//! 120); its frames keep the same cap. While the hub holds it, the hub
//! shows the machine online.
//!
//! In a build with the hub, every path under `/sync/` is these routes',
//! behind a check of their own in place of the browser's request checks
//! (technical design §10.1): agents reach the hub by whatever name it has
//! and prove themselves with signatures, and send no `Origin`. The check
//! takes only what an agent sends, a `GET` asking to upgrade to a WebSocket,
//! with no `Origin` and no body, which a web page can't send.

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
/// every path there otherwise. In a build with the hub, each is behind the
/// sync check, which a route added here can't leave out.
pub(crate) fn routes<S>(hub: &SyncHub) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let not_found = any(|| async { StatusCode::NOT_FOUND });
    let nothing = Router::new()
        .route("/sync", not_found.clone())
        .route("/sync/", not_found.clone())
        .route("/sync/{*rest}", not_found);
    // Every other path under `/sync/` answers 404 beside the handshake's.
    #[cfg(feature = "hub")]
    let nothing = match served::routes(hub) {
        Some(router) => nothing.merge(router),
        None => nothing,
    }
    .layer(axum::middleware::from_fn(check::enforce));
    #[cfg(not(feature = "hub"))]
    let _ = hub;
    nothing
}

/// The daemon's router: `app`, the UI and the API under the browser's
/// checks, behind the routes under `/sync/`. Every path there goes to
/// [`routes`], so a route `app` has under `/sync/` is never reached: an
/// agent route can only be added behind the sync check.
#[cfg(feature = "hub")]
pub(crate) fn in_front_of(app: Router, hub: &SyncHub) -> Router {
    routes(hub).fallback_service(app)
}

/// The sync routes' own check (technical design §10.1). A web page can't
/// pass it: its WebSocket always carries its `Origin`, and its other
/// requests may not set `Upgrade`. So the hub needs no `Host` check here,
/// which DNS rebinding would otherwise call for, and no `Content-Type`
/// check, which guards bodies.
#[cfg(feature = "hub")]
mod check {
    use api::ErrorCode;
    use axum::body::HttpBody as _;
    use axum::extract::Request;
    use axum::http::header::{ALLOW, ORIGIN, UPGRADE};
    use axum::http::{Method, StatusCode};
    use axum::middleware::Next;
    use axum::response::{IntoResponse as _, Response};

    /// Refuses, in this order, a request that carries an `Origin` (`403
    /// origin-not-allowed`, whatever the origin: the UI never opens these
    /// routes), a method other than `GET` (`405`), one that doesn't ask to
    /// upgrade to a WebSocket (`426`) and one with a body (`413
    /// body-too-large`, of a limit of 0 bytes), before any route sees it.
    pub(super) async fn enforce(request: Request, next: Next) -> Response {
        let headers = request.headers();
        if headers.contains_key(ORIGIN) {
            return crate::request_checks::refusal(ErrorCode::OriginNotAllowed);
        }
        if request.method() != Method::GET {
            return (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, "GET")]).into_response();
        }
        let websocket = headers
            .get(UPGRADE)
            .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
        if !websocket {
            return (StatusCode::UPGRADE_REQUIRED, [(UPGRADE, "websocket")]).into_response();
        }
        // A request has no body when its length is known to be 0: none
        // declared, or `Content-Length: 0`. Chunked framing announces one.
        if request.body().size_hint().exact() != Some(0) {
            return crate::body_limit::too_large(0);
        }
        next.run(request).await
    }
}

#[cfg(feature = "hub")]
mod served {
    use std::collections::VecDeque;
    use std::net::IpAddr;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use api::sync::{AgentFrame, MAX_FRAME_BYTES, PATH};
    use app::pairing::{HubPairing, HubSession, HubStep, PROTOCOL_ERROR};
    use axum::Router;
    use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
    use axum::extract::{ConnectInfo, Extension};
    use axum::routing::get;
    use domain::AgentId;
    use domain::fleet::{Fingerprint, KEY_BYTES, SILENCE_LIMIT};
    use tokio::sync::Notify;

    use crate::{BuiltInTls, PeerAddr};

    /// How long the hub waits to send its close frame to a peer that doesn't
    /// read, once the handshake is over or abandoned.
    const CLOSE_WITHIN: Duration = Duration::from_secs(1);

    /// The handshake's bounds before the proofs (decision 120), and the
    /// idle bound of an approved agent's connection after them.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Bounds {
        /// How long a connection has to send its first frame.
        pub(crate) connect_within: Duration,
        /// How long the handshake may take from the upgrade.
        pub(crate) handshake_within: Duration,
        /// How many connections may be unauthenticated at once.
        pub(crate) unauthenticated: usize,
        /// How many of them one address may hold over the built-in TLS.
        pub(crate) per_address: usize,
        /// How long an approved agent's connection may go without a frame
        /// once the hub takes its reports: three of its heartbeats.
        pub(crate) idle_within: Duration,
    }

    impl Bounds {
        pub(crate) const DECIDED: Self = Self {
            connect_within: Duration::from_secs(2),
            handshake_within: Duration::from_secs(15),
            unauthenticated: 32,
            per_address: 4,
            idle_within: SILENCE_LIMIT.unsigned_abs(),
        };
    }

    /// What `/sync/` serves: the handshake, under `serve --hub`, or
    /// nothing; and the fingerprint of the certificate the hub's built-in
    /// TLS serves, when it serves one.
    #[derive(Clone, Debug, Default)]
    pub struct SyncHub {
        served: Option<Arc<Served>>,
        certificate: Option<Fingerprint>,
        /// The hub's TLS key doesn't serve, so `/sync/v1` refuses every
        /// machine until it does.
        without_tls: bool,
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
                certificate: None,
                without_tls: false,
            }
        }

        /// The hub's pairing, when this daemon serves as a hub.
        pub(crate) fn pairing(&self) -> Option<&Arc<HubPairing>> {
            self.served.as_ref().map(|served| &served.pairing)
        }

        /// This hub, serving its built-in TLS with the certificate of
        /// `fingerprint`, which a pinned token names (decision 120).
        #[must_use]
        pub fn with_certificate(self, fingerprint: Fingerprint) -> Self {
            Self {
                certificate: Some(fingerprint),
                ..self
            }
        }

        /// The fingerprint of the certificate the hub's built-in TLS
        /// serves, when it serves one.
        pub(crate) fn certificate(&self) -> Option<Fingerprint> {
            self.certificate
        }

        /// This hub, whose built-in TLS key can't serve: `/sync/v1` answers
        /// `503` until a restart with the key restored, rather than serve
        /// machines without it (decision 120, "The hub key"). The pages
        /// keep its pairing, to list and revoke tokens.
        #[must_use]
        pub fn without_tls(self) -> Self {
            Self {
                certificate: None,
                without_tls: true,
                ..self
            }
        }
    }

    pub(super) fn routes<S>(hub: &SyncHub) -> Option<Router<S>>
    where
        S: Clone + Send + Sync + 'static,
    {
        let served = hub.served.clone()?;
        if hub.without_tls {
            return Some(Router::new().route(
                PATH,
                get(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
            ));
        }
        Some(Router::new().route(
            PATH,
            // A connection over the hub's built-in TLS carries its session's
            // exporter value when it negotiated the hybrid key exchange; a
            // kind-1 `Connect` on any other is `binding-unavailable`. Its
            // address is its own, which bounds how many it may hold; behind
            // a proxy every connection has the proxy's, so none is kept.
            get(
                move |upgrade: WebSocketUpgrade,
                      tls: Option<Extension<BuiltInTls>>,
                      peer: Option<Extension<ConnectInfo<PeerAddr>>>| async move {
                    let from = match (&tls, peer) {
                        (Some(_), Some(Extension(ConnectInfo(PeerAddr(peer))))) => {
                            Some(peer.ip().to_canonical())
                        }
                        _ => None,
                    };
                    let exporter = tls.and_then(|Extension(tls)| tls.exporter);
                    upgrade
                        .max_message_size(MAX_FRAME_BYTES)
                        .max_frame_size(MAX_FRAME_BYTES)
                        // Writes go out at once, and what a peer that doesn't
                        // read leaves queued, its pongs among it, stays bounded.
                        .write_buffer_size(0)
                        .max_write_buffer_size(2 * MAX_FRAME_BYTES)
                        .on_upgrade(move |socket| served.run(socket, exporter, from))
                },
            ),
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

    /// The unauthenticated connections, oldest first.
    #[derive(Default)]
    struct Waiting {
        next: u64,
        open: VecDeque<Open>,
    }

    /// An unauthenticated connection: its address, when it came over the
    /// built-in TLS, and what tells it it was evicted.
    struct Open {
        id: u64,
        from: Option<IpAddr>,
        evicted: Arc<Notify>,
    }

    /// How a connection ends.
    enum End {
        /// The pairing was answered.
        Answered,
        /// Closed with this reason, unanswered.
        Refused(&'static str),
        /// Closed with no reason: the agent left, the time ran out, it was
        /// evicted, a newer connection of the agent replaced it, or the hub
        /// couldn't take a step.
        Dropped,
    }

    /// Where a handshake came to: an end, or an approved agent's session
    /// whose reports the hub now takes.
    enum Handshake {
        Ended(End),
        Syncing(AgentId, Box<HubSession>),
    }

    /// What the agent sent next.
    enum Next {
        Frame(AgentFrame),
        Unreadable,
        Gone,
    }

    impl Served {
        async fn run(
            self: Arc<Self>,
            mut socket: WebSocket,
            exporter: Option<[u8; KEY_BYTES]>,
            from: Option<IpAddr>,
        ) {
            let (id, evicted) = self.admit(from);
            let handshake = tokio::select! {
                handshake = tokio::time::timeout(
                    self.bounds.handshake_within,
                    self.converse(&mut socket, exporter),
                ) => handshake.unwrap_or(Handshake::Ended(End::Dropped)),
                () = evicted.notified() => Handshake::Ended(End::Dropped),
            };
            self.leave(id);
            let end = match handshake {
                Handshake::Ended(end) => end,
                Handshake::Syncing(agent, mut session) => {
                    // The machine is online while this connection lasts; a
                    // newer one of the agent replaces it and closes it.
                    let replaced = Arc::new(Notify::new());
                    let close = Arc::clone(&replaced);
                    self.pairing
                        .went_live(agent, id, Box::new(move || close.notify_one()))
                        .await;
                    let end = tokio::select! {
                        end = self.sync(&mut socket, &mut session) => end,
                        () = replaced.notified() => End::Dropped,
                    };
                    self.pairing.went_away(agent, id).await;
                    end
                }
            };
            let (code, reason) = match end {
                End::Answered => (close_code::NORMAL, ""),
                End::Refused(reason) => (close_code::POLICY, reason),
                End::Dropped => (close_code::POLICY, ""),
            };
            let close = CloseFrame {
                code,
                reason: reason.into(),
            };
            // A peer that doesn't read can't hold the connection open past
            // its place in the table.
            let _ =
                tokio::time::timeout(CLOSE_WITHIN, socket.send(Message::Close(Some(close)))).await;
        }

        /// A place among the unauthenticated, evicting the address's oldest
        /// when it holds its share, and the oldest of all when the table is
        /// full.
        fn admit(&self, from: Option<IpAddr>) -> (u64, Arc<Notify>) {
            let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
            if from.is_some() {
                let mut held = waiting.open.iter().filter(|open| open.from == from).count();
                while held >= self.bounds.per_address {
                    let Some(oldest) = waiting.open.iter().position(|open| open.from == from)
                    else {
                        break;
                    };
                    if let Some(oldest) = waiting.open.remove(oldest) {
                        oldest.evicted.notify_one();
                    }
                    held = held.saturating_sub(1);
                }
            }
            while waiting.open.len() >= self.bounds.unauthenticated {
                let Some(oldest) = waiting.open.pop_front() else {
                    break;
                };
                oldest.evicted.notify_one();
            }
            let id = waiting.next;
            waiting.next = waiting.next.wrapping_add(1);
            let evicted = Arc::new(Notify::new());
            waiting.open.push_back(Open {
                id,
                from,
                evicted: Arc::clone(&evicted),
            });
            (id, evicted)
        }

        fn leave(&self, id: u64) {
            self.waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .open
                .retain(|open| open.id != id);
        }

        async fn converse(
            &self,
            socket: &mut WebSocket,
            exporter: Option<[u8; KEY_BYTES]>,
        ) -> Handshake {
            // The first bound counts from the connection's admission, so
            // reading the hub's keys is within it.
            let first_by = tokio::time::Instant::now() + self.bounds.connect_within;
            let mut session =
                match tokio::time::timeout_at(first_by, self.pairing.session(exporter)).await {
                    Ok(Ok(session)) => session,
                    Ok(Err(error)) => {
                        log::warn!("/sync/v1: the hub's keys couldn't be read: {error:?}");
                        return Handshake::Ended(End::Dropped);
                    }
                    Err(_) => return Handshake::Ended(End::Dropped),
                };
            let first = tokio::time::timeout_at(first_by, next(socket)).await;
            let mut read = first.unwrap_or(Next::Gone);
            loop {
                if let Some(end) = self.take(socket, &mut session, read).await {
                    return Handshake::Ended(end);
                }
                if let Some(agent) = session.syncing_agent() {
                    return Handshake::Syncing(agent, Box::new(session));
                }
                read = next(socket).await;
            }
        }

        /// An approved agent's reports after its handshake, under no bound
        /// but the idle one, which covers reading each frame and answering
        /// it, so an agent that stops reading is let go of too. The agent
        /// holds the connection, with a heartbeat every 30 seconds.
        async fn sync(&self, socket: &mut WebSocket, session: &mut HubSession) -> End {
            loop {
                let frame = async {
                    let read = next(socket).await;
                    self.take(socket, session, read).await
                };
                match tokio::time::timeout(self.bounds.idle_within, frame).await {
                    Ok(None) => {}
                    Ok(Some(end)) => return end,
                    Err(_) => return End::Dropped,
                }
            }
        }

        /// Takes what the agent sent and sends the hub's answer, if any:
        /// the end it comes to, none while the connection goes on.
        async fn take(
            &self,
            socket: &mut WebSocket,
            session: &mut HubSession,
            read: Next,
        ) -> Option<End> {
            let frame = match read {
                Next::Frame(frame) => frame,
                Next::Unreadable => return Some(End::Refused(PROTOCOL_ERROR)),
                Next::Gone => return Some(End::Dropped),
            };
            match self.pairing.step(session, &frame).await {
                HubStep::Reply(answer) => socket
                    .send(Message::text(answer.to_text()))
                    .await
                    .is_err()
                    .then_some(End::Dropped),
                HubStep::Wait => None,
                HubStep::Last(answer) => {
                    let _ = socket.send(Message::text(answer.to_text())).await;
                    Some(End::Answered)
                }
                HubStep::Close(reason) => Some(End::Refused(reason)),
                HubStep::Fail(error) => {
                    log::warn!("/sync/v1: the hub couldn't take a step: {error:?}");
                    Some(End::Dropped)
                }
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
