use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use api::BuildDto;
use app::fakes::{FakeClock, InMemoryAdminPassword, InMemorySessionStore, SeededRandom};
use app::ports::{AdminPassword, AdminPasswordError};
use async_trait::async_trait;
use axum::body::Body;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use futures::StreamExt;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, sign_in_off, test_state};
use crate::{Exposure, RequestChecks, router};

const PASSWORD: &str = "demo-admin-pass";
const OWN_HOST: &str = "127.0.0.1:8080";
const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const AWAY: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20)), 50_000);

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ))
}

fn on_with(password: Arc<dyn AdminPassword>, clock: &Arc<FakeClock>) -> SignIn {
    SignIn::On(Arc::new(Access::new(
        password,
        Arc::new(SeededRandom::new(7)),
        clock.clone(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        Arc::new(app::fakes::InMemorySessionStore::new()),
    )))
}

fn on() -> SignIn {
    on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock())
}

/// Sign-in off, with the admin password kept in `password` and the sessions
/// kept from when it was on in `sessions`.
fn off_over(password: Arc<dyn AdminPassword>, sessions: Arc<InMemorySessionStore>) -> SignIn {
    SignIn::Off { password, sessions }
}

async fn send(sign_in: &SignIn, request: Request) -> Response {
    send_listening(sign_in, Exposure::Loopback, request).await
}

/// [`send`] to a daemon that `exposure` says who can reach.
async fn send_listening(sign_in: &SignIn, exposure: Exposure, request: Request) -> Response {
    let mut state = test_state();
    state.sign_in = sign_in.clone();
    state.exposure = exposure;
    router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(request)
        .await
        .unwrap()
}

/// A request from `HOME` for this daemon's own host.
fn build(method: Method, path: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, OWN_HOST);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let mut request = builder.body(Body::from(body.to_owned())).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(crate::PeerAddr(HOME)));
    request
}

fn get(path: &str, headers: &[(&str, &str)]) -> Request {
    build(Method::GET, path, headers, "")
}

/// A changed request, with the JSON content type the request checks ask of
/// one.
fn post(path: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut all = vec![("content-type", "application/json")];
    all.extend_from_slice(headers);
    build(Method::POST, path, &all, body)
}

fn sign_in_request(password: &str, headers: &[(&str, &str)]) -> Request {
    post(
        SESSION_PATH,
        headers,
        &serde_json::json!({ "password": password }).to_string(),
    )
}

/// `answer` with the build the tests' daemon runs, in fields of its own.
fn told_the_build(mut answer: serde_json::Value) -> serde_json::Value {
    let fields = serde_json::json!({
        "version": "0.3.0",
        "commit": "3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90",
        "dirty": true,
        "release": false
    });
    if let (Some(answer), Some(fields)) = (answer.as_object_mut(), fields.as_object()) {
        answer.extend(fields.clone());
    }
    answer
}

async fn json(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// The refusal code of a response, if it is one.
async fn code(response: Response) -> (StatusCode, Option<String>) {
    let status = response.status();
    let body = json(response).await;
    let code = body
        .get("code")
        .and_then(|code| code.as_str())
        .map(str::to_owned);
    (status, code)
}

fn set_cookie(response: &Response) -> String {
    response.headers()[SET_COOKIE].to_str().unwrap().to_owned()
}

/// A session signed in from `HOME`: the `Cookie` header that carries it and
/// its CSRF token.
async fn signed_in(sign_in: &SignIn) -> (String, String) {
    let response = send(sign_in, sign_in_request(PASSWORD, &[])).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = set_cookie(&response);
    let pair = cookie.split(';').next().unwrap().to_owned();
    let body = json(response).await;
    let csrf = body["csrf-token"].as_str().unwrap().to_owned();
    (pair, csrf)
}

async fn assert_sign_in_required(sign_in: &SignIn, request: Request, what: &str) {
    assert_eq!(
        code(send(sign_in, request).await).await,
        (
            StatusCode::UNAUTHORIZED,
            Some("sign-in-required".to_owned())
        ),
        "{what}"
    );
}

/// Neither refusal of this layer: the request reached what serves it.
async fn assert_let_through(sign_in: &SignIn, request: Request, what: &str) {
    let (status, code) = code(send(sign_in, request).await).await;
    assert!(
        !matches!(
            code.as_deref(),
            Some("sign-in-required" | "csrf-token-invalid")
        ),
        "{what} was refused: {status} {code:?}"
    );
    assert_ne!(status, StatusCode::UNAUTHORIZED, "{what}");
}

#[tokio::test]
async fn with_sign_in_off_every_request_is_let_through_without_a_session() {
    let off = sign_in_off();
    assert_let_through(&off, get("/api/v1/plans", &[]), "a read").await;
    assert_let_through(
        &off,
        post("/api/v1/operations/x/cancel", &[], ""),
        "a changed request without a CSRF token",
    )
    .await;
    let response = send(&off, get(SESSION_PATH, &[])).await;
    assert_eq!(
        json(response).await,
        told_the_build(serde_json::json!({ "sign-in": "off" }))
    );
    let response = send(&off, sign_in_request(PASSWORD, &[])).await;
    assert!(!response.headers().contains_key(SET_COOKIE));
    assert_eq!(
        json(response).await,
        told_the_build(serde_json::json!({ "sign-in": "off" }))
    );
}

#[tokio::test]
async fn the_session_route_says_when_the_daemon_answers_on_the_network() {
    let off = sign_in_off();
    let network = Exposure::Network;
    let response = send_listening(&off, network, get(SESSION_PATH, &[])).await;
    assert_eq!(
        json(response).await,
        told_the_build(serde_json::json!({ "sign-in": "off", "exposure": "network" }))
    );
    let response = send_listening(&off, network, sign_in_request(PASSWORD, &[])).await;
    assert_eq!(
        json(response).await,
        told_the_build(serde_json::json!({ "sign-in": "off", "exposure": "network" }))
    );
    let response = send_listening(&on(), network, get(SESSION_PATH, &[])).await;
    assert_eq!(
        json(response).await,
        serde_json::json!({
            "sign-in": "signed-out",
            "exposure": "network",
            "keep-signed-in-offered": true
        })
    );
}

#[tokio::test]
async fn the_session_route_tells_a_build_with_no_known_commit_as_null() {
    let mut state = test_state();
    state.build = BuildDto {
        version: "0.3.0".to_owned(),
        commit: None,
        dirty: false,
        release: false,
    };
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(get(SESSION_PATH, &[]))
        .await
        .unwrap();
    assert_eq!(
        json(response).await,
        serde_json::json!({
            "sign-in": "off",
            "version": "0.3.0",
            "commit": null,
            "dirty": false,
            "release": false
        })
    );
}

/// The session route's answer to a GET of it, from a daemon whose release
/// check found `0.4.0`, under `sign_in`.
async fn session_with_update(sign_in: &SignIn) -> serde_json::Value {
    let mut state = test_state();
    state.sign_in = sign_in.clone();
    state
        .updates
        .show(Some(domain::updates::Version::parse("0.4.0").unwrap()));
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(get(SESSION_PATH, &[]))
        .await
        .unwrap();
    json(response).await
}

#[tokio::test]
async fn the_session_route_tells_the_available_update_with_the_build() {
    assert_eq!(
        session_with_update(&sign_in_off()).await,
        told_the_build(serde_json::json!({
            "sign-in": "off",
            "available-update": {
                "version": "0.4.0",
                "release-notes": "https://github.com/nledford/Ladderback/releases/tag/v0.4.0"
            }
        }))
    );
    // A page yet to sign in is told neither.
    assert_eq!(
        session_with_update(&on()).await,
        serde_json::json!({ "sign-in": "signed-out", "keep-signed-in-offered": true })
    );
}

#[tokio::test]
async fn with_sign_in_on_an_api_request_without_a_live_session_never_reaches_its_route() {
    let on = on();
    for path in [
        "/api/v1",
        "/api/v1/",
        "/api/v1/plans",
        "/api/v1/nowhere",
        "/api/v1/events",
    ] {
        assert_sign_in_required(&on, get(path, &[]), path).await;
        let not_a_token = format!("{COOKIE_NAME}=abc");
        assert_sign_in_required(&on, get(path, &[("cookie", &not_a_token)]), path).await;
    }
    let unknown = format!("{COOKIE_NAME}={}", Token::from_bytes([1; 32]).to_text());
    assert_sign_in_required(
        &on,
        get("/api/v1/plans", &[("cookie", &unknown)]),
        "unknown",
    )
    .await;
    assert_sign_in_required(
        &on,
        post("/api/v1/operations/x/cancel", &[], ""),
        "a changed request without a session",
    )
    .await;
}

#[tokio::test]
async fn the_web_ui_files_are_served_without_a_session() {
    let on = on();
    for path in ["/", "/index.html", "/app.wasm"] {
        assert_let_through(&on, get(path, &[]), path).await;
    }
}

/// Sign-in guards `/api/v1` alone, so an API path beside it that no route
/// serves is told unknown without a session, never sent the UI.
#[tokio::test]
async fn an_api_path_beside_v1_that_no_route_serves_is_unknown_without_a_session() {
    let on = on();
    for path in ["/api", "/api/v10/plans"] {
        let answer = code(send(&on, get(path, &[])).await).await;
        assert_eq!(
            answer,
            (StatusCode::NOT_FOUND, Some("route-unknown".to_owned())),
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_live_sessions_reads_reach_their_route_with_its_cookie_among_others() {
    let on = on();
    let (cookie, _) = signed_in(&on).await;
    assert_let_through(&on, get("/api/v1/plans", &[("cookie", &cookie)]), "a read").await;
    let among = format!("theme=dark; {cookie}; other=1");
    assert_let_through(
        &on,
        get("/api/v1/plans", &[("cookie", &among)]),
        "among others",
    )
    .await;
}

#[tokio::test]
async fn a_changed_request_without_its_sessions_csrf_token_is_refused() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    let (_, other_csrf) = signed_in(&on).await;
    let path = "/api/v1/operations/x/cancel";
    let refused = |headers: Vec<(&'static str, String)>| {
        let on = on.clone();
        async move {
            let headers: Vec<(&str, &str)> = headers
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect();
            code(send(&on, post(path, &headers, "")).await).await
        }
    };
    let csrf_refusal = (StatusCode::FORBIDDEN, Some("csrf-token-invalid".to_owned()));
    for (headers, what) in [
        (vec![("cookie", cookie.clone())], "no token"),
        (
            vec![
                ("cookie", cookie.clone()),
                ("x-csrf-token", other_csrf.clone()),
            ],
            "another session's token",
        ),
        (
            vec![
                ("cookie", cookie.clone()),
                ("x-csrf-token", "abc".to_owned()),
            ],
            "not a token",
        ),
        (
            vec![
                ("cookie", cookie.clone()),
                ("x-csrf-token", csrf.clone()),
                ("x-csrf-token", csrf.clone()),
            ],
            "the token twice",
        ),
    ] {
        assert_eq!(refused(headers).await, csrf_refusal, "{what}");
    }
    assert_let_through(
        &on,
        post(path, &[("cookie", &cookie), ("x-csrf-token", &csrf)], ""),
        "the session's own token",
    )
    .await;
}

#[tokio::test]
async fn the_session_route_says_where_a_request_stands_and_how_long_its_session_lasts() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let response = send(&on, get(SESSION_PATH, &[])).await;
    assert_eq!(
        json(response).await,
        serde_json::json!({ "sign-in": "signed-out", "keep-signed-in-offered": true })
    );
    let response = send(&on, sign_in_request(PASSWORD, &[])).await;
    let cookie = set_cookie(&response);
    let cookie = cookie.split(';').next().unwrap().to_owned();
    let body = json(response).await;
    let csrf = body["csrf-token"].as_str().unwrap().to_owned();
    assert_eq!(
        body,
        told_the_build(serde_json::json!({
            "sign-in": "signed-in",
            "csrf-token": csrf,
            "idle-seconds": 43_200,
            "ends-in-seconds": 2_592_000
        }))
    );
    clock.advance(domain::SignedDuration::from_hours(11));
    let response = send(&on, get(SESSION_PATH, &[("cookie", &cookie)])).await;
    assert_eq!(
        json(response).await,
        told_the_build(serde_json::json!({
            "sign-in": "signed-in",
            "csrf-token": csrf,
            "idle-seconds": 43_200,
            "ends-in-seconds": 2_592_000 - 11 * 3_600
        }))
    );
    // Asking was a use: the session lasts another 12 hours from there.
    clock.advance(domain::SignedDuration::from_hours(11));
    let response = send(&on, get(SESSION_PATH, &[("cookie", &cookie)])).await;
    assert_eq!(json(response).await["sign-in"], "signed-in");
}

#[tokio::test]
async fn a_sign_in_sets_a_cookie_scripts_cant_read_and_other_sites_never_send() {
    let on = on();
    let response = send(&on, sign_in_request(PASSWORD, &[])).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = set_cookie(&response);
    let body = json(response).await;
    assert_eq!(body["sign-in"], "signed-in");
    let token = cookie
        .strip_prefix(&format!("{COOKIE_NAME}="))
        .and_then(|rest| rest.split(';').next())
        .unwrap();
    assert!(Token::parse(token).is_some(), "{cookie}");
    assert_ne!(Some(token), body["csrf-token"].as_str());
    assert_eq!(
        cookie.strip_prefix(&format!("{COOKIE_NAME}={token}")),
        Some("; Path=/; Max-Age=2592000; HttpOnly; SameSite=Strict")
    );
}

#[tokio::test]
async fn the_cookie_is_secure_only_for_a_page_served_over_https_by_its_own_host() {
    let on = on();
    for (origin, secure) in [
        (None, false),
        (Some(format!("http://{OWN_HOST}")), false),
        (Some(format!("https://{OWN_HOST}")), true),
    ] {
        let headers: Vec<(&str, &str)> = origin.iter().map(|o| ("origin", o.as_str())).collect();
        let response = send(&on, sign_in_request(PASSWORD, &headers)).await;
        assert_eq!(response.status(), StatusCode::OK, "{origin:?}");
        assert_eq!(
            set_cookie(&response).ends_with("; Secure"),
            secure,
            "{origin:?}"
        );
    }
}

#[test]
fn over_https_asks_for_https_and_the_requests_own_host() {
    let headers = |origin: &str, host: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
        headers.insert(HOST, HeaderValue::from_str(host).unwrap());
        headers
    };
    assert!(over_https(&headers(
        "https://nas.demo.test",
        "nas.demo.test"
    )));
    assert!(over_https(&headers(
        "https://NAS.demo.test",
        "nas.demo.test"
    )));
    assert!(!over_https(&headers(
        "https://nas.demo.test:8443",
        "nas.demo.test"
    )));
    assert!(!over_https(&headers("https://other.test", "nas.demo.test")));
    assert!(!over_https(&headers(
        "http://nas.demo.test",
        "nas.demo.test"
    )));
    assert!(!over_https(&headers("https:/", "nas.demo.test")));
    let mut no_host = HeaderMap::new();
    no_host.insert(ORIGIN, HeaderValue::from_static("https://nas.demo.test"));
    assert!(!over_https(&no_host));
}

#[tokio::test]
async fn a_wrong_password_is_refused_and_the_next_attempt_waits() {
    let on = on();
    assert_eq!(
        code(send(&on, sign_in_request("not-the-password", &[])).await).await,
        (StatusCode::UNAUTHORIZED, Some("sign-in-failed".to_owned()))
    );
    let response = send(&on, sign_in_request(PASSWORD, &[])).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(!response.headers().contains_key(SET_COOKIE));
    let retry_after = response.headers()[RETRY_AFTER].to_str().unwrap().to_owned();
    let body = json(response).await;
    assert_eq!(body["code"], "sign-in-wait");
    assert_eq!(body["args"]["seconds"]["value"].to_string(), retry_after);
    assert_ne!(retry_after, "0");
}

#[tokio::test]
async fn the_wait_is_kept_by_the_connections_address_never_a_forwarded_one() {
    let on = on();
    let wrong = sign_in_request("not-the-password", &[("x-forwarded-for", "198.51.100.1")]);
    assert_eq!(send(&on, wrong).await.status(), StatusCode::UNAUTHORIZED);
    let forwarded = sign_in_request(
        PASSWORD,
        &[
            ("x-forwarded-for", "198.51.100.2"),
            ("forwarded", "for=198.51.100.2"),
        ],
    );
    assert_eq!(
        send(&on, forwarded).await.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "another forwarded address is the same client"
    );
    let mut away = sign_in_request(PASSWORD, &[]);
    away.extensions_mut()
        .insert(ConnectInfo(crate::PeerAddr(AWAY)));
    assert_eq!(
        send(&on, away).await.status(),
        StatusCode::OK,
        "another connection's address has its own wait"
    );
}

/// A sign-in the server hands over without its connection's address can't
/// be given a wait of its own. Keyed on one stand-in address, it would make
/// every client share one wait, which a single client could keep at its
/// longest for all: it is refused, loudly, and its password counts for no one.
#[tokio::test]
async fn a_sign_in_without_its_connection_s_address_is_refused_and_counts_for_no_one() {
    let on = on();
    for password in ["not-the-password", PASSWORD] {
        let mut request = sign_in_request(password, &[]);
        request
            .extensions_mut()
            .remove::<ConnectInfo<crate::PeerAddr>>();
        assert_eq!(
            code(send(&on, request).await).await,
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Some("sign-in-unavailable".to_owned())
            )
        );
    }
    assert_eq!(
        send(&on, sign_in_request(PASSWORD, &[])).await.status(),
        StatusCode::OK,
        "the refused sign-ins made a client wait"
    );
}

#[tokio::test]
async fn a_sign_in_body_that_isnt_one_password_is_refused() {
    let on = on();
    for body in [
        "",
        "{}",
        "not json",
        r#"{"password":"demo-admin-pass","user":"x"}"#,
    ] {
        assert_eq!(
            code(send(&on, post(SESSION_PATH, &[], body)).await).await,
            (StatusCode::BAD_REQUEST, Some("invalid-body".to_owned())),
            "{body}"
        );
    }
    assert_eq!(
        code(send(&on, sign_in_request("", &[])).await).await,
        (StatusCode::UNAUTHORIZED, Some("sign-in-failed".to_owned())),
        "an empty password is a wrong one"
    );
}

/// Keeps a password whose hash can't be read.
struct Unreadable;

#[async_trait]
impl AdminPassword for Unreadable {
    async fn verify(&self, _attempt: &Password) -> Result<bool, AdminPasswordError> {
        Err(AdminPasswordError::Unreadable(
            "/demo/admin-password: denied".into(),
        ))
    }

    async fn set(&self, _password: &Password) -> Result<(), AdminPasswordError> {
        Err(AdminPasswordError::Unwritable(
            "/demo/admin-password: denied".into(),
        ))
    }

    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        Ok(PasswordFingerprint::new(1))
    }
}

#[tokio::test]
async fn a_password_that_cant_be_checked_is_unavailable_and_its_detail_stays_in_the_log() {
    let on = on_with(Arc::new(Unreadable), &clock());
    let response = send(&on, sign_in_request(PASSWORD, &[])).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key(SET_COOKIE));
    let body = json(response).await;
    assert_eq!(body, serde_json::json!({ "code": "sign-in-unavailable" }));
}

#[tokio::test]
async fn signing_out_ends_the_session_and_has_the_browser_forget_its_cookie() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    let request = build(
        Method::DELETE,
        SESSION_PATH,
        &[
            ("cookie", &cookie),
            ("content-type", "application/json"),
            ("x-csrf-token", &csrf),
        ],
        "",
    );
    let response = send(&on, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
    assert_eq!(
        json(response).await,
        serde_json::json!({ "sign-in": "signed-out", "keep-signed-in-offered": true })
    );
    assert_sign_in_required(&on, get("/api/v1/plans", &[("cookie", &cookie)]), "after").await;
}

#[tokio::test]
async fn signing_out_without_the_sessions_csrf_token_is_refused_and_the_session_goes_on() {
    let on = on();
    let (cookie, _) = signed_in(&on).await;
    let (_, other_csrf) = signed_in(&on).await;
    for (token, what) in [(None, "no token"), (Some(other_csrf.as_str()), "another's")] {
        let mut headers = vec![
            ("cookie", cookie.as_str()),
            ("content-type", "application/json"),
        ];
        headers.extend(token.map(|token| ("x-csrf-token", token)));
        let response = send(&on, build(Method::DELETE, SESSION_PATH, &headers, "")).await;
        assert!(!response.headers().contains_key(SET_COOKIE), "{what}");
        assert_eq!(
            code(response).await,
            (StatusCode::FORBIDDEN, Some("csrf-token-invalid".to_owned())),
            "{what}"
        );
    }
    assert_let_through(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "the session that was not signed out",
    )
    .await;
}

#[tokio::test]
async fn signing_out_a_session_that_already_ended_still_forgets_its_cookie() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (cookie, _) = signed_in(&on).await;
    clock.advance(domain::access::SESSION_IDLE);
    let request = build(
        Method::DELETE,
        SESSION_PATH,
        &[("cookie", &cookie), ("content-type", "application/json")],
        "",
    );
    let response = send(&on, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
}

/// Keeps sessions in memory until it is broken, then refuses every write.
#[derive(Default)]
struct Breakable {
    kept: app::fakes::InMemorySessionStore,
    broken: std::sync::atomic::AtomicBool,
}

impl Breakable {
    fn check(&self) -> Result<(), app::ports::StoreError> {
        if self.broken.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(app::ports::StoreError::Unavailable {
                detail: "/demo/ladderback.db: disk I/O error".into(),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl app::ports::SessionStore for Breakable {
    async fn add(
        &self,
        session: &domain::access::KeptSession,
        forget: &[domain::access::SessionDigest],
    ) -> Result<(), app::ports::StoreError> {
        self.check()?;
        self.kept.add(session, forget).await
    }

    async fn all(&self) -> Result<Vec<domain::access::KeptSession>, app::ports::StoreError> {
        self.check()?;
        self.kept.all().await
    }

    async fn forget_unreadable(&self) -> Result<usize, app::ports::StoreError> {
        self.check()?;
        self.kept.forget_unreadable().await
    }

    async fn remove(
        &self,
        digests: &[domain::access::SessionDigest],
    ) -> Result<bool, app::ports::StoreError> {
        self.check()?;
        self.kept.remove(digests).await
    }

    async fn remove_all(&self) -> Result<bool, app::ports::StoreError> {
        self.check()?;
        self.kept.remove_all().await
    }

    async fn record_use(
        &self,
        digest: &domain::access::SessionDigest,
        at: Timestamp,
    ) -> Result<(), app::ports::StoreError> {
        self.check()?;
        self.kept.record_use(digest, at).await
    }

    async fn length(
        &self,
    ) -> Result<Option<domain::access::SessionLength>, app::ports::StoreError> {
        self.check()?;
        self.kept.length().await
    }

    async fn set_length(
        &self,
        length: domain::access::SessionLength,
    ) -> Result<bool, app::ports::StoreError> {
        self.check()?;
        self.kept.set_length(length).await
    }
}

#[tokio::test]
async fn a_sign_out_the_store_cant_keep_is_unavailable_yet_ends_the_session_and_forgets_its_cookie()
{
    let store = Arc::new(Breakable::default());
    let on = SignIn::On(Arc::new(Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(7)),
        clock(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        store.clone(),
    )));
    let (cookie, csrf) = signed_in(&on).await;
    store
        .broken
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let request = build(
        Method::DELETE,
        SESSION_PATH,
        &[
            ("cookie", &cookie),
            ("content-type", "application/json"),
            ("x-csrf-token", &csrf),
        ],
        "",
    );
    let response = send(&on, request).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
    assert_eq!(
        json(response).await,
        serde_json::json!({ "code": "sessions-unavailable" })
    );
    assert_sign_in_required(&on, get("/api/v1/plans", &[("cookie", &cookie)]), "after").await;
}

#[tokio::test]
async fn a_sign_in_whose_session_the_store_cant_keep_says_the_sessions_are_unavailable() {
    let store = Arc::new(Breakable::default());
    let on = SignIn::On(Arc::new(Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(7)),
        clock(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        store.clone(),
    )));
    store
        .broken
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let response = send(&on, sign_in_request(PASSWORD, &[])).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key(SET_COOKIE));
    assert_eq!(
        json(response).await,
        serde_json::json!({ "code": "sessions-unavailable" })
    );
}

#[tokio::test]
async fn a_sign_in_body_over_the_api_limit_is_refused_before_it_is_read() {
    let on = on();
    let over = usize::try_from(crate::body_limit::API_BODY_LIMIT).unwrap() + 1;
    let password = "d".repeat(over);
    let response = send(&on, sign_in_request(&password, &[])).await;
    assert!(!response.headers().contains_key(SET_COOKIE));
    assert_eq!(
        code(response).await,
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            Some("body-too-large".to_owned())
        )
    );
}

/// Half the idle limit: a use at this point would keep the session past it.
fn half_idle() -> SignedDuration {
    domain::access::SESSION_IDLE / 2
}

#[tokio::test]
async fn a_read_the_page_sends_on_its_own_never_keeps_the_session() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (cookie, _) = signed_in(&on).await;
    clock.advance(half_idle());
    assert_let_through(
        &on,
        get(
            "/api/v1/plans",
            &[("cookie", &cookie), (api::BACKGROUND_HEADER, "1")],
        ),
        "a read on its own while the session is live",
    )
    .await;
    clock.advance(half_idle());
    assert_sign_in_required(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "12 hours after the last read a person caused",
    )
    .await;
}

#[tokio::test]
async fn a_read_a_person_causes_keeps_the_session() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (cookie, _) = signed_in(&on).await;
    clock.advance(half_idle());
    assert_let_through(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "a read a person caused",
    )
    .await;
    clock.advance(half_idle());
    assert_let_through(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "12 hours after signing in, 6 after the read",
    )
    .await;
}

#[tokio::test]
async fn opening_the_events_stream_never_keeps_the_session() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (cookie, _) = signed_in(&on).await;
    clock.advance(half_idle());
    let handshake = get(
        EVENTS_PATH,
        &[
            ("cookie", &cookie),
            ("connection", "upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-version", "13"),
            ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ],
    );
    let (status, code) = code(send(&on, handshake).await).await;
    assert_ne!(code.as_deref(), Some("sign-in-required"), "{status}");
    clock.advance(half_idle());
    assert_sign_in_required(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "12 hours after signing in, with an events stream opened in between",
    )
    .await;
}

#[tokio::test]
async fn a_session_unused_past_its_idle_limit_is_refused() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (cookie, _) = signed_in(&on).await;
    clock.advance(domain::access::SESSION_IDLE);
    assert_sign_in_required(&on, get("/api/v1/plans", &[("cookie", &cookie)]), "idle").await;
}

#[tokio::test]
async fn an_events_stream_ends_once_the_session_that_opened_it_does() {
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let on = on();
    let (cookie, _) = signed_in(&on).await;
    let listener = crate::GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let mut state = test_state();
    state.sign_in = on.clone();
    let app = router(Arc::new(NoUi), RequestChecks::default(), state);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<crate::PeerAddr>(),
        )
        .await;
    });
    let mut request = format!("ws://{addr}/api/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(COOKIE, HeaderValue::from_str(&cookie).unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    // The stream is open, and answers, while the session is live.
    futures::SinkExt::send(&mut socket, Message::Ping(vec![7].into()))
        .await
        .unwrap();
    match socket.next().await {
        Some(Ok(Message::Pong(payload))) => assert_eq!(payload.as_ref(), [7]),
        other => panic!("a live session's stream answers a ping: {other:?}"),
    }
    let SignIn::On(access) = &on else {
        panic!("signed in above")
    };
    let token = Token::parse(cookie.split_once('=').unwrap().1).unwrap();
    access.sign_out(&token).await.unwrap();
    let ended = tokio::time::timeout(
        crate::SESSION_RECHECK + std::time::Duration::from_secs(2),
        async {
            loop {
                match socket.next().await {
                    None | Some(Err(_)) => break,
                    Some(Ok(message)) if message.is_close() => break,
                    Some(Ok(_)) => {}
                }
            }
        },
    )
    .await;
    assert!(ended.is_ok(), "the stream outlived its session");
}

#[tokio::test]
async fn with_sign_in_off_the_session_route_says_an_admin_password_set_since_the_start_waits() {
    let kept = Arc::new(InMemoryAdminPassword::default());
    let off = off_over(kept.clone(), Arc::new(InMemorySessionStore::new()));
    let response = send(&off, get(SESSION_PATH, &[])).await;
    assert_eq!(
        json(response).await,
        told_the_build(serde_json::json!({ "sign-in": "off" })),
        "nothing is kept yet"
    );
    kept.set(&Password::parse(PASSWORD.to_owned()).unwrap())
        .await
        .unwrap();
    let waits = told_the_build(serde_json::json!({ "sign-in": "off", "admin-password": "set" }));
    let response = send(&off, get(SESSION_PATH, &[])).await;
    assert_eq!(json(response).await, waits);
    // Sign-in stays off until the next start: a sign-in starts no session.
    let response = send(&off, sign_in_request(PASSWORD, &[])).await;
    assert!(!response.headers().contains_key(SET_COOKIE));
    assert_eq!(json(response).await, waits);
    assert_let_through(&off, get("/api/v1/plans", &[]), "a read without a session").await;
}

/// An admin password file that is there but can't be read.
struct Damaged;

#[async_trait]
impl AdminPassword for Damaged {
    async fn verify(&self, _attempt: &Password) -> Result<bool, AdminPasswordError> {
        Err(AdminPasswordError::Unreadable(
            "/demo/admin-password is not an argon2id hash".into(),
        ))
    }

    async fn set(&self, _password: &Password) -> Result<(), AdminPasswordError> {
        Err(AdminPasswordError::Unwritable(
            "/demo/admin-password: denied".into(),
        ))
    }

    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        Err(AdminPasswordError::Unreadable(
            "/demo/admin-password is not an argon2id hash".into(),
        ))
    }
}

#[tokio::test]
async fn with_sign_in_off_the_session_route_says_an_admin_password_file_it_cant_read() {
    let off = off_over(Arc::new(Damaged), Arc::new(InMemorySessionStore::new()));
    let response = send(&off, get(SESSION_PATH, &[])).await;
    let body = json(response).await;
    assert_eq!(
        body,
        told_the_build(serde_json::json!({ "sign-in": "off", "admin-password": "unreadable" }))
    );
    assert!(!body.to_string().contains("argon2id"), "{body}");
}

/// The route that sets and changes the admin password.
const ADMIN_PASSWORD: &str = "/api/v1/admin-password";
const NEW_PASSWORD: &str = "another-admin-pass";

/// A request to change the admin password from the session `cookie` and
/// `csrf`, with `current` as the current password (none when `None`).
fn change_request(cookie: &str, csrf: &str, current: Option<&str>) -> Request {
    let mut body = serde_json::json!({ "new-password": NEW_PASSWORD });
    if let Some(current) = current {
        body["current-password"] = current.into();
    }
    post(
        ADMIN_PASSWORD,
        &[("cookie", cookie), ("x-csrf-token", csrf)],
        &body.to_string(),
    )
}

#[tokio::test]
async fn a_changed_password_has_the_browser_forget_the_session_s_cookie() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    let response = send(&on, change_request(&cookie, &csrf, Some(PASSWORD))).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
    assert_sign_in_required(&on, get("/api/v1/plans", &[("cookie", &cookie)]), "after").await;
    assert_eq!(
        send(&on, sign_in_request(NEW_PASSWORD, &[])).await.status(),
        StatusCode::OK
    );
}

/// No current password, an empty one, or one longer than any admin password
/// can't be the admin password: refused as a wrong one, without a hash, and
/// counted for no one.
#[tokio::test]
async fn a_current_password_that_cant_be_the_admin_password_is_refused_uncounted() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    let over = "d".repeat(domain::access::PASSWORD_MAX_BYTES + 1);
    for (current, what) in [
        (None, "none"),
        (Some(""), "empty"),
        (Some(over.as_str()), "over"),
    ] {
        let response = send(&on, change_request(&cookie, &csrf, current)).await;
        assert!(!response.headers().contains_key(SET_COOKIE), "{what}");
        assert_eq!(
            code(response).await,
            (
                StatusCode::FORBIDDEN,
                Some("current-password-wrong".to_owned())
            ),
            "{what}"
        );
    }
    assert_eq!(
        send(&on, change_request(&cookie, &csrf, Some(PASSWORD)))
            .await
            .status(),
        StatusCode::NO_CONTENT,
        "the refused changes made the client wait"
    );
}

/// A `PUT` of the sign-in settings with `body`, carrying `headers`.
fn put_settings(headers: &[(&str, &str)], body: &str) -> Request {
    let mut all = vec![("content-type", "application/json")];
    all.extend_from_slice(headers);
    build(Method::PUT, settings::SETTINGS_PATH, &all, body)
}

fn settings_body(length: &str) -> String {
    serde_json::json!({ "session-length": length }).to_string()
}

/// The session length, as the access a `SignIn::On` holds has it.
fn length_of(sign_in: &SignIn) -> domain::access::SessionLength {
    match sign_in {
        SignIn::On(access) => access.session_length(),
        SignIn::Off { .. } => panic!("sign-in is on"),
    }
}

#[tokio::test]
async fn the_sign_in_settings_say_the_session_length_and_a_session_changes_it() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    let read = || get(settings::SETTINGS_PATH, &[("cookie", &cookie)]);
    let response = send(&on, read()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json(response).await,
        serde_json::json!({ "session-length": "30-days" })
    );
    // Each name, written out: one read back from the code would follow any
    // change to it.
    for (name, length) in [
        ("7-days", domain::access::SessionLength::SevenDays),
        ("90-days", domain::access::SessionLength::NinetyDays),
        ("1-year", domain::access::SessionLength::OneYear),
        (
            "until-sign-out",
            domain::access::SessionLength::UntilSignOut,
        ),
        ("30-days", domain::access::SessionLength::ThirtyDays),
        (
            "until-browser-closes",
            domain::access::SessionLength::UntilBrowserCloses,
        ),
    ] {
        let response = send(
            &on,
            put_settings(
                &[("cookie", &cookie), ("x-csrf-token", &csrf)],
                &settings_body(name),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        let answer = serde_json::json!({ "session-length": name });
        assert_eq!(json(response).await, answer, "{name}");
        assert_eq!(length_of(&on), length, "{name}");
        assert_eq!(json(send(&on, read()).await).await, answer, "{name}");
    }
}

#[tokio::test]
async fn the_sign_in_settings_need_a_live_session_and_a_change_its_csrf_token() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    assert_sign_in_required(&on, get(settings::SETTINGS_PATH, &[]), "a read").await;
    assert_sign_in_required(&on, put_settings(&[], &settings_body("7-days")), "a change").await;
    assert_eq!(
        code(
            send(
                &on,
                put_settings(&[("cookie", &cookie)], &settings_body("7-days"))
            )
            .await
        )
        .await,
        (StatusCode::FORBIDDEN, Some("csrf-token-invalid".to_owned()))
    );
    assert_eq!(length_of(&on), domain::access::SessionLength::ThirtyDays);
    let with_both = put_settings(
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
        &settings_body("7-days"),
    );
    assert_eq!(send(&on, with_both).await.status(), StatusCode::OK);
    assert_eq!(length_of(&on), domain::access::SessionLength::SevenDays);
}

#[tokio::test]
async fn a_sign_in_settings_body_that_isnt_one_session_length_is_refused_and_changes_nothing() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    for body in [
        "",
        "{}",
        "\"7-days\"",
        r#"{ "session-length": "2-days" }"#,
        r#"{ "session-length": "7 days" }"#,
        r#"{ "session-length": null }"#,
        r#"{ "session-length": "7-days", "idle": false }"#,
    ] {
        let response = send(
            &on,
            put_settings(&[("cookie", &cookie), ("x-csrf-token", &csrf)], body),
        )
        .await;
        assert_eq!(
            code(response).await,
            (StatusCode::BAD_REQUEST, Some("invalid-body".to_owned())),
            "{body}"
        );
    }
    assert_eq!(length_of(&on), domain::access::SessionLength::ThirtyDays);
}

#[tokio::test]
async fn a_change_without_its_connection_s_address_is_refused_and_counts_for_no_one() {
    let on = on();
    let (cookie, csrf) = signed_in(&on).await;
    for current in ["not-the-password", PASSWORD] {
        let mut request = change_request(&cookie, &csrf, Some(current));
        request
            .extensions_mut()
            .remove::<ConnectInfo<crate::PeerAddr>>();
        assert_eq!(
            code(send(&on, request).await).await,
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Some("sign-in-unavailable".to_owned())
            ),
            "{current}"
        );
    }
    assert_eq!(
        send(&on, change_request(&cookie, &csrf, Some(PASSWORD)))
            .await
            .status(),
        StatusCode::NO_CONTENT,
        "the refused changes made the client wait or changed the password"
    );
}

/// Checks the admin password once, for the sign-in, and can't read it after.
#[derive(Default)]
struct UnreadableAfterSignIn {
    checked: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl AdminPassword for UnreadableAfterSignIn {
    async fn verify(&self, attempt: &Password) -> Result<bool, AdminPasswordError> {
        if self.checked.swap(true, std::sync::atomic::Ordering::SeqCst) {
            Err(AdminPasswordError::Unreadable(
                "/demo/admin-password: denied".into(),
            ))
        } else {
            Ok(attempt.expose() == PASSWORD)
        }
    }

    async fn set(&self, _password: &Password) -> Result<(), AdminPasswordError> {
        Ok(())
    }

    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        Ok(PasswordFingerprint::new(1))
    }
}

#[tokio::test]
async fn a_password_that_cant_be_checked_or_kept_is_unavailable_and_its_detail_stays_in_the_log() {
    let on = on_with(Arc::new(UnreadableAfterSignIn::default()), &clock());
    let (cookie, csrf) = signed_in(&on).await;
    let response = send(&on, change_request(&cookie, &csrf, Some(PASSWORD))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "check");
    assert!(!response.headers().contains_key(SET_COOKIE), "check");
    assert_eq!(
        json(response).await,
        serde_json::json!({ "code": "admin-password-unavailable" }),
        "check"
    );
    let off = off_over(Arc::new(Damaged), Arc::new(InMemorySessionStore::new()));
    let body = serde_json::json!({ "new-password": NEW_PASSWORD }).to_string();
    let response = send(&off, post(ADMIN_PASSWORD, &[], &body)).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "keep");
    assert_eq!(
        json(response).await,
        serde_json::json!({ "code": "admin-password-unavailable" }),
        "keep"
    );
}

#[tokio::test]
async fn a_new_password_too_long_is_refused_with_the_most_it_may_have() {
    let long = "d".repeat(domain::access::PASSWORD_MAX_BYTES + 1);
    let body = serde_json::json!({ "new-password": long }).to_string();
    let response = send(&sign_in_off(), post(ADMIN_PASSWORD, &[], &body)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json(response).await;
    assert_eq!(body["code"], "admin-password-too-long");
    assert_eq!(body["args"]["max"]["value"], 1024);
}

#[tokio::test]
async fn with_sign_in_off_the_sign_in_settings_are_refused() {
    let off = sign_in_off();
    let refusal = (StatusCode::CONFLICT, Some("sign-in-off".to_owned()));
    assert_eq!(
        code(send(&off, get(settings::SETTINGS_PATH, &[])).await).await,
        refusal
    );
    assert_eq!(
        code(send(&off, put_settings(&[], &settings_body("7-days"))).await).await,
        refusal
    );
}

#[tokio::test]
async fn a_session_length_the_store_cant_keep_is_unavailable_and_changes_nothing() {
    let store = Arc::new(Breakable::default());
    let on = SignIn::On(Arc::new(Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(7)),
        clock(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        store.clone(),
    )));
    let (cookie, csrf) = signed_in(&on).await;
    store
        .broken
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let response = send(
        &on,
        put_settings(
            &[("cookie", &cookie), ("x-csrf-token", &csrf)],
            &settings_body("7-days"),
        ),
    )
    .await;
    assert_eq!(
        code(response).await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("sessions-unavailable".to_owned())
        )
    );
    assert_eq!(length_of(&on), domain::access::SessionLength::ThirtyDays);
}

/// Sessions kept from when sign-in was on, before the admin password file
/// went: a password Settings sets with sign-in off has the store forget them.
#[tokio::test]
async fn with_sign_in_off_a_password_set_in_settings_has_the_store_forget_every_kept_session() {
    use app::ports::SessionStore;
    let sessions = Arc::new(InMemorySessionStore::new());
    let was_on = SignIn::On(Arc::new(Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(7)),
        clock(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        sessions.clone(),
    )));
    signed_in(&was_on).await;
    assert_eq!(sessions.all().await.unwrap().len(), 1);
    let off = off_over(Arc::new(InMemoryAdminPassword::default()), sessions.clone());
    let body = serde_json::json!({ "new-password": NEW_PASSWORD }).to_string();
    let response = send(&off, post(ADMIN_PASSWORD, &[], &body)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(sessions.all().await.unwrap(), Vec::new());
}

fn access_of(sign_in: &SignIn) -> Arc<Access> {
    match sign_in {
        SignIn::On(access) => access.clone(),
        SignIn::Off { .. } => panic!("sign-in is on"),
    }
}

/// A session signed in from `HOME` with Keep me signed in on this device:
/// the `Cookie` header that carries it, the whole `Set-Cookie` of the
/// sign-in, and its answer.
async fn kept_signed_in(sign_in: &SignIn) -> (String, String, serde_json::Value) {
    let body = serde_json::json!({ "password": PASSWORD, "keep-signed-in": true });
    let response = send(sign_in, post(SESSION_PATH, &[], &body.to_string())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = set_cookie(&response);
    let pair = cookie.split(';').next().unwrap().to_owned();
    (pair, cookie, json(response).await)
}

#[tokio::test]
async fn a_session_kept_signed_in_is_said_to_have_no_idle_limit_and_its_cookie_lasts_as_long_as_it()
{
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (pair, cookie, body) = kept_signed_in(&on).await;
    assert_eq!(
        body,
        told_the_build(serde_json::json!({
            "sign-in": "signed-in",
            "csrf-token": body["csrf-token"],
            "ends-in-seconds": 2_592_000
        }))
    );
    assert_eq!(
        cookie,
        format!("{pair}; Path=/; Max-Age=2592000; HttpOnly; SameSite=Strict")
    );
    clock.advance(SignedDuration::from_hours(13));
    let response = send(&on, get(SESSION_PATH, &[("cookie", &pair)])).await;
    let read = json(response).await;
    assert_eq!(read["sign-in"], "signed-in", "{read}");
    assert_eq!(read.get("idle-seconds"), None, "{read}");
}

#[tokio::test]
async fn until_the_browser_closes_offers_no_keep_me_signed_in_and_sets_a_cookie_with_no_max_age() {
    let on = on();
    access_of(&on)
        .set_session_length(domain::access::SessionLength::UntilBrowserCloses)
        .await
        .unwrap();
    let response = send(&on, get(SESSION_PATH, &[])).await;
    assert_eq!(
        json(response).await,
        serde_json::json!({ "sign-in": "signed-out", "keep-signed-in-offered": false })
    );
    let (pair, cookie, body) = kept_signed_in(&on).await;
    assert_eq!(cookie, format!("{pair}; Path=/; HttpOnly; SameSite=Strict"));
    assert_eq!(body["idle-seconds"], 43_200, "{body}");
    assert_eq!(body.get("ends-in-seconds"), None, "{body}");
}

#[tokio::test]
async fn a_use_renews_the_cookie_once_a_day_and_a_read_on_its_own_never_does() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (pair, _, _) = kept_signed_in(&on).await;
    clock.advance(SignedDuration::from_hours(24));
    let on_its_own = send(
        &on,
        get(
            "/api/v1/plans",
            &[("cookie", &pair), (api::BACKGROUND_HEADER, "1")],
        ),
    )
    .await;
    assert_eq!(on_its_own.headers().get(SET_COOKIE), None);
    let used = send(&on, get("/api/v1/plans", &[("cookie", &pair)])).await;
    assert_eq!(
        set_cookie(&used),
        format!("{pair}; Path=/; Max-Age=2505600; HttpOnly; SameSite=Strict")
    );
    clock.advance(SignedDuration::from_hours(1));
    let again = send(&on, get("/api/v1/plans", &[("cookie", &pair)])).await;
    assert_eq!(again.headers().get(SET_COOKIE), None);
    // The session route renews it too, as the sign-in set it: an `Origin`
    // of HTTPS on the renewing request changes nothing.
    clock.advance(SignedDuration::from_hours(23));
    let asked = send(
        &on,
        get(
            SESSION_PATH,
            &[("cookie", &pair), ("origin", "https://127.0.0.1:8080")],
        ),
    )
    .await;
    assert_eq!(
        set_cookie(&asked),
        format!("{pair}; Path=/; Max-Age=2419200; HttpOnly; SameSite=Strict")
    );
}

#[tokio::test]
async fn a_session_read_the_page_sends_on_its_own_never_keeps_the_session() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (on_its_own, _) = signed_in(&on).await;
    let (by_a_person, _) = signed_in(&on).await;
    clock.advance(half_idle());
    for (cookie, background) in [(&on_its_own, true), (&by_a_person, false)] {
        let mut headers = vec![("cookie", cookie.as_str())];
        if background {
            headers.push((api::BACKGROUND_HEADER, "1"));
        }
        let read = json(send(&on, get(SESSION_PATH, &headers)).await).await;
        assert_eq!(
            read["sign-in"], "signed-in",
            "background {background}: {read}"
        );
    }
    clock.advance(half_idle());
    assert_sign_in_required(
        &on,
        get("/api/v1/plans", &[("cookie", &on_its_own)]),
        "12 hours after signing in, the session read sent on its own",
    )
    .await;
    assert_let_through(
        &on,
        get("/api/v1/plans", &[("cookie", &by_a_person)]),
        "12 hours after signing in, 6 after the session read a person caused",
    )
    .await;
}

#[tokio::test]
async fn a_session_read_on_its_own_never_renews_the_cookie_and_one_a_person_causes_does() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (pair, _, _) = kept_signed_in(&on).await;
    clock.advance(SignedDuration::from_hours(24));
    let on_its_own = send(
        &on,
        get(
            SESSION_PATH,
            &[("cookie", &pair), (api::BACKGROUND_HEADER, "1")],
        ),
    )
    .await;
    assert_eq!(on_its_own.headers().get(SET_COOKIE), None);
    let read = json(on_its_own).await;
    assert_eq!(read["sign-in"], "signed-in", "{read}");
    let asked = send(&on, get(SESSION_PATH, &[("cookie", &pair)])).await;
    assert_eq!(
        set_cookie(&asked),
        format!("{pair}; Path=/; Max-Age=2505600; HttpOnly; SameSite=Strict")
    );
}

#[tokio::test]
async fn a_renewal_is_secure_as_its_sign_in_was_though_a_same_origin_read_has_no_origin() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let body = serde_json::json!({ "password": PASSWORD, "keep-signed-in": true });
    let origin = format!("https://{OWN_HOST}");
    let response = send(
        &on,
        post(SESSION_PATH, &[("origin", &origin)], &body.to_string()),
    )
    .await;
    let cookie = set_cookie(&response);
    assert!(cookie.ends_with("; Secure"), "{cookie}");
    let pair = cookie.split(';').next().unwrap().to_owned();
    clock.advance(SignedDuration::from_hours(24));
    let used = send(&on, get("/api/v1/plans", &[("cookie", &pair)])).await;
    assert_eq!(
        set_cookie(&used),
        format!("{pair}; Path=/; Max-Age=2505600; HttpOnly; SameSite=Strict; Secure")
    );
    clock.advance(SignedDuration::from_hours(24));
    let asked = send(&on, get(SESSION_PATH, &[("cookie", &pair)])).await;
    assert_eq!(
        set_cookie(&asked),
        format!("{pair}; Path=/; Max-Age=2419200; HttpOnly; SameSite=Strict; Secure")
    );
}

#[tokio::test]
async fn a_renewal_never_replaces_the_cookie_a_route_sets_and_rides_on_a_refusal() {
    let clock = clock();
    let on = on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock);
    let (pair, _, body) = kept_signed_in(&on).await;
    let csrf = body["csrf-token"].as_str().unwrap().to_owned();
    clock.advance(SignedDuration::from_hours(24));
    // A changed request without its CSRF token is refused, and the use that
    // renewed the cookie still says so.
    let refused = send(&on, post("/api/v1/plans", &[("cookie", &pair)], "{}")).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        set_cookie(&refused),
        format!("{pair}; Path=/; Max-Age=2505600; HttpOnly; SameSite=Strict")
    );
    clock.advance(SignedDuration::from_hours(24));
    let response = send(&on, change_request(&pair, &csrf, Some(PASSWORD))).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
}

fn sign_out_everywhere_request(headers: &[(&str, &str)]) -> Request {
    let mut all = vec![("content-type", "application/json")];
    all.extend_from_slice(headers);
    build(Method::DELETE, SESSIONS_PATH, &all, "")
}

#[tokio::test]
async fn signing_out_everywhere_ends_every_session_this_one_too_and_leaves_api_tokens_alone() {
    use app::ports::SessionStore;
    let sessions = Arc::new(InMemorySessionStore::new());
    let on = SignIn::On(Arc::new(Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(7)),
        clock(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        sessions.clone(),
    )));
    let (cookie, csrf) = signed_in(&on).await;
    let (other, _) = signed_in(&on).await;
    let created = access_of(&on)
        .create_api_token(
            domain::access::api_token::ApiTokenLabel::new("nightly-mcp".to_owned()).unwrap(),
            domain::access::api_token::TokenPermissions::new(
                domain::access::permissions::Permissions::default(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let bearer = format!("Bearer {}", created.text.expose());
    let refused = send(&on, sign_out_everywhere_request(&[("cookie", &cookie)])).await;
    assert_eq!(
        code(refused).await,
        (StatusCode::FORBIDDEN, Some("csrf-token-invalid".to_owned())),
        "without its CSRF token"
    );
    let response = send(
        &on,
        sign_out_everywhere_request(&[("cookie", &cookie), ("x-csrf-token", &csrf)]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
    assert_eq!(
        json(response).await,
        serde_json::json!({ "sign-in": "signed-out", "keep-signed-in-offered": true })
    );
    assert_sign_in_required(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "this one",
    )
    .await;
    assert_sign_in_required(&on, get("/api/v1/plans", &[("cookie", &other)]), "another").await;
    assert_eq!(sessions.all().await.unwrap(), Vec::new());
    let read = send(&on, get("/api/v1/plans", &[("authorization", &bearer)])).await;
    assert_eq!(read.status(), StatusCode::OK, "the API token");
}

#[tokio::test]
async fn with_sign_in_off_signing_out_everywhere_is_refused() {
    assert_eq!(
        code(send(&sign_in_off(), sign_out_everywhere_request(&[])).await).await,
        (StatusCode::CONFLICT, Some("sign-in-off".to_owned()))
    );
}

#[tokio::test]
async fn a_sign_out_everywhere_the_store_cant_keep_is_unavailable_yet_ends_every_session_and_forgets_the_cookie()
 {
    let store = Arc::new(Breakable::default());
    let on = SignIn::On(Arc::new(Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(7)),
        clock(),
        Arc::new(app::fakes::InMemoryApiTokenStore::new()),
        store.clone(),
    )));
    let (cookie, csrf) = signed_in(&on).await;
    let (other, _) = signed_in(&on).await;
    store
        .broken
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let response = send(
        &on,
        sign_out_everywhere_request(&[("cookie", &cookie), ("x-csrf-token", &csrf)]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        set_cookie(&response),
        format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict")
    );
    assert_eq!(
        json(response).await,
        serde_json::json!({ "code": "sessions-unavailable" })
    );
    assert_sign_in_required(
        &on,
        get("/api/v1/plans", &[("cookie", &cookie)]),
        "this one",
    )
    .await;
    assert_sign_in_required(&on, get("/api/v1/plans", &[("cookie", &other)]), "another").await;
}
