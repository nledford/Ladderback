use std::sync::Arc;
use std::time::Duration;

use api::sync::{AgentFrame, HubFrame, PATH};
use app::fakes::{FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, SeededRandom};
use app::pairing::{BINDING_UNAVAILABLE, HubPairing, PROTOCOL_ERROR};
use app::ports::{Attempt, Identity, IdentityError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{HubPin, PublicKey, SeedFile, SigningSeed, TlsKey};
use futures::{SinkExt as _, StreamExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use super::served::Bounds;
use super::*;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long a test waits for the hub to say anything.
const PATIENCE: Duration = Duration::from_secs(5);

fn pairing() -> Arc<HubPairing> {
    pairing_on(Arc::new(InMemoryIdentity::default()))
}

fn pairing_on(identity: Arc<dyn Identity>) -> Arc<HubPairing> {
    Arc::new(HubPairing::new(
        Arc::new(InMemoryAgentRegistry::new()),
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(5)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        )),
    ))
}

/// `hub`'s routes on loopback; their address.
async fn serve(hub: &SyncHub) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = routes::<()>(hub);
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("127.0.0.1:{}", address.port())
}

async fn open(address: &str) -> Socket {
    connect_async(format!("ws://{address}{PATH}"))
        .await
        .unwrap()
        .0
}

fn connect(binding_kind: u8) -> Message {
    Message::text(
        AgentFrame::Connect {
            agent_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            agent_nonce: "A".repeat(43),
            binding_kind,
        }
        .to_text(),
    )
}

/// The next message, past pings; `None` when the connection ended.
async fn next(socket: &mut Socket) -> Option<Message> {
    let read = tokio::time::timeout(PATIENCE, socket.next()).await.unwrap();
    read.and_then(Result::ok)
}

/// The reason the hub closed with, `""` for none; panics on anything else.
async fn closed_with(socket: &mut Socket) -> String {
    match next(socket).await {
        Some(Message::Close(frame)) => frame.map(|f| f.reason.to_string()).unwrap_or_default(),
        None => String::new(),
        other => panic!("not closed: {other:?}"),
    }
}

#[tokio::test]
async fn a_connect_is_answered_with_the_hubs_challenge() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket.send(connect(0)).await.unwrap();
    let Some(Message::Text(text)) = next(&mut socket).await else {
        panic!("no answer");
    };
    assert!(matches!(
        HubFrame::from_text(text.as_str()),
        Ok(HubFrame::Challenge { .. })
    ));
}

/// This listener has no TLS of the hub's own, so a `Connect` that binds to
/// one is closed `binding-unavailable` (decision 121).
#[tokio::test]
async fn a_connect_binding_to_tls_closes_binding_unavailable_here() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket.send(connect(1)).await.unwrap();
    assert_eq!(closed_with(&mut socket).await, BINDING_UNAVAILABLE);
}

#[tokio::test]
async fn a_frame_that_isnt_one_closes_protocol_error() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket
        .send(Message::text("{\"type\":\"hello\"}"))
        .await
        .unwrap();
    assert_eq!(closed_with(&mut socket).await, PROTOCOL_ERROR);
}

/// A frame over 4 KiB ends the connection before it is read (decision 120,
/// "Bounds before the proofs").
#[tokio::test]
async fn a_frame_over_4_kib_ends_the_connection() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    let long = format!("{{\"type\":\"connect\",\"pad\":\"{}\"}}", "a".repeat(4096));
    let _ = socket.send(Message::text(long)).await;
    match next(&mut socket).await {
        None | Some(Message::Close(_)) => {}
        other => panic!("the connection went on: {other:?}"),
    }
}

/// Key files that are slow to read, as a data folder on a stalled disk is.
struct SlowIdentity {
    files: InMemoryIdentity,
    delay: Duration,
}

#[async_trait]
impl Identity for SlowIdentity {
    async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError> {
        tokio::time::sleep(self.delay).await;
        self.files.seed(file).await
    }

    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, IdentityError> {
        self.files.keep_seed(file, seed).await
    }

    async fn forget_recovery_seed(&self) -> Result<(), IdentityError> {
        self.files.forget_recovery_seed().await
    }

    async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError> {
        self.files.recovery_key().await
    }

    async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError> {
        self.files.keep_recovery_key(key).await
    }

    async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError> {
        self.files.tls_key().await
    }

    async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError> {
        self.files.keep_tls_key(key).await
    }

    async fn pin(&self) -> Result<Option<HubPin>, IdentityError> {
        self.files.pin().await
    }

    async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError> {
        self.files.write_pin(pin).await
    }

    async fn begin_attempt(&self) -> Result<Attempt, IdentityError> {
        self.files.begin_attempt().await
    }
}

/// The first bound counts from when the connection is admitted, so reading
/// the hub's keys is within it: a peer that says nothing holds its place
/// for the bound, not for the bound and the time the keys took.
#[tokio::test]
async fn the_first_bound_includes_the_time_the_hubs_keys_take_to_read() {
    let bounds = Bounds {
        connect_within: Duration::from_millis(200),
        ..Bounds::DECIDED
    };
    let slow = SlowIdentity {
        files: InMemoryIdentity::default(),
        delay: Duration::from_millis(600),
    };
    let address = serve(&SyncHub::bounded(pairing_on(Arc::new(slow)), bounds)).await;
    let started = std::time::Instant::now();
    let mut socket = open(&address).await;
    assert_eq!(closed_with(&mut socket).await, "");
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(500),
        "closed after {took:?}, not within the 200 ms first bound"
    );
}

/// The scheduling margin a bound may overrun by on a busy machine.
const MARGIN: Duration = Duration::from_millis(700);

/// Closed with no reason, no sooner than `bound` after `started` (the hub
/// counts from after that) and no later than `bound` and [`MARGIN`]: a
/// bound of seconds wouldn't pass for one of a few hundred milliseconds.
async fn closed_at_its_bound(socket: &mut Socket, started: std::time::Instant, bound: Duration) {
    assert_eq!(closed_with(socket).await, "");
    let took = started.elapsed();
    assert!(
        took >= bound && took <= bound + MARGIN,
        "closed after {took:?}, not at its {bound:?} bound"
    );
}

#[tokio::test]
async fn a_connection_silent_past_its_first_bound_is_closed() {
    let bound = Duration::from_millis(100);
    let bounds = Bounds {
        connect_within: bound,
        ..Bounds::DECIDED
    };
    let address = serve(&SyncHub::bounded(pairing(), bounds)).await;
    let started = std::time::Instant::now();
    let mut socket = open(&address).await;
    closed_at_its_bound(&mut socket, started, bound).await;
}

#[tokio::test]
async fn a_handshake_not_over_within_its_bound_is_closed() {
    let bound = Duration::from_millis(300);
    let bounds = Bounds {
        handshake_within: bound,
        ..Bounds::DECIDED
    };
    let address = serve(&SyncHub::bounded(pairing(), bounds)).await;
    let started = std::time::Instant::now();
    let mut socket = open(&address).await;
    socket.send(connect(0)).await.unwrap();
    assert!(matches!(next(&mut socket).await, Some(Message::Text(_))));
    closed_at_its_bound(&mut socket, started, bound).await;
}

/// When the table of unauthenticated connections is full, the oldest is
/// evicted for the newcomer, which is served.
#[tokio::test]
async fn a_full_table_evicts_its_oldest_connection() {
    let bounds = Bounds {
        unauthenticated: 2,
        ..Bounds::DECIDED
    };
    let address = serve(&SyncHub::bounded(pairing(), bounds)).await;
    let mut oldest = open(&address).await;
    let mut second = open(&address).await;
    // Both are in the table once the hub has answered each.
    for socket in [&mut oldest, &mut second] {
        socket.send(connect(0)).await.unwrap();
        assert!(matches!(next(socket).await, Some(Message::Text(_))));
    }
    let mut newcomer = open(&address).await;
    assert_eq!(closed_with(&mut oldest).await, "");
    newcomer.send(connect(0)).await.unwrap();
    assert!(matches!(next(&mut newcomer).await, Some(Message::Text(_))));
}

/// `serve` without `--hub` answers 404 under `/sync/`, as a release does.
#[tokio::test]
async fn without_the_hub_every_path_under_sync_answers_404() {
    let address = serve(&SyncHub::default()).await;
    let refused = connect_async(format!("ws://{address}{PATH}")).await;
    let Err(tokio_tungstenite::tungstenite::Error::Http(response)) = refused else {
        panic!("upgraded without the hub");
    };
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// The daemon's whole router, with `serve --hub`'s sync routes.
async fn send(request: axum::http::Request<axum::body::Body>) -> axum::response::Response {
    use tower::ServiceExt as _;
    let state = crate::AppState {
        sync: SyncHub::serving(pairing()),
        ..crate::request_checks::tests::test_state()
    };
    crate::router(
        Arc::new(crate::request_checks::tests::NoUi),
        crate::RequestChecks::new([]),
        state,
    )
    .oneshot(request)
    .await
    .unwrap()
}

/// What an agent's WebSocket handshake carries, besides its `Host`.
const HANDSHAKE: [(&str, &str); 4] = [
    ("connection", "upgrade"),
    ("upgrade", "websocket"),
    ("sec-websocket-version", "13"),
    ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
];

fn handshake(
    method: &str,
    path: &str,
    more: &[(&str, &str)],
) -> axum::http::Request<axum::body::Body> {
    let mut headers = vec![("host", "localhost:9898")];
    headers.extend(HANDSHAKE);
    headers.extend(more);
    crate::request_checks::tests::request(method, path, &headers)
}

/// Every path the sync routes answer, served or not.
const SYNC_PATHS: [&str; 6] = [
    "/sync/v1",
    "/sync/v1?x=1",
    "/sync/v1/connect",
    "/sync/v2",
    "/sync/",
    "/sync",
];

/// A web page's WebSocket always carries its origin, its own page's too, so
/// a handshake with one is a browser's: refused before the handshake, on
/// every path under `/sync/`.
#[tokio::test]
async fn a_handshake_carrying_an_origin_is_refused_before_it_starts() {
    for path in SYNC_PATHS {
        for origin in ["http://evil.example", "http://localhost:9898", "null"] {
            let response = send(handshake("GET", path, &[("origin", origin)])).await;
            assert_eq!(
                crate::request_checks::tests::code(response).await,
                Some((StatusCode::FORBIDDEN, "origin-not-allowed".to_owned())),
                "{path} from {origin}"
            );
        }
    }
}

#[tokio::test]
async fn only_a_get_reaches_the_sync_routes() {
    for path in SYNC_PATHS {
        for method in [
            "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT",
        ] {
            let response = send(handshake(method, path, &[])).await;
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path}"
            );
            assert_eq!(response.headers()["allow"], "GET", "{method} {path}");
        }
    }
}

/// A page's `fetch` can't ask to upgrade (`Upgrade` is a header it may not
/// set), so a request that doesn't, by whatever name it reaches the hub,
/// is refused before any route sees it.
#[tokio::test]
async fn a_request_that_isnt_a_websocket_handshake_is_refused() {
    for path in SYNC_PATHS {
        for upgrade in [None, Some("h2c"), Some("websocket2")] {
            let mut headers = vec![("host", "hub.other.example:8443")];
            headers.extend(upgrade.map(|value| ("upgrade", value)));
            let request = crate::request_checks::tests::request("GET", path, &headers);
            let response = send(request).await;
            assert_eq!(
                response.status(),
                StatusCode::UPGRADE_REQUIRED,
                "{path} upgrading to {upgrade:?}"
            );
            assert_eq!(response.headers()["upgrade"], "websocket", "{path}");
        }
    }
}

/// Every path under `/sync/` is the sync routes': a route the UI's and the
/// API's router has there is never reached, so an agent route can only be
/// added behind the sync check.
#[tokio::test]
async fn a_route_beside_the_sync_routes_under_sync_is_never_reached() {
    use tower::ServiceExt as _;
    let app = Router::new().route(
        "/sync/v1/x",
        axum::routing::get(|| async { "the browser's" }),
    );
    let response = in_front_of(app, &SyncHub::serving(pairing()))
        .oneshot(handshake("GET", "/sync/v1/x", &[]))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
