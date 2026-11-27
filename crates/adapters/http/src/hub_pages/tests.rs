use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use app::access::Access;
use app::fakes::{
    FakeClock, FakeSigning, InMemoryAdminPassword, InMemoryAgentRegistry, InMemoryApiTokenStore,
    InMemoryIdentity, InMemoryOwnershipRegistry, InMemoryReportInbox, InMemorySessionStore,
    SeededRandom,
};
use app::ownership::RepositoryOwners;
use app::pairing::HubPairing;
use app::ports::{AgentRegistry as _, OutboxEntry, ReportInbox as _, Signing as _};
use axum::body::Body;
use axum::http::header::{HOST, SET_COOKIE};
use axum::http::{Method, StatusCode};
use domain::access::api_token::{ApiTokenLabel, TokenPermissions};
use domain::access::permissions::{Permission, Permissions};
use domain::fleet::{
    AgentRecord, AgentReport, AgentStatus, EpochSeconds, Fingerprint, Happening, Hostname,
    MachineFingerprint, MachineVersion, OutboxEpoch, ReportSeq, SigningSeed, WireName,
};
use domain::{AgentId, OperationId, Timestamp, Uuid};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, SignIn, SyncHub, router};

const PASSWORD: &str = "demo-admin-pass";
const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const REMOTE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
const ADDRESS: &str = "https://hub.example:9443";
/// The certificate of the hub's built-in TLS, when a test's hub serves it.
const CERTIFICATE: Fingerprint = Fingerprint::from_digest([0x3c; 32]);

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ))
}

/// A daemon serving as a hub, with sign-in on when `access` is given, and
/// its built-in TLS with `certificate` when that is given.
struct Daemon {
    pairing: Option<Arc<HubPairing>>,
    registry: Arc<InMemoryAgentRegistry>,
    access: Option<Arc<Access>>,
    certificate: Option<Fingerprint>,
    inbox: Arc<InMemoryReportInbox>,
    /// The registry of maintenance owners the hub decides leases in.
    owners: Arc<InMemoryOwnershipRegistry>,
}

impl Daemon {
    fn hub(sign_in: bool) -> Self {
        let access = sign_in.then(|| {
            Arc::new(Access::new(
                Arc::new(InMemoryAdminPassword::with(PASSWORD)),
                Arc::new(SeededRandom::new(7)),
                clock(),
                Arc::new(InMemoryApiTokenStore::new()),
                Arc::new(InMemorySessionStore::new()),
            ))
        });
        let registry = Arc::new(InMemoryAgentRegistry::new());
        let pairing = HubPairing::new(
            Arc::clone(&registry) as _,
            Arc::new(InMemoryIdentity::default()),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(5)),
            clock(),
            "0.5.0-hub".to_owned(),
        );
        let inbox = Arc::new(InMemoryReportInbox::default());
        let pairing = pairing.with_inbox(Arc::clone(&inbox) as _);
        let owners = Arc::new(InMemoryOwnershipRegistry::new());
        let pairing = pairing.with_owners(Arc::new(RepositoryOwners::new(
            Arc::clone(&registry) as _,
            Arc::clone(&owners) as _,
            clock(),
        )));
        let pairing = match &access {
            Some(access) => pairing.with_access(Arc::clone(access)),
            None => pairing,
        };
        Self {
            pairing: Some(Arc::new(pairing)),
            registry,
            access,
            certificate: None,
            inbox,
            owners,
        }
    }

    /// A hub with sign-in off serving its built-in TLS.
    fn hub_on_tls() -> Self {
        Self {
            certificate: Some(CERTIFICATE),
            ..Self::hub(false)
        }
    }

    fn not_a_hub() -> Self {
        Self {
            pairing: None,
            registry: Arc::new(InMemoryAgentRegistry::new()),
            access: None,
            certificate: None,
            inbox: Arc::new(InMemoryReportInbox::default()),
            owners: Arc::new(InMemoryOwnershipRegistry::new()),
        }
    }

    async fn send(&self, request: Request) -> Response {
        let mut state = test_state();
        if let Some(access) = &self.access {
            state.sign_in = SignIn::On(Arc::clone(access));
        }
        if let Some(pairing) = &self.pairing {
            state.sync = SyncHub::serving(Arc::clone(pairing));
        }
        if let Some(certificate) = self.certificate {
            state.sync = state.sync.with_certificate(certificate);
        }
        router(
            Arc::new(NoUi),
            RequestChecks::new(["hub.example".to_owned()]),
            state,
        )
        .oneshot(request)
        .await
        .unwrap()
    }

    /// The hub key's fingerprint, which confirms with sign-in off.
    async fn fingerprint(&self) -> String {
        let key = self.pairing.as_ref().unwrap().hub_key().await.unwrap();
        Fingerprint::of_key(&key).to_string()
    }

    /// The hub key's key ID, which confirmed with sign-in off before the
    /// fingerprint did (#1344): the first 8 hex digits of its SHA-256.
    async fn former_key_id(&self) -> String {
        let key = self.pairing.as_ref().unwrap().hub_key().await.unwrap();
        let digest = Fingerprint::of_key(&key).full_hex();
        digest.get(..8).unwrap().to_uppercase()
    }

    /// A session's `Cookie` header and its CSRF token.
    async fn signed_in(&self) -> [(&'static str, String); 2] {
        let body = serde_json::json!({ "password": PASSWORD }).to_string();
        let response = self
            .send(build(Method::POST, "/api/v1/session", LOOPBACK, &[], &body))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[SET_COOKIE].to_str().unwrap().to_owned();
        let pair = cookie.split(';').next().unwrap().to_owned();
        let body = json(response).await;
        [
            ("cookie", pair),
            (
                "x-csrf-token",
                body["csrf-token"].as_str().unwrap().to_owned(),
            ),
        ]
    }
}

fn build(
    method: Method,
    path: &str,
    peer: IpAddr,
    headers: &[(&str, String)],
    body: &str,
) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, "127.0.0.1:8080")
        .header("content-type", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    let mut request = builder.body(Body::from(body.to_owned())).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(crate::PeerAddr(SocketAddr::new(peer, 50_000))));
    request
}

/// `request` as it comes over the hub's built-in TLS.
fn over_tls(mut request: Request) -> Request {
    request.extensions_mut().insert(crate::BuiltInTls {
        exporter: Some([9; 32]),
    });
    request
}

fn ask(confirmation: &str) -> String {
    serde_json::json!({ "address": ADDRESS, "confirmation": confirmation }).to_string()
}

fn make(peer: IpAddr, headers: &[(&str, String)], body: &str) -> Request {
    build(Method::POST, api::PAIRING_TOKENS_PATH, peer, headers, body)
}

async fn json(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

async fn refusal(response: Response) -> (StatusCode, String) {
    let status = response.status();
    let body = json(response).await;
    (status, body["code"].as_str().unwrap_or_default().to_owned())
}

#[tokio::test]
async fn the_hub_answers_its_keys_fingerprint_in_full_and_no_other_id() {
    let daemon = Daemon::hub(false);
    let response = daemon
        .send(build(Method::GET, api::HUB_PATH, LOOPBACK, &[], ""))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let key = daemon.pairing.as_ref().unwrap().hub_key().await.unwrap();
    let read = json(response).await;
    assert_eq!(
        read,
        serde_json::json!({
            "fingerprint": Fingerprint::of_key(&key).to_string(),
            "certificate": "ca",
        })
    );
}

/// A page that came over the hub's built-in TLS is offered the hub's own
/// certificate first; one that came in plain, as through a proxy, a
/// certificate authority's. Both are told the certificate's fingerprint.
#[tokio::test]
async fn the_page_is_offered_first_the_certificate_it_came_in_by() {
    let daemon = Daemon::hub_on_tls();
    for (request, offered) in [
        (
            over_tls(build(Method::GET, api::HUB_PATH, LOOPBACK, &[], "")),
            api::TokenCertificateDto::Pinned,
        ),
        (
            build(Method::GET, api::HUB_PATH, LOOPBACK, &[], ""),
            api::TokenCertificateDto::Ca,
        ),
    ] {
        let response = daemon.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let hub: api::HubDto = serde_json::from_value(json(response).await).unwrap();
        assert_eq!(hub.certificate, offered);
        assert_eq!(hub.own_certificate, Some(CERTIFICATE.to_string()));
    }
}

/// A pinned ask on a hub serving its built-in TLS makes a token that pins
/// that certificate, and says so; a certificate authority's ask, one that
/// trusts the system's.
#[tokio::test]
async fn a_token_pins_the_hubs_certificate_when_asked_to() {
    let daemon = Daemon::hub_on_tls();
    let fingerprint = daemon.fingerprint().await;
    for (kind, tls, said) in [
        (
            "pinned",
            domain::fleet::TokenTls::Pinned(CERTIFICATE),
            api::TokenCertificateDto::Pinned,
        ),
        (
            "ca",
            domain::fleet::TokenTls::Ca,
            api::TokenCertificateDto::Ca,
        ),
    ] {
        let body = serde_json::json!({
            "address": ADDRESS,
            "certificate": kind,
            "confirmation": fingerprint,
        })
        .to_string();
        let response = daemon.send(make(LOOPBACK, &[], &body)).await;
        assert_eq!(response.status(), StatusCode::CREATED, "{kind}");
        let made: api::PairingTokenDto = serde_json::from_value(json(response).await).unwrap();
        assert_eq!(made.certificate, Some(said));
        let token = domain::fleet::PairingToken::parse(&made.token).unwrap();
        assert_eq!(token.tls(), tls);
    }
}

/// A pinned certificate is served over TLS, so a pinned ask for a plain
/// `http://` address is refused as an address no machine could use.
#[tokio::test]
async fn a_pinned_ask_for_a_plain_address_is_refused() {
    let daemon = Daemon::hub_on_tls();
    let fingerprint = daemon.fingerprint().await;
    let body = serde_json::json!({
        "address": "http://127.0.0.1:9",
        "certificate": "pinned",
        "confirmation": fingerprint,
    })
    .to_string();
    let response = daemon.send(make(LOOPBACK, &[], &body)).await;
    assert_eq!(
        refusal(response).await,
        (StatusCode::BAD_REQUEST, "hub-address-invalid".to_owned())
    );
}

/// A plain `http://` address shows no certificate, so the token made for
/// it names none, even on a hub serving its built-in TLS (#1343).
#[tokio::test]
async fn a_token_for_a_plain_address_names_no_certificate() {
    let daemon = Daemon::hub_on_tls();
    let fingerprint = daemon.fingerprint().await;
    let body = serde_json::json!({
        "address": "http://127.0.0.1:9",
        "confirmation": fingerprint,
    })
    .to_string();
    let response = daemon.send(make(LOOPBACK, &[], &body)).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let made = json(response).await;
    assert_eq!(made.get("certificate"), None, "{made}");
    assert_eq!(made["address"], "http://127.0.0.1:9");
}

#[tokio::test]
async fn a_daemon_not_serving_as_a_hub_answers_its_routes_as_unknown() {
    let daemon = Daemon::not_a_hub();
    for (method, path) in [
        (Method::GET, api::HUB_PATH.to_owned()),
        (Method::GET, api::HUB_RECOVERY_PATH.to_owned()),
        (Method::POST, api::HUB_RECOVERY_CONFIRMATION_PATH.to_owned()),
        (Method::POST, api::PAIRING_TOKENS_PATH.to_owned()),
        (
            Method::DELETE,
            format!("{}/0000000000000000", api::PAIRING_TOKENS_PATH),
        ),
    ] {
        let response = daemon
            .send(build(method.clone(), &path, LOOPBACK, &[], &ask("x")))
            .await;
        assert_eq!(
            refusal(response).await,
            (StatusCode::NOT_FOUND, "route-unknown".to_owned()),
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn on_loopback_the_fingerprint_makes_a_token_shown_once_and_canceled_once() {
    let daemon = Daemon::hub(false);
    let fingerprint = daemon.fingerprint().await.to_lowercase();
    let response = daemon.send(make(LOOPBACK, &[], &ask(&fingerprint))).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let made: api::PairingTokenDto = serde_json::from_value(json(response).await).unwrap();
    let key = daemon.pairing.as_ref().unwrap().hub_key().await.unwrap();
    assert_eq!(made.fingerprint, Fingerprint::of_key(&key).to_string());
    assert_eq!(made.address, ADDRESS);
    assert_eq!(made.seconds_left, 15 * 60);
    assert_eq!(made.expires_at, "2027-01-15T08:15:00Z");
    let token = domain::fleet::PairingToken::parse(&made.token).expect("the token reads");
    assert_eq!(token.hub_key(), &key);
    // No certificate of the hub's own is served yet, so none is pinned: the
    // token trusts a certificate authority's (decision 121, "The hub's
    // dialog").
    assert_eq!(token.tls(), domain::fleet::TokenTls::Ca);
    let cancel = || {
        build(
            Method::DELETE,
            &format!("{}/{}", api::PAIRING_TOKENS_PATH, made.token_id),
            LOOPBACK,
            &[],
            "",
        )
    };
    assert_eq!(daemon.send(cancel()).await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        refusal(daemon.send(cancel()).await).await,
        (StatusCode::NOT_FOUND, "pairing-token-unknown".to_owned())
    );
}

#[tokio::test]
async fn beyond_loopback_with_sign_in_off_no_token_is_made() {
    let daemon = Daemon::hub(false);
    let fingerprint = daemon.fingerprint().await;
    let remote_peer = make(REMOTE, &[], &ask(&fingerprint));
    let mut remote_host = make(LOOPBACK, &[], &ask(&fingerprint));
    remote_host
        .headers_mut()
        .insert(HOST, "hub.example:8080".parse().unwrap());
    for request in [remote_peer, remote_host] {
        assert_eq!(
            refusal(daemon.send(request).await).await,
            (StatusCode::FORBIDDEN, "pairing-needs-sign-in".to_owned())
        );
    }
}

// Beyond loopback with sign-in off, no confirmation can help: the person
// sets an admin password first, whatever was typed, nothing or too much
// included.
#[tokio::test]
async fn beyond_loopback_with_sign_in_off_any_confirmation_needs_sign_in() {
    let daemon = Daemon::hub(false);
    let oversized = "a".repeat(domain::access::PASSWORD_MAX_BYTES + 1);
    for confirmation in ["", oversized.as_str(), "00000000"] {
        assert_eq!(
            refusal(daemon.send(make(REMOTE, &[], &ask(confirmation))).await).await,
            (StatusCode::FORBIDDEN, "pairing-needs-sign-in".to_owned()),
            "confirmed by {} bytes",
            confirmation.len()
        );
    }
}

#[tokio::test]
async fn a_wrong_fingerprint_or_the_key_id_is_a_step_up_that_failed() {
    let daemon = Daemon::hub(false);
    let wrong = if daemon.fingerprint().await == "0000-0000-0000-0000-0000" {
        "1111-1111-1111-1111-1111"
    } else {
        "0000-0000-0000-0000-0000"
    };
    for confirmation in [wrong.to_owned(), daemon.former_key_id().await] {
        assert_eq!(
            refusal(daemon.send(make(LOOPBACK, &[], &ask(&confirmation))).await).await,
            (StatusCode::FORBIDDEN, "step-up-failed".to_owned()),
            "{confirmation}"
        );
    }
}

// Nothing typed, or more than any password, is no confirmation: refused as
// a wrong one, with sign-in off and on, and in no sign-in wait, so the
// password from the same address still confirms at once.
#[tokio::test]
async fn an_empty_or_oversized_confirmation_is_a_step_up_that_failed_in_no_wait() {
    let oversized = "a".repeat(domain::access::PASSWORD_MAX_BYTES + 1);
    let signed_off = Daemon::hub(false);
    let signed_on = Daemon::hub(true);
    let session = signed_on.signed_in().await;
    for confirmation in ["", oversized.as_str()] {
        assert_eq!(
            refusal(
                signed_off
                    .send(make(LOOPBACK, &[], &ask(confirmation)))
                    .await
            )
            .await,
            (StatusCode::FORBIDDEN, "step-up-failed".to_owned()),
            "sign-in off, confirmed by {} bytes",
            confirmation.len()
        );
        assert_eq!(
            refusal(
                signed_on
                    .send(make(REMOTE, &session, &ask(confirmation)))
                    .await
            )
            .await,
            (StatusCode::FORBIDDEN, "step-up-failed".to_owned()),
            "sign-in on, confirmed by {} bytes",
            confirmation.len()
        );
    }
    let made = signed_on.send(make(REMOTE, &session, &ask(PASSWORD))).await;
    assert_eq!(made.status(), StatusCode::CREATED);
}

// With sign-in off, a request `ladderback mcp` marks as an AI agent's opens
// no hub route: each is a session's only, and the refusal names it.
#[tokio::test]
async fn with_sign_in_off_an_ai_agent_opens_no_hub_route() {
    let daemon = Daemon::hub(false);
    let fingerprint = daemon.fingerprint().await;
    let made = daemon.send(make(LOOPBACK, &[], &ask(&fingerprint))).await;
    assert_eq!(made.status(), StatusCode::CREATED);
    let made: api::PairingTokenDto = serde_json::from_value(json(made).await).unwrap();
    let agent = [(api::AI_AGENT_HEADER, "1".to_owned())];
    let cancel_path = format!("{}/{}", api::PAIRING_TOKENS_PATH, made.token_id);
    for request in [
        make(LOOPBACK, &agent, &ask(&fingerprint)),
        build(Method::DELETE, &cancel_path, LOOPBACK, &agent, ""),
        build(Method::GET, api::HUB_PATH, LOOPBACK, &agent, ""),
    ] {
        let route = format!("{} {}", request.method(), request.uri());
        let response = daemon.send(request).await;
        let status = response.status();
        let body = json(response).await;
        assert_eq!(
            (status, body["code"].as_str(), &body["args"]),
            (
                StatusCode::FORBIDDEN,
                Some("without-token-permission"),
                &serde_json::json!({ "permission": { "type": "text", "value": "session" } })
            ),
            "{route}"
        );
    }
    let cancel = build(Method::DELETE, &cancel_path, LOOPBACK, &[], "");
    assert_eq!(daemon.send(cancel).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn with_sign_in_on_a_session_confirms_with_the_password_from_anywhere() {
    let daemon = Daemon::hub(true);
    let session = daemon.signed_in().await;
    // A wrong try counts in its address's sign-in waits, so each try comes
    // from an address of its own.
    let peer = |last| IpAddr::V4(Ipv4Addr::new(192, 0, 2, last));
    let wrong = daemon.send(make(peer(11), &session, &ask("not-it"))).await;
    assert_eq!(
        refusal(wrong).await,
        (StatusCode::FORBIDDEN, "step-up-failed".to_owned())
    );
    let again = daemon.send(make(peer(11), &session, &ask(PASSWORD))).await;
    assert_eq!(
        refusal(again).await,
        (StatusCode::TOO_MANY_REQUESTS, "sign-in-wait".to_owned())
    );
    let fingerprint = daemon.fingerprint().await;
    let by_fingerprint = daemon
        .send(make(peer(12), &session, &ask(&fingerprint)))
        .await;
    assert_eq!(
        refusal(by_fingerprint).await,
        (StatusCode::FORBIDDEN, "step-up-failed".to_owned())
    );
    let made = daemon.send(make(REMOTE, &session, &ask(PASSWORD))).await;
    assert_eq!(made.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_api_token_with_every_permission_cant_make_or_cancel_a_token() {
    let daemon = Daemon::hub(true);
    let session = daemon.signed_in().await;
    let made = daemon.send(make(LOOPBACK, &session, &ask(PASSWORD))).await;
    assert_eq!(made.status(), StatusCode::CREATED);
    let made: api::PairingTokenDto = serde_json::from_value(json(made).await).unwrap();
    let created = daemon
        .access
        .as_ref()
        .unwrap()
        .create_api_token(
            ApiTokenLabel::new("assistant".to_owned()).unwrap(),
            TokenPermissions::new(Permission::ALL.iter().fold(Permissions::NONE, |held, row| {
                held.with(*row, Some(row.level())).unwrap()
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let bearer = [("authorization", format!("Bearer {}", created.text.expose()))];
    let cancel_path = format!("{}/{}", api::PAIRING_TOKENS_PATH, made.token_id);
    for request in [
        make(LOOPBACK, &bearer, &ask(PASSWORD)),
        build(Method::DELETE, &cancel_path, LOOPBACK, &bearer, ""),
        build(Method::GET, api::HUB_PATH, LOOPBACK, &bearer, ""),
    ] {
        assert_eq!(
            refusal(daemon.send(request).await).await,
            (StatusCode::FORBIDDEN, "api-token-permission".to_owned())
        );
    }
    // The token survived the refused cancel: the session still cancels it.
    let cancel = build(Method::DELETE, &cancel_path, LOOPBACK, &session, "");
    assert_eq!(daemon.send(cancel).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn an_ask_the_hub_cant_read_is_refused_by_its_kind() {
    let daemon = Daemon::hub(false);
    let fingerprint = daemon.fingerprint().await;
    let cases = [
        (
            r#"{"address":1}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
        (
            serde_json::json!({ "address": "ftp://hub", "confirmation": fingerprint }).to_string(),
            StatusCode::BAD_REQUEST,
            "hub-address-invalid",
        ),
        (
            serde_json::json!({ "address": ADDRESS, "life-minutes": 4, "confirmation": fingerprint })
                .to_string(),
            StatusCode::BAD_REQUEST,
            "pairing-token-life",
        ),
        // The certificate's kind isn't the page's to choose while the hub
        // serves no certificate of its own: a pinned one is no ask it reads.
        (
            serde_json::json!({ "address": ADDRESS, "certificate": "pinned", "confirmation": fingerprint })
                .to_string(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
    ];
    for (body, status, code) in cases {
        assert_eq!(
            refusal(daemon.send(make(LOOPBACK, &[], &body)).await).await,
            (status, code.to_owned()),
            "{body}"
        );
    }
}

#[test]
fn a_loopback_peer_and_host_alone_are_on_loopback() {
    let host = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, value.parse().unwrap());
        headers
    };
    assert!(!beyond_loopback(&host("127.0.0.1:8080"), LOOPBACK));
    assert!(!beyond_loopback(&host("localhost:8080"), LOOPBACK));
    assert!(!beyond_loopback(
        &host("[::1]:8080"),
        "::1".parse().unwrap()
    ));
    assert!(beyond_loopback(&host("127.0.0.1:8080"), REMOTE));
    assert!(beyond_loopback(&host("hub.example:8080"), LOOPBACK));
    assert!(beyond_loopback(&HeaderMap::new(), LOOPBACK));
}

/// An agent the hub holds, `n` its agent ID's and key's seed, approved at
/// or pending since `at`.
fn agent(n: u8, hostname: &str, approved: bool, at: Timestamp) -> AgentRecord {
    AgentRecord {
        agent_id: AgentId::from_uuid(Uuid::from_bytes([n; 16])),
        key: FakeSigning
            .public_key(&SigningSeed::from_bytes([n; 32]))
            .unwrap(),
        hostname: Hostname::parse(hostname).unwrap(),
        status: if approved {
            AgentStatus::Approved { at }
        } else {
            AgentStatus::Pending { since: at }
        },
        counter: 1,
        last_seen: at,
        zone: None,
        version: None,
    }
}

fn approve(
    peer: IpAddr,
    headers: &[(&str, String)],
    waiting: &AgentRecord,
    confirmation: &str,
) -> Request {
    let body = serde_json::json!({ "key": waiting.key.to_hex(), "confirmation": confirmation });
    let path = api::machine_approval_path(&waiting.agent_id.as_uuid().to_string());
    build(Method::POST, &path, peer, headers, &body.to_string())
}

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

#[tokio::test]
async fn the_page_lists_each_agent_with_its_key_its_wait_its_lookalike_and_a_second_computer() {
    let daemon = Daemon::hub(false);
    let earlier = Timestamp::from_second(1_799_999_400).unwrap();
    let approved = agent(1, "macbook", true, earlier);
    let waiting = agent(2, "rnacbook", false, earlier);
    daemon.registry.hold(approved.clone());
    daemon.registry.hold(waiting.clone());
    // The hub took one computer under the waiting agent's ID and refused
    // a second.
    for computer in ["a", "b"] {
        let fingerprint = MachineFingerprint::from_hex(&computer.repeat(64)).unwrap();
        daemon
            .registry
            .check_machine(waiting.agent_id, Some(&fingerprint), earlier)
            .await
            .unwrap();
    }
    let response = daemon
        .send(build(
            Method::GET,
            api::HUB_MACHINES_PATH,
            LOOPBACK,
            &[],
            "",
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let listed: api::HubAgentsDto = serde_json::from_value(json(response).await).unwrap();
    let expected = |record: &AgentRecord, status, looks_like| api::HubAgentDto {
        machine_id: record.agent_id.as_uuid().to_string(),
        hostname: record.hostname.as_str().to_owned(),
        key: record.key.to_hex(),
        fingerprint: Fingerprint::of_key(&record.key).to_string(),
        status,
        since: earlier.to_string(),
        seconds_ago: 600,
        looks_like,
        second_machine_at: None,
    };
    let mut flagged = expected(
        &waiting,
        api::AgentStatusDto::Pending,
        Some(api::LookalikeDto {
            machine_id: approved.agent_id.as_uuid().to_string(),
            hostname: "macbook".to_owned(),
            last_seen: earlier.to_string(),
        }),
    );
    flagged.second_machine_at = Some(earlier.to_string());
    assert_eq!(
        listed.machines,
        [
            expected(&approved, api::AgentStatusDto::Approved, None),
            flagged,
        ]
    );
}

/// The dashboard's route: each approved machine with its latest
/// operations, newest first, a failure's code, a cancel and the count let
/// go of; no waiting machine.
#[tokio::test]
async fn the_page_shows_each_approved_machine_with_its_latest_operations() {
    let daemon = Daemon::hub(false);
    let earlier = Timestamp::from_second(1_799_999_400).unwrap();
    let approved = agent(1, "macbook", true, earlier);
    daemon.registry.hold(approved.clone());
    daemon.registry.hold(agent(2, "laptop", false, earlier));
    let failed = Happening::Failed {
        code: WireName::parse("repository-unreachable").unwrap(),
    };
    for (seq, n, happening, at) in [
        (1, 1, Happening::Started, 1_799_999_000),
        (2, 1, Happening::Succeeded, 1_799_999_100),
        (3, 2, failed, 1_799_999_200),
        (4, 3, Happening::Canceled, 1_799_999_300),
    ] {
        let entry = OutboxEntry {
            epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
            seq: ReportSeq::new(seq).unwrap(),
            report: AgentReport {
                op_id: OperationId::from_uuid(Uuid::from_u128(n)),
                happening,
                at: EpochSeconds::from_seconds(at),
            },
        };
        daemon
            .inbox
            .ingest(approved.agent_id, &entry, now())
            .await
            .unwrap();
    }
    let response = daemon
        .send(build(
            Method::GET,
            api::HUB_DASHBOARD_PATH,
            LOOPBACK,
            &[],
            "",
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let shown: api::HubMachinesDto = serde_json::from_value(json(response).await).unwrap();
    let time = |second| Some(Timestamp::from_second(second).unwrap().to_string());
    assert_eq!(
        shown.machines,
        [api::HubMachineDto {
            machine_id: approved.agent_id.as_uuid().to_string(),
            hostname: "macbook".to_owned(),
            last_seen: approved.last_seen.to_string(),
            online: false,
            zone: None,
            version: None,
            reports_dropped: 0,
            operations: vec![
                api::MachineOperationDto {
                    operation: Uuid::from_u128(3).to_string(),
                    started_at: None,
                    ended_at: time(1_799_999_300),
                    outcome: api::MachineOutcomeDto::Canceled,
                    code: None,
                },
                api::MachineOperationDto {
                    operation: Uuid::from_u128(2).to_string(),
                    started_at: None,
                    ended_at: time(1_799_999_200),
                    outcome: api::MachineOutcomeDto::Failed,
                    code: Some("repository-unreachable".to_owned()),
                },
                api::MachineOperationDto {
                    operation: Uuid::from_u128(1).to_string(),
                    started_at: time(1_799_999_000),
                    ended_at: time(1_799_999_100),
                    outcome: api::MachineOutcomeDto::Succeeded,
                    code: None,
                },
            ],
            plans: None,
            progress: None,
        }]
    );
}

/// The dashboard's route tells each machine's version as it wrote it, and
/// whether it is older or newer than the hub's own (`0.5.0-hub` here): no
/// note for the hub's release, whatever its build, nor for a version that
/// doesn't read as SemVer, and no version before the machine gave one
/// (#1707).
#[tokio::test]
async fn the_page_shows_each_machines_version_and_how_it_stands_against_the_hubs() {
    let daemon = Daemon::hub(false);
    let earlier = Timestamp::from_second(1_799_999_400).unwrap();
    let versions = [
        Some("0.4.9"),
        Some("0.5.0-hub+3f937cf"),
        Some("0.6.0"),
        Some("nightly"),
        None,
    ];
    for (n, version) in (1..).zip(versions) {
        daemon.registry.hold(AgentRecord {
            version: version.map(|text| MachineVersion::parse(text).unwrap()),
            ..agent(n, &format!("laptop-{n}"), true, earlier)
        });
    }
    let response = daemon
        .send(build(
            Method::GET,
            api::HUB_DASHBOARD_PATH,
            LOOPBACK,
            &[],
            "",
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let shown: api::HubMachinesDto = serde_json::from_value(json(response).await).unwrap();
    let told = |version: &str, against_hub| {
        Some(api::MachineVersionDto {
            version: version.to_owned(),
            against_hub,
        })
    };
    assert_eq!(
        shown
            .machines
            .into_iter()
            .map(|machine| machine.version)
            .collect::<Vec<_>>(),
        [
            told("0.4.9", Some(api::AgainstHubDto::Older)),
            told("0.5.0-hub+3f937cf", None),
            told("0.6.0", Some(api::AgainstHubDto::Newer)),
            told("nightly", None),
            None,
        ]
    );
}

#[tokio::test]
async fn on_loopback_the_fingerprint_approves_the_agent_under_the_key_compared_once() {
    let daemon = Daemon::hub(false);
    let waiting = agent(2, "laptop", false, now());
    // The hub key is made while no agent is registered, as on a real hub.
    let fingerprint = daemon.fingerprint().await;
    daemon.registry.hold(waiting.clone());
    let response = daemon
        .send(approve(LOOPBACK, &[], &waiting, &fingerprint))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let answered: api::HubAgentDto = serde_json::from_value(json(response).await).unwrap();
    assert_eq!(answered.status, api::AgentStatusDto::Approved);
    assert_eq!(answered.hostname, "laptop");
    assert_eq!(answered.key, waiting.key.to_hex());
    let kept = daemon.registry.agent(waiting.agent_id).await.unwrap();
    assert!(kept.is_some_and(|kept| !kept.is_pending()));
    let again = daemon
        .send(approve(LOOPBACK, &[], &waiting, &fingerprint))
        .await;
    assert_eq!(
        refusal(again).await,
        (StatusCode::CONFLICT, "machine-approved-already".to_owned())
    );
}

#[tokio::test]
async fn an_agent_or_key_that_never_paired_is_unknown_and_approves_nothing() {
    let daemon = Daemon::hub(false);
    let waiting = agent(2, "laptop", false, now());
    // The hub key is made while no agent is registered, as on a real hub.
    let fingerprint = daemon.fingerprint().await;
    daemon.registry.hold(waiting.clone());
    let other_key = AgentRecord {
        key: agent(3, "laptop", false, now()).key,
        ..waiting.clone()
    };
    let never = agent(4, "desktop", false, now());
    for asked in [&other_key, &never] {
        assert_eq!(
            refusal(
                daemon
                    .send(approve(LOOPBACK, &[], asked, &fingerprint))
                    .await
            )
            .await,
            (StatusCode::NOT_FOUND, "machine-unknown".to_owned())
        );
    }
    let body = serde_json::json!({ "key": waiting.key.to_hex(), "confirmation": fingerprint });
    let not_an_id = api::machine_approval_path("laptop");
    let not_an_id = build(Method::POST, &not_an_id, LOOPBACK, &[], &body.to_string());
    assert_eq!(
        refusal(daemon.send(not_an_id).await).await,
        (StatusCode::NOT_FOUND, "machine-unknown".to_owned())
    );
    let path = api::machine_approval_path(&waiting.agent_id.as_uuid().to_string());
    let not_a_key = serde_json::json!({ "key": "laptop", "confirmation": fingerprint }).to_string();
    let not_a_key = build(Method::POST, &path, LOOPBACK, &[], &not_a_key);
    assert_eq!(
        refusal(daemon.send(not_a_key).await).await,
        (StatusCode::BAD_REQUEST, "invalid-body".to_owned())
    );
    let kept = daemon.registry.agent(waiting.agent_id).await.unwrap();
    assert!(kept.is_some_and(|kept| kept.is_pending()));
}

#[tokio::test]
async fn an_approval_is_confirmed_as_a_token_is() {
    let daemon = Daemon::hub(false);
    let waiting = agent(2, "laptop", false, now());
    // The hub key is made while no agent is registered, as on a real hub.
    let fingerprint = daemon.fingerprint().await;
    daemon.registry.hold(waiting.clone());
    assert_eq!(
        refusal(
            daemon
                .send(approve(REMOTE, &[], &waiting, &fingerprint))
                .await
        )
        .await,
        (StatusCode::FORBIDDEN, "pairing-needs-sign-in".to_owned())
    );
    let wrong = if fingerprint == "0000-0000-0000-0000-0000" {
        "1111-1111-1111-1111-1111"
    } else {
        "0000-0000-0000-0000-0000"
    };
    let former_key_id = daemon.former_key_id().await;
    for wrong in [wrong, former_key_id.as_str()] {
        assert_eq!(
            refusal(daemon.send(approve(LOOPBACK, &[], &waiting, wrong)).await).await,
            (StatusCode::FORBIDDEN, "step-up-failed".to_owned()),
            "{wrong}"
        );
    }
    let kept = daemon.registry.agent(waiting.agent_id).await.unwrap();
    assert!(kept.is_some_and(|kept| kept.is_pending()));
}

#[tokio::test]
async fn with_sign_in_on_only_a_session_lists_or_approves_and_confirms_with_the_password() {
    let daemon = Daemon::hub(true);
    let waiting = agent(2, "laptop", false, now());
    // The hub key is made while no agent is registered, as on a real hub.
    daemon.pairing.as_ref().unwrap().hub_key().await.unwrap();
    daemon.registry.hold(waiting.clone());
    for path in [api::HUB_MACHINES_PATH, api::HUB_DASHBOARD_PATH] {
        let list = build(Method::GET, path, LOOPBACK, &[], "");
        assert_eq!(daemon.send(list).await.status(), StatusCode::UNAUTHORIZED);
    }
    let unsigned = daemon
        .send(approve(LOOPBACK, &[], &waiting, PASSWORD))
        .await;
    assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);
    let created = daemon
        .access
        .as_ref()
        .unwrap()
        .create_api_token(
            ApiTokenLabel::new("assistant".to_owned()).unwrap(),
            TokenPermissions::new(Permission::ALL.iter().fold(Permissions::NONE, |held, row| {
                held.with(*row, Some(row.level())).unwrap()
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let bearer = [("authorization", format!("Bearer {}", created.text.expose()))];
    for request in [
        build(Method::GET, api::HUB_MACHINES_PATH, LOOPBACK, &bearer, ""),
        build(Method::GET, api::HUB_DASHBOARD_PATH, LOOPBACK, &bearer, ""),
        approve(LOOPBACK, &bearer, &waiting, PASSWORD),
    ] {
        assert_eq!(
            refusal(daemon.send(request).await).await,
            (StatusCode::FORBIDDEN, "api-token-permission".to_owned())
        );
    }
    let session = daemon.signed_in().await;
    let wrong = daemon
        .send(approve(REMOTE, &session, &waiting, "not-it"))
        .await;
    assert_eq!(
        refusal(wrong).await,
        (StatusCode::FORBIDDEN, "step-up-failed".to_owned())
    );
    let peer = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 12));
    let approved = daemon
        .send(approve(peer, &session, &waiting, PASSWORD))
        .await;
    assert_eq!(approved.status(), StatusCode::OK);
}

fn remove(peer: IpAddr, headers: &[(&str, String)], agent: &AgentRecord) -> Request {
    let body = serde_json::json!({ "key": agent.key.to_hex() });
    let path = api::machine_removal_path(&agent.agent_id.as_uuid().to_string());
    build(Method::POST, &path, peer, headers, &body.to_string())
}

#[tokio::test]
async fn on_loopback_a_waiting_or_approved_agent_is_removed_under_its_key_once() {
    let daemon = Daemon::hub(false);
    let approved = agent(1, "macbook", true, now());
    let waiting = agent(2, "laptop", false, now());
    daemon.registry.hold(approved.clone());
    daemon.registry.hold(waiting.clone());
    let rekeyed = AgentRecord {
        key: domain::fleet::PublicKey::from_bytes([0x55; 32]).unwrap(),
        ..approved.clone()
    };
    assert_eq!(
        refusal(daemon.send(remove(LOOPBACK, &[], &rekeyed)).await).await,
        (StatusCode::NOT_FOUND, "machine-unknown".to_owned()),
        "another key removes nothing"
    );
    for removed in [&approved, &waiting] {
        let response = daemon.send(remove(LOOPBACK, &[], removed)).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let kept = daemon.registry.agent(removed.agent_id).await.unwrap();
        assert_eq!(kept, None);
        let tombstone = daemon.registry.tombstone(removed.agent_id).await.unwrap();
        assert_eq!(tombstone.map(|kept| kept.key), Some(removed.key));
        assert_eq!(
            refusal(daemon.send(remove(LOOPBACK, &[], removed)).await).await,
            (StatusCode::NOT_FOUND, "machine-unknown".to_owned())
        );
    }
    let not_an_id = api::machine_removal_path("laptop");
    let body = serde_json::json!({ "key": waiting.key.to_hex() }).to_string();
    let not_an_id = build(Method::POST, &not_an_id, LOOPBACK, &[], &body);
    assert_eq!(
        refusal(daemon.send(not_an_id).await).await,
        (StatusCode::NOT_FOUND, "machine-unknown".to_owned())
    );
    let path = api::machine_removal_path(&waiting.agent_id.as_uuid().to_string());
    for body in [
        r#"{"key":"laptop"}"#,
        r#"{"key":"aa","confirmation":"x"}"#,
        "",
    ] {
        let bad = build(Method::POST, &path, LOOPBACK, &[], body);
        assert_eq!(
            refusal(daemon.send(bad).await).await,
            (StatusCode::BAD_REQUEST, "invalid-body".to_owned()),
            "{body:?}"
        );
    }
}

#[tokio::test]
async fn beyond_loopback_with_sign_in_off_no_agent_is_removed() {
    let daemon = Daemon::hub(false);
    let waiting = agent(2, "laptop", false, now());
    daemon.registry.hold(waiting.clone());
    assert_eq!(
        refusal(daemon.send(remove(REMOTE, &[], &waiting)).await).await,
        (StatusCode::FORBIDDEN, "pairing-needs-sign-in".to_owned())
    );
    assert!(
        daemon
            .registry
            .agent(waiting.agent_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn with_sign_in_on_only_a_session_removes_and_without_a_step_up() {
    let daemon = Daemon::hub(true);
    let waiting = agent(2, "laptop", false, now());
    daemon.registry.hold(waiting.clone());
    let unsigned = daemon.send(remove(LOOPBACK, &[], &waiting)).await;
    assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);
    let created = daemon
        .access
        .as_ref()
        .unwrap()
        .create_api_token(
            ApiTokenLabel::new("assistant".to_owned()).unwrap(),
            TokenPermissions::new(Permission::ALL.iter().fold(Permissions::NONE, |held, row| {
                held.with(*row, Some(row.level())).unwrap()
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let bearer = [("authorization", format!("Bearer {}", created.text.expose()))];
    assert_eq!(
        refusal(daemon.send(remove(LOOPBACK, &bearer, &waiting)).await).await,
        (StatusCode::FORBIDDEN, "api-token-permission".to_owned())
    );
    let session = daemon.signed_in().await;
    let removed = daemon.send(remove(REMOTE, &session, &waiting)).await;
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
}

/// The hub hears a floor it signed above the epoch it counts, as after a
/// restore from an older copy: it enters recovery mode (decision 137).
async fn into_recovery(daemon: &Daemon) {
    use app::ports::OwnershipRegistry as _;
    use domain::maintenance::{LeaseEpoch, ProvenFloor, RepositoryReports, StartWindow};

    let floor = ProvenFloor {
        repository: domain::RepositoryId::parse(&"0a".repeat(32)).unwrap(),
        holder: AgentId::from_uuid(Uuid::from_u128(2)),
        epoch: LeaseEpoch::new(5).unwrap(),
        expires_at: now(),
        held: true,
    };
    daemon
        .owners
        .report(
            floor.holder,
            &RepositoryReports::new([]),
            &[floor],
            now(),
            &StartWindow::new([]),
        )
        .await
        .unwrap();
}

fn read_recovery(peer: IpAddr, headers: &[(&str, String)]) -> Request {
    build(Method::GET, api::HUB_RECOVERY_PATH, peer, headers, "")
}

fn end_recovery(peer: IpAddr, headers: &[(&str, String)], confirmation: &str) -> Request {
    let body = serde_json::json!({ "confirmation": confirmation }).to_string();
    let path = api::HUB_RECOVERY_CONFIRMATION_PATH;
    build(Method::POST, path, peer, headers, &body)
}

/// Whether the hub answers it is in recovery mode.
async fn recovering(daemon: &Daemon) -> bool {
    let response = daemon.send(read_recovery(LOOPBACK, &[])).await;
    assert_eq!(response.status(), StatusCode::OK);
    let read: api::HubRecoveryDto = serde_json::from_value(json(response).await).unwrap();
    read.recovering
}

/// The page reads whether the hub is in recovery mode, and a person ends
/// it with a step-up confirmed as an approval's is; a second end, or one on
/// a hub not in recovery mode, changes nothing (decision 137).
#[tokio::test]
async fn on_loopback_the_fingerprint_ends_recovery_mode_the_page_reads() {
    let daemon = Daemon::hub(false);
    let fingerprint = daemon.fingerprint().await;
    assert!(!recovering(&daemon).await);
    into_recovery(&daemon).await;
    assert!(recovering(&daemon).await);
    let ended = daemon.send(end_recovery(LOOPBACK, &[], &fingerprint)).await;
    assert_eq!(ended.status(), StatusCode::NO_CONTENT);
    assert!(!recovering(&daemon).await);
    let again = daemon.send(end_recovery(LOOPBACK, &[], &fingerprint)).await;
    assert_eq!(again.status(), StatusCode::NO_CONTENT);
    assert!(!recovering(&daemon).await);
}

/// Ending recovery mode is confirmed as an approval is: refused beyond
/// loopback while sign-in is off, refused with a wrong fingerprint or a
/// body the hub can't read, and the hub stays in recovery mode.
#[tokio::test]
async fn recovery_mode_ends_only_with_a_step_up_that_holds() {
    let daemon = Daemon::hub(false);
    let fingerprint = daemon.fingerprint().await;
    into_recovery(&daemon).await;
    assert_eq!(
        refusal(daemon.send(end_recovery(REMOTE, &[], &fingerprint)).await).await,
        (StatusCode::FORBIDDEN, "pairing-needs-sign-in".to_owned())
    );
    let wrong = if fingerprint == "0000-0000-0000-0000-0000" {
        "1111-1111-1111-1111-1111"
    } else {
        "0000-0000-0000-0000-0000"
    };
    assert_eq!(
        refusal(daemon.send(end_recovery(LOOPBACK, &[], wrong)).await).await,
        (StatusCode::FORBIDDEN, "step-up-failed".to_owned())
    );
    let unreadable = build(
        Method::POST,
        api::HUB_RECOVERY_CONFIRMATION_PATH,
        LOOPBACK,
        &[],
        "{}",
    );
    assert_eq!(
        refusal(daemon.send(unreadable).await).await,
        (StatusCode::BAD_REQUEST, "invalid-body".to_owned())
    );
    assert!(recovering(&daemon).await);
}

/// With sign-in on, only a session reads or ends recovery mode, and it
/// confirms with the password from anywhere; an API token with every
/// permission does neither.
#[tokio::test]
async fn with_sign_in_on_only_a_session_reads_or_ends_recovery_mode() {
    let daemon = Daemon::hub(true);
    daemon.pairing.as_ref().unwrap().hub_key().await.unwrap();
    into_recovery(&daemon).await;
    assert_eq!(
        daemon.send(read_recovery(LOOPBACK, &[])).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let created = daemon
        .access
        .as_ref()
        .unwrap()
        .create_api_token(
            ApiTokenLabel::new("assistant".to_owned()).unwrap(),
            TokenPermissions::new(Permission::ALL.iter().fold(Permissions::NONE, |held, row| {
                held.with(*row, Some(row.level())).unwrap()
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let bearer = [("authorization", format!("Bearer {}", created.text.expose()))];
    for request in [
        read_recovery(LOOPBACK, &bearer),
        end_recovery(LOOPBACK, &bearer, PASSWORD),
    ] {
        assert_eq!(
            refusal(daemon.send(request).await).await,
            (StatusCode::FORBIDDEN, "api-token-permission".to_owned())
        );
    }
    let session = daemon.signed_in().await;
    let wrong = daemon.send(end_recovery(REMOTE, &session, "not-it")).await;
    assert_eq!(
        refusal(wrong).await,
        (StatusCode::FORBIDDEN, "step-up-failed".to_owned())
    );
    let read = daemon.send(read_recovery(REMOTE, &session)).await;
    let read: api::HubRecoveryDto = serde_json::from_value(json(read).await).unwrap();
    assert!(read.recovering);
    let peer = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 12));
    let ended = daemon.send(end_recovery(peer, &session, PASSWORD)).await;
    assert_eq!(ended.status(), StatusCode::NO_CONTENT);
    let read = daemon.send(read_recovery(REMOTE, &session)).await;
    let read: api::HubRecoveryDto = serde_json::from_value(json(read).await).unwrap();
    assert!(!read.recovering);
}
