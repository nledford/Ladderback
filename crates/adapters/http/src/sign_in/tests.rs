use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use app::fakes::{FakeClock, InMemoryAdminPassword, SeededRandom};
use app::ports::{AdminPassword, AdminPasswordError};
use async_trait::async_trait;
use axum::body::Body;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use futures::StreamExt;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
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
    )))
}

fn on() -> SignIn {
    on_with(Arc::new(InMemoryAdminPassword::with(PASSWORD)), &clock())
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
    let off = SignIn::Off;
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
    let off = SignIn::Off;
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
        serde_json::json!({ "sign-in": "signed-out", "exposure": "network" })
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
    for path in ["/", "/index.html", "/app.wasm", "/api", "/api/v10/plans"] {
        assert_let_through(&on, get(path, &[]), path).await;
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
        serde_json::json!({ "sign-in": "signed-out" })
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
        serde_json::json!({ "sign-in": "signed-out" })
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
    access.sign_out(&token);
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
