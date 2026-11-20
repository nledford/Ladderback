use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use app::access::Access;
use app::fakes::{
    FakeClock, FakeSigning, InMemoryAdminPassword, InMemoryAgentRegistry, InMemoryApiTokenStore,
    InMemoryIdentity, InMemorySessionStore, SeededRandom,
};
use app::pairing::HubPairing;
use axum::body::Body;
use axum::http::header::{HOST, SET_COOKIE};
use axum::http::{Method, StatusCode};
use domain::Timestamp;
use domain::access::api_token::{ApiTokenLabel, TokenPermissions};
use domain::access::permissions::{Permission, Permissions};
use domain::fleet::{Fingerprint, KeyId};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, SignIn, SyncHub, router};

const PASSWORD: &str = "demo-admin-pass";
const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const REMOTE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
const ADDRESS: &str = "https://hub.example:9443";

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ))
}

/// A daemon serving as a hub, with sign-in on when `access` is given.
struct Daemon {
    pairing: Option<Arc<HubPairing>>,
    access: Option<Arc<Access>>,
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
        let pairing = HubPairing::new(
            Arc::new(InMemoryAgentRegistry::new()),
            Arc::new(InMemoryIdentity::default()),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(5)),
            clock(),
            "0.5.0-hub".to_owned(),
        );
        let pairing = match &access {
            Some(access) => pairing.with_access(Arc::clone(access)),
            None => pairing,
        };
        Self {
            pairing: Some(Arc::new(pairing)),
            access,
        }
    }

    fn not_a_hub() -> Self {
        Self {
            pairing: None,
            access: None,
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
        router(
            Arc::new(NoUi),
            RequestChecks::new(["hub.example".to_owned()]),
            state,
        )
        .oneshot(request)
        .await
        .unwrap()
    }

    async fn key_id(&self) -> String {
        let key = self.pairing.as_ref().unwrap().hub_key().await.unwrap();
        KeyId::of_key(&key).to_string()
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
async fn the_hub_answers_its_keys_fingerprint_in_full_and_its_key_id() {
    let daemon = Daemon::hub(false);
    let response = daemon
        .send(build(Method::GET, api::HUB_PATH, LOOPBACK, &[], ""))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let key = daemon.pairing.as_ref().unwrap().hub_key().await.unwrap();
    let hub: api::HubDto = serde_json::from_value(json(response).await).unwrap();
    assert_eq!(hub.fingerprint, Fingerprint::of_key(&key).to_string());
    assert_eq!(hub.key_id, KeyId::of_key(&key).to_string());
}

#[tokio::test]
async fn a_daemon_not_serving_as_a_hub_answers_its_routes_as_unknown() {
    let daemon = Daemon::not_a_hub();
    for (method, path) in [
        (Method::GET, api::HUB_PATH.to_owned()),
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
async fn on_loopback_the_key_id_makes_a_token_shown_once_and_cancelled_once() {
    let daemon = Daemon::hub(false);
    let key_id = daemon.key_id().await.to_lowercase();
    let response = daemon.send(make(LOOPBACK, &[], &ask(&key_id))).await;
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
    let key_id = daemon.key_id().await;
    let remote_peer = make(REMOTE, &[], &ask(&key_id));
    let mut remote_host = make(LOOPBACK, &[], &ask(&key_id));
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
async fn a_wrong_key_id_is_a_step_up_that_failed() {
    let daemon = Daemon::hub(false);
    let wrong = if daemon.key_id().await == "00000000" {
        "11111111"
    } else {
        "00000000"
    };
    assert_eq!(
        refusal(daemon.send(make(LOOPBACK, &[], &ask(wrong))).await).await,
        (StatusCode::FORBIDDEN, "step-up-failed".to_owned())
    );
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
    let key_id = daemon.key_id().await;
    let by_key_id = daemon.send(make(peer(12), &session, &ask(&key_id))).await;
    assert_eq!(
        refusal(by_key_id).await,
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
}

#[tokio::test]
async fn an_ask_the_hub_cant_read_is_refused_by_its_kind() {
    let daemon = Daemon::hub(false);
    let key_id = daemon.key_id().await;
    let cases = [
        (
            r#"{"address":1}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
        (
            serde_json::json!({ "address": "ftp://hub", "confirmation": key_id }).to_string(),
            StatusCode::BAD_REQUEST,
            "hub-address-invalid",
        ),
        (
            serde_json::json!({ "address": ADDRESS, "life-minutes": 4, "confirmation": key_id })
                .to_string(),
            StatusCode::BAD_REQUEST,
            "pairing-token-life",
        ),
        // The certificate's kind isn't the page's to choose while the hub
        // serves no certificate of its own: a pinned one is no ask it reads.
        (
            serde_json::json!({ "address": ADDRESS, "certificate": "pinned", "confirmation": key_id })
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
