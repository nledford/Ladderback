use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use app::fakes::{FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, SeededRandom};
use app::ports::{Added, ApiTokenStore, StoreError};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::header::{HOST, ORIGIN, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::Response;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use domain::access::api_token::{ApiToken, ApiTokenId, ApiTokenLabel, Scope, Scopes};
use domain::execution::Trigger;
use futures::StreamExt;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{AppState, CancelError, Canceller, RequestChecks, SignIn, StartError, router};

const PASSWORD: &str = "demo-admin-pass";
const OWN_HOST: &str = "127.0.0.1:8080";
const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// A daemon with sign-in on, its access and the operations it was asked for.
struct Daemon {
    access: Arc<Access>,
    operations: Arc<Recorder>,
    clock: Arc<FakeClock>,
}

impl Daemon {
    fn new() -> Self {
        Self::over(Arc::new(InMemoryApiTokenStore::new()))
    }

    fn over(tokens: Arc<dyn ApiTokenStore>) -> Self {
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        ));
        Self {
            access: Arc::new(Access::new(
                Arc::new(InMemoryAdminPassword::with(PASSWORD)),
                Arc::new(SeededRandom::new(7)),
                clock.clone(),
                tokens,
                Arc::new(app::fakes::InMemorySessionStore::new()),
            )),
            operations: Arc::new(Recorder::default()),
            clock,
        }
    }

    fn state(&self) -> AppState {
        let mut state = test_state();
        state.sign_in = SignIn::On(Arc::clone(&self.access));
        state.operations = self.operations.clone();
        state
    }

    async fn send(&self, request: Request) -> Response {
        router(Arc::new(NoUi), RequestChecks::default(), self.state())
            .oneshot(request)
            .await
            .unwrap()
    }

    /// A token with `scopes`, as a program sends it: `Bearer <text>`.
    async fn token(&self, scopes: &[Scope]) -> String {
        let created = self
            .access
            .create_api_token(
                ApiTokenLabel::new("nightly-mcp".to_owned()).unwrap(),
                Scopes::new(scopes.iter().copied()).unwrap(),
            )
            .await
            .unwrap();
        format!("Bearer {}", created.text.expose())
    }

    /// A session's `Cookie` header and its CSRF token.
    async fn signed_in(&self) -> (String, String) {
        let response = self
            .send(post(
                "/api/v1/session",
                &[],
                &serde_json::json!({ "password": PASSWORD }).to_string(),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[SET_COOKIE].to_str().unwrap().to_owned();
        let pair = cookie.split(';').next().unwrap().to_owned();
        let body = json(response).await;
        (pair, body["csrf-token"].as_str().unwrap().to_owned())
    }
}

/// Records what it was asked to start and cancel. Cancelling `prune-op` is
/// refused to a token, as the daemon refuses any operation but a backup.
#[derive(Default)]
struct Recorder {
    started: Mutex<Vec<Trigger>>,
    cancelled: Mutex<Vec<Canceller>>,
}

#[async_trait]
impl crate::Operations for Recorder {
    async fn start_backup(&self, _plan: String, trigger: Trigger) -> Result<String, StartError> {
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(trigger);
        Ok("018f0000-0000-7000-8000-000000000001".to_owned())
    }

    async fn start_maintenance(
        &self,
        _repository: String,
        _request: crate::MaintenanceRequest,
        _selection: Option<String>,
    ) -> Result<String, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn preview(
        &self,
        _repository: String,
        _request: crate::MaintenanceRequest,
    ) -> Result<api::MaintenancePreviewDto, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn groups(
        &self,
        _repository: String,
    ) -> Result<api::SnapshotGroupsDto, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn stats(
        &self,
        _repository: String,
    ) -> Result<api::RepositoryStatsDto, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn read_stats(
        &self,
        _repository: String,
    ) -> Result<api::StatsReadingDto, crate::MaintenanceRefusal> {
        Err(crate::MaintenanceRefusal::UnknownRepository)
    }

    async fn history(
        &self,
        _plan: Option<String>,
        _after: Option<String>,
        _limit: Option<u64>,
    ) -> Result<api::OperationsPageDto, crate::HistoryError> {
        Err(crate::HistoryError::BadCursor)
    }

    async fn operation(&self, _id: String) -> Result<api::OperationDetailDto, crate::LookupError> {
        Err(crate::LookupError::Unknown)
    }

    async fn cancel(&self, id: String, by: Canceller) -> Result<(), CancelError> {
        self.cancelled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(by);
        if id == "prune-op" && by == Canceller::ApiToken {
            return Err(CancelError::SessionOnly);
        }
        Ok(())
    }
}

/// A store that can't be read or written.
struct Broken;

#[async_trait]
impl ApiTokenStore for Broken {
    async fn add(&self, _token: &ApiToken, _at_most: usize) -> Result<Added, StoreError> {
        Err(broken())
    }
    async fn get(&self, _id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        Err(broken())
    }
    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        Err(broken())
    }
    async fn revoke(&self, _id: &ApiTokenId) -> Result<bool, StoreError> {
        Err(broken())
    }
    async fn revoke_all_but(&self, _password: PasswordFingerprint) -> Result<(), StoreError> {
        Err(broken())
    }
    async fn record_use(&self, _id: &ApiTokenId, _at: Timestamp) -> Result<(), StoreError> {
        Err(broken())
    }
}

fn broken() -> StoreError {
    StoreError::Unavailable {
        detail: "the disk is gone".to_owned(),
    }
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

fn post(path: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut all = vec![("content-type", "application/json")];
    all.extend_from_slice(headers);
    build(Method::POST, path, &all, body)
}

fn delete(path: &str, headers: &[(&str, &str)]) -> Request {
    let mut all = vec![("content-type", "application/json")];
    all.extend_from_slice(headers);
    build(Method::DELETE, path, &all, "{}")
}

async fn text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn json(response: Response) -> serde_json::Value {
    serde_json::from_str(&text(response).await).unwrap_or(serde_json::Value::Null)
}

/// The status, the refusal's code and its arguments.
async fn refusal(response: Response) -> (StatusCode, Option<String>, serde_json::Value) {
    let status = response.status();
    let body = json(response).await;
    let code = body["code"].as_str().map(str::to_owned);
    (status, code, body["args"].clone())
}

async fn assert_needs(daemon: &Daemon, request: Request, scope: &str) {
    let what = format!("{} {}", request.method(), request.uri());
    let (status, code, args) = refusal(daemon.send(request).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("api-token-scope")),
        "{what}"
    );
    assert_eq!(
        args,
        serde_json::json!({ "scope": { "type": "text", "value": scope } }),
        "{what}"
    );
}

#[test]
fn the_scope_table_names_each_route_a_token_may_use_and_no_other() {
    use Scope::{Preview, Read, Run};
    for (method, path, scope) in [
        (Method::GET, "/api/v1/events", Some(Read)),
        // The recent log, read as Operations is read (#618).
        (Method::GET, "/api/v1/log", Some(Read)),
        (Method::HEAD, "/api/v1/log", Some(Read)),
        (Method::DELETE, "/api/v1/log", None),
        (Method::GET, "/api/v1/operations", Some(Read)),
        (Method::HEAD, "/api/v1/operations", Some(Read)),
        (Method::GET, "/api/v1/operations/018f", Some(Read)),
        (Method::GET, "/api/v1/plans", Some(Read)),
        (Method::GET, "/api/v1/repositories", Some(Read)),
        (
            Method::GET,
            "/api/v1/repositories/home/snapshots",
            Some(Read),
        ),
        (Method::GET, "/api/v1/repositories/home/groups", Some(Read)),
        (
            Method::GET,
            "/api/v1/repositories/home/snapshots/abcd/folder",
            Some(Read),
        ),
        (
            Method::HEAD,
            "/api/v1/repositories/home/snapshots/abcd/folder",
            Some(Read),
        ),
        (Method::GET, "/api/v1/repositories/home/stats", Some(Read)),
        (Method::POST, "/api/v1/repositories/home/stats", Some(Run)),
        (Method::POST, "/api/v1/operations", Some(Run)),
        (Method::POST, "/api/v1/operations/018f/cancel", Some(Run)),
        (Method::POST, "/api/v1/repositories/home/test", Some(Run)),
        (
            Method::POST,
            "/api/v1/repositories/home/maintenance/preview",
            Some(Preview),
        ),
        (Method::GET, "/api/v1/plans/home/profile", Some(Read)),
        (Method::HEAD, "/api/v1/plans/home/profile", Some(Read)),
        (Method::POST, "/api/v1/profiles/preview", Some(Preview)),
        // A `read` token reads the session route: the build, never a session.
        (Method::GET, "/api/v1/session", Some(Read)),
        (Method::HEAD, "/api/v1/session", Some(Read)),
        // Session-only: signing in and out, the tokens, configuration
        // changes, maintenance, and every route the table doesn't name.
        (Method::POST, "/api/v1/session", None),
        (Method::DELETE, "/api/v1/session", None),
        (Method::DELETE, "/api/v1/sessions", None),
        (Method::GET, "/api/v1/tokens", None),
        (Method::POST, "/api/v1/tokens", None),
        (Method::GET, "/api/v1/settings/sign-in", None),
        (Method::PUT, "/api/v1/settings/sign-in", None),
        (Method::GET, "/api/v1/restart", None),
        (Method::POST, "/api/v1/restart", None),
        (Method::DELETE, "/api/v1/restart", None),
        (Method::GET, "/api/v1/full-disk-access", None),
        (Method::POST, "/api/v1/full-disk-access/check", None),
        (Method::POST, "/api/v1/full-disk-access/settings", None),
        (Method::DELETE, "/api/v1/tokens/abcdefghijkl", None),
        (Method::POST, "/api/v1/repositories", None),
        (Method::POST, "/api/v1/repositories/_test", None),
        (Method::POST, "/api/v1/repositories/_init", None),
        (Method::POST, "/api/v1/repositories/home/init", None),
        (Method::POST, "/api/v1/repositories/home/maintenance", None),
        // A restore needs step-up, which no token can carry (§10.5).
        (
            Method::POST,
            "/api/v1/repositories/home/snapshots/abcd/restore",
            None,
        ),
        (
            Method::POST,
            "/api/v1/repositories/home/snapshots/abcd/restore/preview",
            None,
        ),
        (Method::PUT, "/api/v1/repositories/home/maintainer", None),
        (Method::GET, "/api/v1/plans/home", None),
        (Method::PUT, "/api/v1/plans/home", None),
        (Method::DELETE, "/api/v1/plans/home", None),
        (Method::GET, "/api/v1/no-such-route", None),
        (Method::GET, "/api/v1", None),
        (Method::GET, "/api/v1/", None),
        (Method::GET, "/api/v1/plans/", None),
        (Method::GET, "/api/v1/operations//", None),
        (Method::GET, "/api/v1/repositories//snapshots", None),
        (Method::DELETE, "/api/v1/operations/018f", None),
        (Method::POST, "/api/v1/plans", None),
        (Method::PUT, "/api/v1/operations", None),
        (Method::OPTIONS, "/api/v1/operations", None),
        (Method::GET, "/api/v1/operations/018f/cancel", None),
        (
            Method::GET,
            "/api/v1/repositories/home/maintenance/preview",
            None,
        ),
        (Method::POST, "/api/v1/plans/home/profile", None),
        (Method::GET, "/api/v1/profiles/preview", None),
        (Method::POST, "/api/v1/profiles", None),
        (Method::GET, "/api/v1/plans//profile", None),
        (
            Method::POST,
            "/api/v1/repositories/home/snapshots/abcd/folder",
            None,
        ),
        (
            Method::GET,
            "/api/v1/repositories/home/snapshots/abcd",
            None,
        ),
        (
            Method::GET,
            "/api/v1/repositories/home/snapshots//folder",
            None,
        ),
    ] {
        assert_eq!(required_scope(&method, path), scope, "{method} {path}");
    }
}

#[tokio::test]
async fn a_read_token_reads_without_a_session_or_a_csrf_token() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let response = daemon
        .send(get("/api/v1/plans", &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// The snapshot tree reads a folder with the `read` scope, as it reads the
/// list of snapshots (decision 93).
#[tokio::test]
async fn a_read_token_reads_a_snapshot_s_folder() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let response = daemon
        .send(get(&folder_of_a_snapshot(), &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// The folder route of a snapshot, at its top.
fn folder_of_a_snapshot() -> String {
    format!(
        "/api/v1/repositories/home/snapshots/{}/folder?path=",
        "ab".repeat(32)
    )
}

#[tokio::test]
async fn the_bearer_scheme_is_read_in_any_case() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let lower = token.replacen("Bearer", "bearer", 1);
    let response = daemon
        .send(get("/api/v1/plans", &[("authorization", &lower)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_token_without_the_routes_scope_is_refused_naming_the_scope() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let auth = [("authorization", token.as_str())];
    assert_needs(
        &daemon,
        post("/api/v1/operations", &auth, r#"{"plan":"home"}"#),
        "run",
    )
    .await;
    assert_needs(
        &daemon,
        post("/api/v1/repositories/home/maintenance/preview", &auth, "{}"),
        "preview",
    )
    .await;
    assert!(daemon.operations.started.lock().unwrap().is_empty());
    let token = daemon.token(&[Scope::Run, Scope::Preview]).await;
    let auth = [("authorization", token.as_str())];
    assert_needs(&daemon, get("/api/v1/plans", &auth), "read").await;
    assert_needs(&daemon, get(&folder_of_a_snapshot(), &auth), "read").await;
}

#[tokio::test]
async fn a_token_never_opens_a_route_for_a_session_only() {
    let daemon = Daemon::new();
    let token = daemon
        .token(&[Scope::Read, Scope::Run, Scope::Preview, Scope::Propose])
        .await;
    let auth = [("authorization", token.as_str())];
    for request in [
        post("/api/v1/session", &auth, "{}"),
        get("/api/v1/tokens", &auth),
        post(
            "/api/v1/tokens",
            &auth,
            r#"{"label":"x","scopes":["read"]}"#,
        ),
        delete("/api/v1/tokens/abcdefghijkl", &auth),
        post("/api/v1/repositories", &auth, "{}"),
        post("/api/v1/repositories/_test", &auth, "{}"),
        post("/api/v1/repositories/home/maintenance", &auth, "{}"),
        post(
            &format!(
                "/api/v1/repositories/home/snapshots/{}/restore",
                "ab".repeat(32)
            ),
            &auth,
            r#"{"path":"","destination":"new-folder","confirmation":"abababab"}"#,
        ),
        post(
            &format!(
                "/api/v1/repositories/home/snapshots/{}/restore/preview",
                "ab".repeat(32)
            ),
            &auth,
            r#"{"path":""}"#,
        ),
        post("/api/v1/plans", &auth, "{}"),
        get("/api/v1/plans/home", &auth),
        build(
            Method::PUT,
            "/api/v1/plans/home",
            &[("content-type", "application/json"), auth[0]],
            "{}",
        ),
        delete("/api/v1/plans/home", &auth),
        get("/api/v1/no-such-route", &auth),
    ] {
        assert_needs(&daemon, request, "session").await;
    }
}

/// A `read` token is told the build the daemon runs, as a signed-in page is,
/// and that it has no session, even beside a live session's cookie
/// (decision 93); a token without `read` is refused, naming it.
#[tokio::test]
async fn a_read_token_reads_the_build_on_the_session_route_and_never_a_session() {
    let daemon = Daemon::new();
    let (cookie, _) = daemon.signed_in().await;
    let token = daemon.token(&[Scope::Read]).await;
    daemon.clock.advance(domain::SignedDuration::from_hours(1));
    let background = (api::BACKGROUND_HEADER, "1");
    for headers in [
        vec![("authorization", token.as_str())],
        vec![("authorization", token.as_str()), background],
        vec![
            ("authorization", token.as_str()),
            ("cookie", cookie.as_str()),
        ],
        vec![
            ("authorization", token.as_str()),
            ("cookie", cookie.as_str()),
            background,
        ],
    ] {
        let response = daemon.send(get("/api/v1/session", &headers)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(SET_COOKIE), None, "{headers:?}");
        let body = json(response).await;
        assert_eq!(body["sign-in"], "signed-out", "{body}");
        assert_eq!(body["csrf-token"], serde_json::Value::Null, "{body}");
        assert_eq!(body.get("keep-signed-in-offered"), None, "{body}");
        assert!(body["version"].is_string(), "{body}");
    }
    // The token's reads never used the browser's session: its end hasn't
    // moved, and 12 hours after signing in it has gone idle.
    let read = async || {
        let session = daemon
            .send(get(
                "/api/v1/session",
                &[("cookie", cookie.as_str()), background],
            ))
            .await;
        assert_eq!(session.headers().get(SET_COOKIE), None);
        json(session).await
    };
    let session = read().await;
    assert_eq!(session["sign-in"], "signed-in", "{session}");
    assert_eq!(session["idle-seconds"], 43_200, "{session}");
    assert_eq!(session["ends-in-seconds"], 2_592_000 - 3_600, "{session}");
    daemon.clock.advance(domain::SignedDuration::from_hours(11));
    let session = read().await;
    assert_eq!(session["sign-in"], "signed-out", "{session}");
    let run = daemon.token(&[Scope::Run]).await;
    assert_needs(
        &daemon,
        get("/api/v1/session", &[("authorization", &run)]),
        "read",
    )
    .await;
}

/// A good token is judged alone even beside a live session and its CSRF
/// token: the session doesn't open what the token may not (decision 93).
#[tokio::test]
async fn a_good_token_beside_a_live_session_still_cannot_use_a_session_only_route() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let token = daemon
        .token(&[Scope::Read, Scope::Run, Scope::Preview, Scope::Propose])
        .await;
    let both = [
        ("authorization", token.as_str()),
        ("cookie", cookie.as_str()),
        ("x-csrf-token", csrf.as_str()),
    ];
    for request in [
        get("/api/v1/tokens", &both),
        post(
            "/api/v1/tokens",
            &both,
            r#"{"label":"x","scopes":["read"]}"#,
        ),
        delete("/api/v1/tokens/abcdefghijkl", &both),
        post("/api/v1/repositories/home/maintenance", &both, "{}"),
        get("/api/v1/restart", &both),
        post("/api/v1/restart", &both, r#"{"when":"now"}"#),
        delete("/api/v1/restart", &both),
    ] {
        assert_needs(&daemon, request, "session").await;
    }
}

/// No operations, for a daemon that offers a restart.
struct Idle;

impl crate::Work for Idle {
    fn hold(&self) {}

    fn release(&self) {}

    fn running(&self) -> usize {
        0
    }
}

#[tokio::test]
async fn the_signed_in_admin_restarts_with_the_csrf_token_and_is_named_as_who_asked() {
    use futures::FutureExt;
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let restart = crate::Restart::new(true, Arc::new(Idle));
    let send = |request: Request| {
        let mut state = daemon.state();
        state.restart = restart.clone();
        router(Arc::new(NoUi), RequestChecks::default(), state).oneshot(request)
    };
    let without_csrf = send(post(
        "/api/v1/restart",
        &[("cookie", &cookie)],
        r#"{"when":"now"}"#,
    ))
    .await
    .unwrap();
    assert_eq!(without_csrf.status(), StatusCode::FORBIDDEN);
    assert_eq!(restart.due().now_or_never(), None, "restarted without CSRF");
    let asked = send(post(
        "/api/v1/restart",
        &[("cookie", &cookie), ("x-csrf-token", &csrf)],
        r#"{"when":"now"}"#,
    ))
    .await
    .unwrap();
    assert_eq!(asked.status(), StatusCode::ACCEPTED);
    assert_eq!(
        restart.due().now_or_never(),
        Some(crate::RestartAsker::SignedInAdmin)
    );
}

#[tokio::test]
async fn a_wrong_token_is_refused_and_the_session_cookie_beside_it_does_not_help() {
    let daemon = Daemon::new();
    let (cookie, _) = daemon.signed_in().await;
    let token = daemon.token(&[Scope::Read]).await;
    let mut wrong = token.clone();
    wrong.pop();
    wrong.push(if token.ends_with('A') { 'B' } else { 'A' });
    let response = daemon
        .send(get(
            "/api/v1/plans",
            &[("authorization", &wrong), ("cookie", &cookie)],
        ))
        .await;
    let (status, code, _) = refusal(response).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::UNAUTHORIZED, Some("api-token-invalid"))
    );
}

#[tokio::test]
async fn text_that_is_no_token_is_refused_as_invalid() {
    let daemon = Daemon::new();
    for auth in [
        "Bearer",
        "Bearer ",
        "Bearer not-a-token",
        "Bearer ladderback_x_y",
    ] {
        let daemon_fresh = Daemon::new();
        let (status, code, _) = refusal(
            daemon_fresh
                .send(get("/api/v1/plans", &[("authorization", auth)]))
                .await,
        )
        .await;
        assert_eq!(
            (status, code.as_deref()),
            (StatusCode::UNAUTHORIZED, Some("api-token-invalid")),
            "{auth:?}"
        );
    }
    // Two tokens in one request are no token.
    let token = daemon.token(&[Scope::Read]).await;
    let (status, code, _) = refusal(
        daemon
            .send(get(
                "/api/v1/plans",
                &[("authorization", &token), ("authorization", &token)],
            ))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::UNAUTHORIZED, Some("api-token-invalid"))
    );
}

#[tokio::test]
async fn another_authorization_scheme_is_ignored_and_the_request_judged_by_its_session() {
    let daemon = Daemon::new();
    let basic = [("authorization", "Basic YWRtaW46ZGVtbw==")];
    let (status, code, _) = refusal(daemon.send(get("/api/v1/plans", &basic)).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::UNAUTHORIZED, Some("sign-in-required"))
    );
    let (cookie, _) = daemon.signed_in().await;
    let response = daemon
        .send(get(
            "/api/v1/plans",
            &[
                ("authorization", "Basic YWRtaW46ZGVtbw=="),
                ("cookie", &cookie),
            ],
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn after_a_wrong_token_the_next_waits_unchecked_and_so_does_a_sign_in() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let response = daemon
        .send(get(
            "/api/v1/plans",
            &[("authorization", "Bearer ladderback_x_y")],
        ))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = daemon
        .send(get("/api/v1/plans", &[("authorization", &token)]))
        .await;
    assert_eq!(response.headers()[RETRY_AFTER], "1");
    let (status, code, args) = refusal(response).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::TOO_MANY_REQUESTS, Some("sign-in-wait"))
    );
    assert_eq!(
        args,
        serde_json::json!({ "seconds": { "type": "count", "value": 1 } })
    );
    let response = daemon
        .send(post(
            "/api/v1/session",
            &[],
            &serde_json::json!({ "password": PASSWORD }).to_string(),
        ))
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_token_request_without_its_connections_address_is_refused() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let request = Request::builder()
        .uri("/api/v1/plans")
        .header(HOST, OWN_HOST)
        .header("authorization", &token)
        .body(Body::empty())
        .unwrap();
    let (status, code, _) = refusal(daemon.send(request).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("sign-in-unavailable"))
    );
}

#[tokio::test]
async fn the_request_checks_come_before_the_token_and_a_refused_one_starts_no_wait() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read, Scope::Run]).await;
    let wrong = "Bearer ladderback_x_y";
    let foreign = build(
        Method::GET,
        "/api/v1/plans",
        &[
            ("authorization", wrong),
            (ORIGIN.as_str(), "https://evil.example"),
        ],
        "",
    );
    let (_, code, _) = refusal(daemon.send(foreign).await).await;
    assert_eq!(code.as_deref(), Some("origin-not-allowed"));
    let not_json = build(
        Method::POST,
        "/api/v1/operations",
        &[("authorization", wrong), ("content-type", "text/plain")],
        r#"{"plan":"home"}"#,
    );
    let (_, code, _) = refusal(daemon.send(not_json).await).await;
    assert_eq!(code.as_deref(), Some("json-required"));
    // Neither counted as a failed token: the good one is let in.
    let response = daemon
        .send(get("/api/v1/plans", &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_to_a_token_too() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Run]).await;
    let body = format!(r#"{{"plan":"{}"}}"#, "x".repeat(2 * 1024 * 1024));
    let (status, code, _) = refusal(
        daemon
            .send(post(
                "/api/v1/operations",
                &[("authorization", &token)],
                &body,
            ))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::PAYLOAD_TOO_LARGE, Some("body-too-large"))
    );
    assert!(daemon.operations.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_run_a_token_starts_is_recorded_as_the_tokens_and_one_by_hand_as_manual() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Run]).await;
    let response = daemon
        .send(post(
            "/api/v1/operations",
            &[("authorization", &token)],
            r#"{"plan":"home"}"#,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let (cookie, csrf) = daemon.signed_in().await;
    let response = daemon
        .send(post(
            "/api/v1/operations",
            &[("cookie", &cookie), ("x-csrf-token", &csrf)],
            r#"{"plan":"home"}"#,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let kept = daemon.access.api_tokens().await.unwrap();
    let started = daemon.operations.started.lock().unwrap().clone();
    assert_eq!(
        started,
        [
            Trigger::ApiToken {
                id: kept[0].id.clone(),
                label: kept[0].label.clone(),
            },
            Trigger::Manual
        ]
    );
}

#[tokio::test]
async fn a_token_cancels_as_a_token_and_what_it_may_not_stop_needs_a_session() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Run]).await;
    let auth = [("authorization", token.as_str())];
    let response = daemon
        .send(post("/api/v1/operations/backup-op/cancel", &auth, "{}"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_needs(
        &daemon,
        post("/api/v1/operations/prune-op/cancel", &auth, "{}"),
        "session",
    )
    .await;
    let (cookie, csrf) = daemon.signed_in().await;
    let response = daemon
        .send(post(
            "/api/v1/operations/prune-op/cancel",
            &[("cookie", &cookie), ("x-csrf-token", &csrf)],
            "{}",
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cancelled = daemon.operations.cancelled.lock().unwrap().clone();
    assert_eq!(
        cancelled,
        [Canceller::ApiToken, Canceller::ApiToken, Canceller::Session]
    );
}

#[tokio::test]
async fn with_sign_in_off_the_token_routes_say_so_and_the_header_is_ignored() {
    let mut state = test_state();
    state.sign_in = crate::request_checks::tests::sign_in_off();
    let send = |request: Request| {
        let state = state.clone();
        async move {
            router(Arc::new(NoUi), RequestChecks::default(), state)
                .oneshot(request)
                .await
                .unwrap()
        }
    };
    for request in [
        get(TOKENS_PATH, &[]),
        post(TOKENS_PATH, &[], r#"{"label":"x","scopes":["read"]}"#),
        delete("/api/v1/tokens/abcdefghijkl", &[]),
    ] {
        let (status, code, _) = refusal(send(request).await).await;
        assert_eq!(
            (status, code.as_deref()),
            (StatusCode::CONFLICT, Some("sign-in-off"))
        );
    }
    let response = send(get(
        "/api/v1/plans",
        &[("authorization", "Bearer ladderback_x_y")],
    ))
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_created_tokens_secret_is_shown_once_and_never_listed() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let response = daemon
        .send(post(
            TOKENS_PATH,
            &session,
            r#"{"label":"nightly-mcp","scopes":["run","read"]}"#,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json(response).await;
    let secret = created["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("ladderback_"), "{secret}");
    assert_eq!(created["token"]["label"], "nightly-mcp");
    assert_eq!(
        created["token"]["scopes"],
        serde_json::json!(["read", "run"])
    );
    assert_eq!(created["token"]["created-at"], "2027-01-15T08:00:00Z");
    assert_eq!(created["token"]["last-used-at"], serde_json::Value::Null);
    let id = created["token"]["id"].as_str().unwrap().to_owned();
    assert!(secret.contains(&id));

    let response = daemon
        .send(get(
            "/api/v1/plans",
            &[("authorization", &format!("Bearer {secret}"))],
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = daemon.send(get(TOKENS_PATH, &session)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let listed = text(response).await;
    let secret_part = secret.rsplit('_').next().unwrap();
    assert!(!listed.contains(secret_part), "{listed}");
    let listed: serde_json::Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(listed["tokens"][0]["id"], id.as_str());
    assert_eq!(listed["tokens"][0]["last-used-at"], "2027-01-15T08:00:00Z");
}

#[tokio::test]
async fn a_token_is_refused_without_a_label_of_1_to_64_characters_or_a_scope() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let long = serde_json::json!({ "label": "x".repeat(65), "scopes": ["read"] }).to_string();
    for (body, status, code, args) in [
        (
            r#"{"label":"","scopes":["read"]}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "api-token-label",
            serde_json::json!({ "max": { "type": "count", "value": 64 } }),
        ),
        (
            long,
            StatusCode::BAD_REQUEST,
            "api-token-label",
            serde_json::json!({ "max": { "type": "count", "value": 64 } }),
        ),
        (
            r#"{"label":"x","scopes":[]}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "api-token-no-scope",
            serde_json::Value::Null,
        ),
        (
            r#"{"label":"x","scopes":["admin"]}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
            serde_json::Value::Null,
        ),
        (
            r#"{"label":"x","scopes":["read"],"expires":1}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
            serde_json::Value::Null,
        ),
    ] {
        let response = daemon.send(post(TOKENS_PATH, &session, &body)).await;
        assert_eq!(
            refusal(response).await,
            (status, Some(code.to_owned()), args),
            "{body}"
        );
    }
    assert!(daemon.access.api_tokens().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_token_over_the_100_kept_is_refused() {
    let daemon = Daemon::new();
    for _ in 0..100 {
        daemon.token(&[Scope::Read]).await;
    }
    let (cookie, csrf) = daemon.signed_in().await;
    let response = daemon
        .send(post(
            TOKENS_PATH,
            &[("cookie", &cookie), ("x-csrf-token", &csrf)],
            r#"{"label":"one-more","scopes":["read"]}"#,
        ))
        .await;
    assert_eq!(
        refusal(response).await,
        (
            StatusCode::CONFLICT,
            Some("api-token-limit".to_owned()),
            serde_json::json!({ "limit": { "type": "count", "value": 100 } })
        )
    );
}

#[tokio::test]
async fn a_revoked_token_is_refused_and_revoking_it_again_finds_none() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let id = daemon.access.api_tokens().await.unwrap()[0].id.clone();
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let path = format!("/api/v1/tokens/{}", id.as_str());
    let response = daemon.send(delete(&path, &session)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (status, code, _) = refusal(
        daemon
            .send(get("/api/v1/plans", &[("authorization", &token)]))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::UNAUTHORIZED, Some("api-token-invalid"))
    );
    let response = daemon.send(delete(&path, &session)).await;
    assert_eq!(
        refusal(response).await,
        (
            StatusCode::NOT_FOUND,
            Some("api-token-unknown".to_owned()),
            serde_json::json!({ "token": { "type": "text", "value": id.as_str() } })
        )
    );
    let response = daemon
        .send(delete("/api/v1/tokens/NOT-AN-ID", &session))
        .await;
    assert_eq!(
        refusal(response).await,
        (
            StatusCode::NOT_FOUND,
            Some("api-token-unknown".to_owned()),
            serde_json::json!({ "token": { "type": "text", "value": "NOT-AN-ID" } })
        )
    );
}

#[tokio::test]
async fn tokens_that_cant_be_read_are_unavailable_and_their_detail_stays_in_the_log() {
    let daemon = Daemon::over(Arc::new(Broken));
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    for request in [
        get(TOKENS_PATH, &session),
        post(TOKENS_PATH, &session, r#"{"label":"x","scopes":["read"]}"#),
        delete("/api/v1/tokens/abcdefghijkl", &session),
        get(
            "/api/v1/plans",
            &[(
                "authorization",
                "Bearer ladderback_abcdefghijkl_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            )],
        ),
    ] {
        let what = format!("{} {}", request.method(), request.uri());
        let response = daemon.send(request).await;
        let status = response.status();
        let body = text(response).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{what}: {body}");
        assert!(body.contains("api-tokens-unavailable"), "{what}: {body}");
        assert!(!body.contains("disk"), "{what}: {body}");
    }
}

#[tokio::test]
async fn an_events_stream_ends_once_the_token_that_opened_it_is_revoked() {
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let daemon = Daemon::new();
    let token = daemon.token(&[Scope::Read]).await;
    let listener = crate::GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::new(NoUi), RequestChecks::default(), daemon.state());
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
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_str(&token).unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    futures::SinkExt::send(&mut socket, Message::Ping(vec![7].into()))
        .await
        .unwrap();
    match socket.next().await {
        Some(Ok(Message::Pong(payload))) => assert_eq!(payload.as_ref(), [7]),
        other => panic!("a kept token's stream answers a ping: {other:?}"),
    }
    let id = daemon.access.api_tokens().await.unwrap()[0].id.clone();
    assert!(daemon.access.revoke_api_token(&id).await.unwrap());
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
    assert!(ended.is_ok(), "the stream outlived its token");
}

#[test]
fn only_a_bearer_header_is_a_token_and_two_are_none() {
    let headers = |values: &[&str]| {
        let mut map = axum::http::HeaderMap::new();
        for value in values {
            map.append(
                axum::http::header::AUTHORIZATION,
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    };
    assert!(bearer(&headers(&[])).is_none());
    assert!(bearer(&headers(&["Basic YWRtaW46ZGVtbw=="])).is_none());
    assert!(bearer(&headers(&["Bearerish x"])).is_none());
    assert!(matches!(bearer(&headers(&["Bearer"])), Some(None)));
    assert!(matches!(bearer(&headers(&["BEARER nope"])), Some(None)));
    assert!(matches!(
        bearer(&headers(&["Bearer a", "Bearer b"])),
        Some(None)
    ));
}
