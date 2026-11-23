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
        "0.0.0-test".to_owned(),
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

/// An agent sends its `Hello` right after its `Proof`, unanswered, so a
/// refused proof leaves that `Hello` unread on the hub's side. The hub still
/// ends the connection so that its refusal reaches the agent: closing a
/// socket with unread data resets it, and a reset can discard what the
/// agent hasn't read yet.
#[tokio::test]
async fn a_refused_proof_reaches_an_agent_that_sent_its_hello_after_it() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut socket = open(&address).await;
    socket.send(connect(0)).await.unwrap();
    let Some(Message::Text(_)) = next(&mut socket).await else {
        panic!("no challenge");
    };
    let proof = AgentFrame::Proof {
        counter: 1,
        agent_proof: "A".repeat(86),
    };
    let hello = AgentFrame::Signed {
        kind: "hello".to_owned(),
        seq: 1,
        payload: "A".repeat(1_000),
        signature: "A".repeat(86),
    };
    socket.send(Message::text(proof.to_text())).await.unwrap();
    socket.send(Message::text(hello.to_text())).await.unwrap();
    // The hub has answered and closed before the agent reads.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let Some(Message::Text(text)) = next(&mut socket).await else {
        panic!("the refusal was lost");
    };
    assert!(matches!(
        HubFrame::from_text(text.as_str()),
        Ok(HubFrame::Refused { .. })
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

/// `hub`'s routes on loopback as the hub's built-in TLS hands each
/// connection on: with its session, and with `peer` as its address.
async fn serve_over_tls(hub: &SyncHub, peer: std::net::SocketAddr) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = routes::<()>(hub)
        .layer(axum::Extension(crate::BuiltInTls { exporter: None }))
        .layer(axum::Extension(axum::extract::ConnectInfo(
            crate::PeerAddr(peer),
        )));
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("127.0.0.1:{}", address.port())
}

/// Five unauthenticated connections, each answered in turn, the last with
/// the hub's challenge once it is in the table.
async fn five_answered(address: &str) -> Vec<Socket> {
    let mut sockets = Vec::new();
    for _ in 0..5 {
        let mut socket = open(address).await;
        socket.send(connect(0)).await.unwrap();
        assert!(matches!(next(&mut socket).await, Some(Message::Text(_))));
        sockets.push(socket);
    }
    sockets
}

/// Over the hub's built-in TLS, one address may hold 4 unauthenticated
/// connections: a fifth evicts that address's oldest, and the others stay
/// in the handshake (decision 120, "Bounds before the proofs").
#[tokio::test]
async fn over_built_in_tls_a_fifth_connection_from_one_address_evicts_its_oldest() {
    let peer = "192.0.2.7:50000".parse().unwrap();
    let address = serve_over_tls(&SyncHub::serving(pairing()), peer).await;
    let mut sockets = five_answered(&address).await;
    assert_eq!(closed_with(&mut sockets[0]).await, "");
    sockets[1].send(Message::text("not a frame")).await.unwrap();
    assert_eq!(closed_with(&mut sockets[1]).await, PROTOCOL_ERROR);
}

/// Behind a proxy every connection has the proxy's address, so only the
/// total bounds them: a fifth evicts none.
#[tokio::test]
async fn behind_a_proxy_a_fifth_connection_evicts_none() {
    let address = serve(&SyncHub::serving(pairing())).await;
    let mut sockets = five_answered(&address).await;
    sockets[0].send(Message::text("not a frame")).await.unwrap();
    assert_eq!(closed_with(&mut sockets[0]).await, PROTOCOL_ERROR);
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

/// A hub whose TLS key can't serve refuses every machine at `/sync/v1`
/// rather than serve one without it (decision 120, "The hub key").
#[tokio::test]
async fn a_hub_without_its_tls_refuses_every_machine() {
    let address = serve(&SyncHub::serving(pairing()).without_tls()).await;
    let refused = connect_async(format!("ws://{address}{PATH}")).await;
    let Err(tokio_tungstenite::tungstenite::Error::Http(response)) = refused else {
        panic!("upgraded without the hub's TLS");
    };
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// The daemon's whole router, with `serve --hub`'s sync routes.
fn daemon() -> Router {
    let state = crate::AppState {
        sync: SyncHub::serving(pairing()),
        ..crate::request_checks::tests::test_state()
    };
    crate::router(
        Arc::new(crate::request_checks::tests::NoUi),
        crate::RequestChecks::new([]),
        state,
    )
}

async fn send(request: axum::http::Request<axum::body::Body>) -> axum::response::Response {
    use tower::ServiceExt as _;
    daemon().oneshot(request).await.unwrap()
}

/// An agent reaches the hub by whatever name it knows it by, which the
/// daemon wasn't told to answer to, and its client sends no `Origin`: its
/// handshake gets through the whole router, whatever the case of its
/// `Upgrade`, as a proxy may write it.
#[tokio::test]
async fn an_agents_handshake_reaches_the_hub_by_any_name() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, daemon()).await });
    for upgrade in ["websocket", "WebSocket"] {
        let mut request = format!("ws://{address}{PATH}")
            .into_client_request()
            .unwrap();
        assert_eq!(request.headers().get("origin"), None);
        let headers = request.headers_mut();
        headers.insert(
            "host",
            axum::http::HeaderValue::from_static("hub.other.example:8443"),
        );
        headers.insert("upgrade", axum::http::HeaderValue::from_static(upgrade));
        let mut socket = connect_async(request).await.unwrap().0;
        socket.send(connect(0)).await.unwrap();
        let Some(Message::Text(text)) = next(&mut socket).await else {
            panic!("no answer upgrading to {upgrade}");
        };
        assert!(matches!(
            HubFrame::from_text(text.as_str()),
            Ok(HubFrame::Challenge { .. })
        ));
    }
}

/// An agent's handshake has no body; one that does is refused before the
/// handshake, whatever its length, and when it has none it declares.
#[tokio::test]
async fn a_handshake_with_a_body_is_refused_before_it_starts() {
    let streamed = || {
        let chunks = [Ok::<_, std::io::Error>("{}".as_bytes())];
        axum::body::Body::from_stream(futures::stream::iter(chunks))
    };
    for (what, body) in [
        ("one byte", axum::body::Body::from("x")),
        ("JSON", axum::body::Body::from("{}")),
        ("of no known length", streamed()),
    ] {
        let mut request = handshake("GET", PATH, &[]);
        *request.body_mut() = body;
        let response = send(request).await;
        assert_eq!(
            crate::request_checks::tests::code(response).await,
            Some((StatusCode::PAYLOAD_TOO_LARGE, "body-too-large".to_owned())),
            "a body {what}"
        );
    }
}

/// A web page's request is refused as one whatever else is wrong with it:
/// the `Origin` is checked first.
#[tokio::test]
async fn the_origin_is_checked_before_the_method_and_the_upgrade() {
    let origin = ("origin", "http://evil.example");
    let without_upgrade =
        crate::request_checks::tests::request("GET", PATH, &[("host", "localhost:9898"), origin]);
    for (what, request) in [
        ("a POST", handshake("POST", PATH, &[origin])),
        ("a GET asking for no upgrade", without_upgrade),
    ] {
        assert_eq!(
            crate::request_checks::tests::code(send(request).await).await,
            Some((StatusCode::FORBIDDEN, "origin-not-allowed".to_owned())),
            "{what}"
        );
    }
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

/// An agent's link over a plain WebSocket to `address`, whatever the route,
/// as one with no TLS of the hub's own carries its frames.
struct Plain(String);

struct PlainLink(Socket);

#[async_trait]
impl app::ports::HubConnector for Plain {
    async fn connect(
        &self,
        _route: &domain::fleet::HubRoute,
    ) -> Result<Box<dyn app::ports::HubLink>, app::ports::LinkError> {
        Ok(Box::new(PlainLink(open(&self.0).await)))
    }
}

#[async_trait]
impl app::ports::HubLink for PlainLink {
    fn exporter(&self) -> Option<[u8; domain::fleet::KEY_BYTES]> {
        None
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), app::ports::LinkError> {
        self.0
            .send(Message::text(frame.to_text()))
            .await
            .map_err(|_| app::ports::LinkError::Closed { reason: None })
    }

    async fn receive(&mut self) -> Result<HubFrame, app::ports::LinkError> {
        loop {
            return match self.0.next().await {
                Some(Ok(Message::Text(text))) => HubFrame::from_text(text.as_str())
                    .map_err(|_| app::ports::LinkError::Unreadable),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                _ => Err(app::ports::LinkError::Closed { reason: None }),
            };
        }
    }
}

/// A hub that keeps reports, served on loopback under `bounds`, and an
/// agent it approved: what connects that agent, its ID and its hostname.
async fn approved_agent(
    bounds: Bounds,
) -> (
    app::pairing::ConnectToHub,
    domain::AgentId,
    domain::fleet::Hostname,
) {
    use app::pairing::{AgentPairing, ConnectToHub, PairWithHub};
    use app::ports::AgentRegistry as _;
    use domain::fleet::{Hostname, HubAddress, TokenTls};
    use domain::{AgentId, Uuid};

    let registry = Arc::new(InMemoryAgentRegistry::new());
    let pairing = Arc::new(
        HubPairing::new(
            registry.clone(),
            Arc::new(InMemoryIdentity::default()),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(5)),
            Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).unwrap(),
            )),
            "0.0.0-test".to_owned(),
        )
        .with_inbox(Arc::new(app::fakes::InMemoryReportInbox::default())),
    );
    let address = serve(&SyncHub::bounded(pairing.clone(), bounds)).await;
    let keys = pairing.keys().await.unwrap();
    let at = HubAddress::parse(&format!("http://{address}")).unwrap();
    let token = pairing
        .make_token(&keys, at, TokenTls::Ca, None)
        .await
        .unwrap();
    let agent = AgentId::from_uuid(Uuid::from_u128(1));
    let hostname = Hostname::parse("laptop").unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let connector = Arc::new(Plain(address));
    let machine = AgentPairing::new(
        identity.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(9)),
    );
    PairWithHub::new(machine, connector.clone())
        .pair(token, agent, hostname.clone(), None)
        .await
        .unwrap();
    let waiting = registry.agent(agent).await.unwrap().unwrap();
    pairing.approve(&waiting).await.unwrap().unwrap();
    let connecting = ConnectToHub::new(
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        connector,
        "0.0.0-agent".to_owned(),
    );
    (connecting, agent, hostname)
}

/// A report of a start at `seq` of one epoch.
fn report(seq: u64) -> app::ports::OutboxEntry {
    use domain::fleet::{AgentReport, EpochSeconds, Happening, OutboxEpoch, ReportSeq};
    use domain::{OperationId, Uuid};

    app::ports::OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(seq).unwrap(),
        report: AgentReport {
            op_id: OperationId::from_uuid(Uuid::from_u128(0x0b)),
            happening: Happening::Started,
            at: EpochSeconds::from_seconds(1_800_000_000),
        },
    }
}

/// Once the hub welcomed an approved agent, the connection leaves the
/// handshake's bound: a report sent after it is acknowledged. It is closed
/// once the agent sends nothing for the idle bound (§7.2).
#[tokio::test]
async fn a_syncing_connection_outlives_the_handshake_bound_until_it_is_idle() {
    use domain::fleet::{Ack, Approval};

    let handshake = Duration::from_millis(300);
    let idle = Duration::from_millis(900);
    let bounds = Bounds {
        handshake_within: handshake,
        idle_within: idle,
        ..Bounds::DECIDED
    };
    let (connecting, agent, hostname) = approved_agent(bounds).await;
    let (connected, mut channel) = connecting
        .connect_channel(agent, hostname, None)
        .await
        .unwrap();
    assert_eq!(connected.approval, Approval::Approved);
    tokio::time::sleep(handshake * 2).await;
    let entry = report(1);
    let started = std::time::Instant::now();
    channel.send_report(&entry).await.unwrap();
    assert_eq!(
        channel.receive_ack().await.unwrap(),
        Ack {
            epoch: entry.epoch,
            seq: entry.seq,
        }
    );
    let closed = tokio::time::timeout(idle + PATIENCE, channel.receive_ack()).await;
    assert!(matches!(closed, Ok(Err(_))), "{closed:?}");
    let took = started.elapsed();
    assert!(
        took >= idle && took <= idle + MARGIN,
        "closed after {took:?}, not at its {idle:?} idle bound"
    );
}

/// One live connection per agent ID (decision 120): an agent's new session
/// closes its older one, which takes no report after it, and the older
/// one's end leaves the newer one live, so the next session closes it.
#[tokio::test]
async fn a_new_session_of_an_agent_closes_its_older_one() {
    use domain::fleet::Ack;

    let (connecting, agent, hostname) = approved_agent(Bounds::DECIDED).await;
    let (_, mut older) = connecting
        .connect_channel(agent, hostname.clone(), None)
        .await
        .unwrap();
    let (_, mut newer) = connecting
        .connect_channel(agent, hostname.clone(), None)
        .await
        .unwrap();
    // The send may fail or not, depending on when the close arrives; no
    // `Ack` answers it either way.
    let _ = older.send_report(&report(1)).await;
    let answer = tokio::time::timeout(PATIENCE, older.receive_ack()).await;
    assert!(matches!(answer, Ok(Err(_))), "{answer:?}");
    drop(older);
    // The older connection's end has run before the newer one sends.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let entry = report(2);
    newer.send_report(&entry).await.unwrap();
    assert_eq!(
        newer.receive_ack().await.unwrap(),
        Ack {
            epoch: entry.epoch,
            seq: entry.seq,
        }
    );
    let (_, _newest) = connecting
        .connect_channel(agent, hostname, None)
        .await
        .unwrap();
    let _ = newer.send_report(&report(3)).await;
    let answer = tokio::time::timeout(PATIENCE, newer.receive_ack()).await;
    assert!(matches!(answer, Ok(Err(_))), "{answer:?}");
}
