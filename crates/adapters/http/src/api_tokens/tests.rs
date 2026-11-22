use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use app::fakes::{FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, SeededRandom};
use app::ports::{Added, ApiTokenStore, PermissionsSet, StoreError};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::header::{HOST, ORIGIN, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::Response;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use domain::access::api_token::{ApiToken, ApiTokenId, ApiTokenLabel, TokenPermissions};
use domain::access::permissions::{Permission, Permissions};
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

    /// A token with `permissions`, as a program sends it: `Bearer <text>`.
    async fn token(&self, permissions: &[Permission]) -> String {
        let created = self
            .access
            .create_api_token(
                ApiTokenLabel::new("nightly-mcp".to_owned()).unwrap(),
                TokenPermissions::new(granting(permissions)).unwrap(),
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
        Ok(api::OperationsPageDto {
            operations: vec![failed_on_a_file()],
            next: None,
        })
    }

    async fn operation(&self, _id: String) -> Result<api::OperationDetailDto, crate::LookupError> {
        Ok(api::OperationDetailDto {
            operation: failed_on_a_file(),
            warnings: vec![unreadable()],
        })
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
    async fn set_permissions(
        &self,
        _id: &ApiTokenId,
        _permissions: TokenPermissions,
    ) -> Result<PermissionsSet, StoreError> {
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

/// The permissions that grant exactly `rows`.
fn granting(rows: &[Permission]) -> Permissions {
    rows.iter().fold(Permissions::NONE, |permissions, row| {
        permissions.with(*row, Some(row.level())).unwrap()
    })
}

async fn assert_needs(daemon: &Daemon, request: Request, permission: &str) {
    let what = format!("{} {}", request.method(), request.uri());
    let (status, code, args) = refusal(daemon.send(request).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("api-token-permission")),
        "{what}"
    );
    assert_eq!(
        args,
        serde_json::json!({ "permission": { "type": "text", "value": permission } }),
        "{what}"
    );
}

#[test]
fn the_permission_table_names_each_route_a_token_may_use_and_no_other() {
    use Permission::{BackupRuns, Backups, Configuration, DaemonLog, Previews, SnapshotContents};
    for (method, path, permission) in [
        (Method::GET, "/api/v1/events", Some(Backups)),
        // The daemon's log and the names in a snapshot each need their own.
        (Method::GET, "/api/v1/log", Some(DaemonLog)),
        (Method::HEAD, "/api/v1/log", Some(DaemonLog)),
        (Method::DELETE, "/api/v1/log", None),
        (Method::GET, "/api/v1/operations", Some(Backups)),
        (Method::HEAD, "/api/v1/operations", Some(Backups)),
        (Method::GET, "/api/v1/operations/018f", Some(Backups)),
        (Method::GET, "/api/v1/plans", Some(Backups)),
        (Method::GET, "/api/v1/repositories", Some(Backups)),
        // The status summary, read as the plans are (§4.6).
        (Method::GET, "/api/v1/status", Some(Backups)),
        // Which hub this machine is paired with, read as the status is
        // (§4.6, `pairing_state`): public, and never a way to pair.
        (Method::GET, "/api/v1/pairing", Some(Backups)),
        (Method::HEAD, "/api/v1/pairing", Some(Backups)),
        (Method::POST, "/api/v1/pairing", None),
        (Method::POST, "/api/v1/status", None),
        (
            Method::GET,
            "/api/v1/repositories/home/snapshots",
            Some(Backups),
        ),
        (
            Method::GET,
            "/api/v1/repositories/home/groups",
            Some(Backups),
        ),
        (
            Method::GET,
            "/api/v1/repositories/home/snapshots/abcd/folder",
            Some(SnapshotContents),
        ),
        (
            Method::HEAD,
            "/api/v1/repositories/home/snapshots/abcd/folder",
            Some(SnapshotContents),
        ),
        (
            Method::GET,
            "/api/v1/repositories/home/stats",
            Some(Backups),
        ),
        (
            Method::POST,
            "/api/v1/repositories/home/stats",
            Some(BackupRuns),
        ),
        (Method::POST, "/api/v1/operations", Some(BackupRuns)),
        (
            Method::POST,
            "/api/v1/operations/018f/cancel",
            Some(BackupRuns),
        ),
        (
            Method::POST,
            "/api/v1/repositories/home/test",
            Some(BackupRuns),
        ),
        // A search of the snapshots names what a folder's page names.
        (
            Method::GET,
            "/api/v1/repositories/home/find",
            Some(SnapshotContents),
        ),
        (
            Method::HEAD,
            "/api/v1/repositories/home/find",
            Some(SnapshotContents),
        ),
        (Method::POST, "/api/v1/repositories/home/find", None),
        (
            Method::POST,
            "/api/v1/repositories/home/maintenance/preview",
            Some(Previews),
        ),
        (Method::GET, "/api/v1/plans/home/profile", Some(Backups)),
        (Method::HEAD, "/api/v1/plans/home/profile", Some(Backups)),
        (Method::POST, "/api/v1/profiles/preview", Some(Previews)),
        (Method::GET, "/api/v1/proposals", Some(Configuration)),
        (Method::GET, "/api/v1/proposals/018f", Some(Configuration)),
        (Method::POST, "/api/v1/proposals", Some(Configuration)),
        (
            Method::POST,
            "/api/v1/proposals/018f/withdraw",
            Some(Configuration),
        ),
        (Method::POST, "/api/v1/dry-runs", Some(Configuration)),
        (Method::POST, "/api/v1/proposals/018f/apply", None),
        (Method::PUT, "/api/v1/tokens/abcdefghijkl/permissions", None),
        // A token reads the session route: the build, never a session.
        (Method::GET, "/api/v1/session", Some(Backups)),
        (Method::HEAD, "/api/v1/session", Some(Backups)),
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
        (Method::GET, "/api/v1/machine", None),
        (Method::POST, "/api/v1/machine/new-id", None),
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
        // A download needs step-up too, and its ticket is the browser's.
        (
            Method::POST,
            "/api/v1/repositories/home/snapshots/abcd/download",
            None,
        ),
        (Method::GET, "/api/v1/downloads/abcd", None),
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
        let needs = permission.map_or(Needs::Session, Needs::Permission);
        assert_eq!(required_permission(&method, path), needs, "{method} {path}");
    }
    assert_eq!(
        required_permission(&Method::GET, "/api/v1/permissions"),
        Needs::AnyToken
    );
    assert_eq!(
        required_permission(&Method::POST, "/api/v1/permissions"),
        Needs::Session
    );
}

#[tokio::test]
async fn a_read_token_reads_without_a_session_or_a_csrf_token() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::Backups]).await;
    let response = daemon
        .send(get("/api/v1/plans", &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// An AI agent in connect mode reads the status summary with the `read`
/// scope, as it reads the plans it sums up (§4.6).
#[tokio::test]
async fn a_read_token_reads_the_status_summary_with_what_the_probe_found() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::Backups]).await;
    let response = daemon
        .send(get("/api/v1/status", &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status["full-disk-access"], "not-applicable", "{status}");
}

/// The snapshot tree reads a folder with the `read` scope, as it reads the
/// list of snapshots (decision 93).
#[tokio::test]
async fn a_read_token_reads_a_snapshot_s_folder() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::SnapshotContents]).await;
    let response = daemon
        .send(get(&folder_of_a_snapshot(), &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// An agent searches a repository's snapshots with Snapshot contents, as it
/// reads a snapshot's folder (#953): neither Backups nor Backup runs is
/// needed.
#[tokio::test]
async fn a_token_with_snapshot_contents_searches_a_repository_s_snapshots() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::SnapshotContents]).await;
    let response = daemon
        .send(get(
            "/api/v1/repositories/home/find?pattern=tax",
            &[("authorization", &token)],
        ))
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
    let token = daemon.token(&[Permission::Backups]).await;
    let lower = token.replacen("Bearer", "bearer", 1);
    let response = daemon
        .send(get("/api/v1/plans", &[("authorization", &lower)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_token_without_the_routes_permission_is_refused_naming_the_permission() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::Backups]).await;
    let auth = [("authorization", token.as_str())];
    assert_needs(
        &daemon,
        post("/api/v1/operations", &auth, r#"{"plan":"home"}"#),
        "backup-runs",
    )
    .await;
    assert_needs(
        &daemon,
        post("/api/v1/repositories/home/maintenance/preview", &auth, "{}"),
        "previews",
    )
    .await;
    assert!(daemon.operations.started.lock().unwrap().is_empty());
    let token = daemon
        .token(&[Permission::BackupRuns, Permission::Previews])
        .await;
    let auth = [("authorization", token.as_str())];
    assert_needs(&daemon, get("/api/v1/plans", &auth), "backups").await;
    assert_needs(&daemon, get("/api/v1/log", &auth), "daemon-log").await;
    assert_needs(
        &daemon,
        get(&folder_of_a_snapshot(), &auth),
        "snapshot-contents",
    )
    .await;
    assert_needs(&daemon, get(SEARCH, &auth), "snapshot-contents").await;
    // Backups alone names nothing inside a snapshot.
    let token = daemon.token(&[Permission::Backups]).await;
    let auth = [("authorization", token.as_str())];
    assert_needs(
        &daemon,
        get(&folder_of_a_snapshot(), &auth),
        "snapshot-contents",
    )
    .await;
    assert_needs(&daemon, get(SEARCH, &auth), "snapshot-contents").await;
    // Refused before the repository is looked up: one the daemon doesn't
    // hold is refused alike, so the refusal tells nothing of either.
    assert_needs(
        &daemon,
        get("/api/v1/repositories/missing/find?pattern=tax", &auth),
        "snapshot-contents",
    )
    .await;
    assert_needs(&daemon, get("/api/v1/log", &auth), "daemon-log").await;
}

/// A search of the repository `home` for `tax`.
const SEARCH: &str = "/api/v1/repositories/home/find?pattern=tax";

#[tokio::test]
async fn a_token_never_opens_a_route_for_a_session_only() {
    let daemon = Daemon::new();
    let token = daemon.token(&Permission::ALL).await;
    let auth = [("authorization", token.as_str())];
    for request in [
        post("/api/v1/session", &auth, "{}"),
        get("/api/v1/tokens", &auth),
        post(
            "/api/v1/tokens",
            &auth,
            r#"{"label":"x","permissions":{"backups":"read"}}"#,
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
        post(
            &format!(
                "/api/v1/repositories/home/snapshots/{}/download",
                "ab".repeat(32)
            ),
            &auth,
            r#"{"path":"","format":"tar","confirmation":"abababab"}"#,
        ),
        get(&format!("/api/v1/downloads/{}", "ab".repeat(32)), &auth),
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
    let token = daemon.token(&[Permission::Backups]).await;
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
    let run = daemon.token(&[Permission::BackupRuns]).await;
    assert_needs(
        &daemon,
        get("/api/v1/session", &[("authorization", &run)]),
        "backups",
    )
    .await;
}

/// A good token is judged alone even beside a live session and its CSRF
/// token: the session doesn't open what the token may not (decision 93).
#[tokio::test]
async fn a_good_token_beside_a_live_session_still_cannot_use_a_session_only_route() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let token = daemon.token(&Permission::ALL).await;
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
            r#"{"label":"x","permissions":{"backups":"read"}}"#,
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
    let token = daemon.token(&[Permission::Backups]).await;
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
    let token = daemon.token(&[Permission::Backups]).await;
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
    let token = daemon.token(&[Permission::Backups]).await;
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
    let token = daemon.token(&[Permission::Backups]).await;
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
    let token = daemon
        .token(&[Permission::Backups, Permission::BackupRuns])
        .await;
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
    let token = daemon.token(&[Permission::BackupRuns]).await;
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
    let token = daemon.token(&[Permission::BackupRuns]).await;
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
    let token = daemon.token(&[Permission::BackupRuns]).await;
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
        post(
            TOKENS_PATH,
            &[],
            r#"{"label":"x","permissions":{"backups":"read"}}"#,
        ),
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
            r#"{"label":"nightly-mcp","permissions":{"backup-runs":"start-and-cancel","backups":"read"}}"#,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json(response).await;
    let secret = created["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("ladderback_"), "{secret}");
    assert_eq!(created["token"]["label"], "nightly-mcp");
    assert_eq!(
        created["token"]["permissions"],
        serde_json::json!({
            "backups": "read",
            "backup-runs": "start-and-cancel",
            "previews": "none",
            "configuration": "none",
            "snapshot-contents": "none",
            "daemon-log": "none",
            "repository-check": "none",
        })
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
async fn a_token_is_refused_without_a_label_of_1_to_64_characters_or_a_permission() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let long = serde_json::json!({ "label": "x".repeat(65), "permissions": {"backups": "read"} })
        .to_string();
    for (body, status, code, args) in [
        (
            r#"{"label":"","permissions":{"backups":"read"}}"#.to_owned(),
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
            r#"{"label":"x","permissions":{}}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "api-token-no-permission",
            serde_json::Value::Null,
        ),
        (
            r#"{"label":"x","permissions":{"backups":"none","daemon-log":"none"}}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "api-token-no-permission",
            serde_json::Value::Null,
        ),
        (
            r#"{"label":"x","permissions":{"admin":"read"}}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
            serde_json::Value::Null,
        ),
        // A level its permission doesn't have.
        (
            r#"{"label":"x","permissions":{"backups":"propose"}}"#.to_owned(),
            StatusCode::BAD_REQUEST,
            "invalid-body",
            serde_json::Value::Null,
        ),
        (
            r#"{"label":"x","permissions":{"backups":"read"},"expires":1}"#.to_owned(),
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
        daemon.token(&[Permission::Backups]).await;
    }
    let (cookie, csrf) = daemon.signed_in().await;
    let response = daemon
        .send(post(
            TOKENS_PATH,
            &[("cookie", &cookie), ("x-csrf-token", &csrf)],
            r#"{"label":"one-more","permissions":{"backups":"read"}}"#,
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
    let token = daemon.token(&[Permission::Backups]).await;
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
        post(
            TOKENS_PATH,
            &session,
            r#"{"label":"x","permissions":{"backups":"read"}}"#,
        ),
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
    let token = daemon.token(&[Permission::Backups]).await;
    let listener = crate::GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::new(NoUi), RequestChecks::default(), daemon.state());
    tokio::spawn(async move {
        let _ = crate::serve_connections(listener, app, std::future::pending()).await;
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

/// Exports any plan, and remembers what each export was asked to do with
/// the profile's commands.
#[derive(Default)]
struct Exports(Mutex<Vec<api::ProfileCommands>>);

#[async_trait]
impl crate::Profiles for Exports {
    async fn preview(
        &self,
        _request: api::ProfilePreviewDto,
    ) -> Result<api::ProfileImportDto, crate::ProfilesError> {
        Err(crate::ProfilesError::InvalidBody)
    }

    async fn export(
        &self,
        _plan: String,
        commands: api::ProfileCommands,
    ) -> Result<api::ProfileExportDto, crate::ProfilesError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(commands);
        Ok(api::ProfileExportDto {
            text: "[repository]\n".to_owned(),
            notices: Vec::new(),
        })
    }
}

impl Daemon {
    /// Asks for the profile at `uri` with `headers`, and answers what the
    /// export was asked to do with its commands.
    async fn export_asked(&self, uri: &str, headers: &[(&str, &str)]) -> api::ProfileCommands {
        let exports = Arc::new(Exports::default());
        let mut state = self.state();
        state.profiles = exports.clone();
        let response = router(Arc::new(NoUi), RequestChecks::default(), state)
            .oneshot(get(uri, headers))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let asked = exports.0.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(asked.len(), 1, "{uri}: {asked:?}");
        asked[0]
    }
}

/// A token is a program's, perhaps an AI agent's: whatever its permissions
/// and whatever it asks, its export leaves out the commands a person wrote,
/// which may carry a token of their own (#856).
#[tokio::test]
async fn a_token_s_export_leaves_the_commands_out_whatever_it_asks() {
    let daemon = Daemon::new();
    for permissions in [&[Permission::Backups][..], &Permission::ALL] {
        let token = daemon.token(permissions).await;
        for uri in [
            "/api/v1/plans/home/profile",
            "/api/v1/plans/home/profile?commands=written",
            "/api/v1/plans/home/profile?commands=left-out",
        ] {
            assert_eq!(
                daemon.export_asked(uri, &[("authorization", &token)]).await,
                api::ProfileCommands::LeftOut,
                "{permissions:?} {uri}"
            );
        }
    }
}

/// The signed-in person's export writes the commands, unless it asks for
/// them left out.
#[tokio::test]
async fn a_session_s_export_writes_the_commands_unless_it_asks_them_left_out() {
    let daemon = Daemon::new();
    let (cookie, _) = daemon.signed_in().await;
    for (uri, commands) in [
        ("/api/v1/plans/home/profile", api::ProfileCommands::Written),
        (
            "/api/v1/plans/home/profile?commands=written",
            api::ProfileCommands::Written,
        ),
        (
            "/api/v1/plans/home/profile?commands=left-out",
            api::ProfileCommands::LeftOut,
        ),
    ] {
        assert_eq!(
            daemon.export_asked(uri, &[("cookie", &cookie)]).await,
            commands,
            "{uri}"
        );
    }
}

fn put(path: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut all = vec![("content-type", "application/json")];
    all.extend_from_slice(headers);
    build(Method::PUT, path, &all, body)
}

/// The path that changes the only token's permissions.
async fn permissions_path(daemon: &Daemon) -> String {
    let id = daemon.access.api_tokens().await.unwrap()[0].id.clone();
    format!("/api/v1/tokens/{}/permissions", id.as_str())
}

#[tokio::test]
async fn the_admin_changes_a_tokens_permissions_with_the_current_password() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::Backups]).await;
    let path = permissions_path(&daemon).await;
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let body = |password: &str| {
        serde_json::json!({
            "permissions": { "backup-runs": "start-and-cancel", "daemon-log": "read" },
            "current-password": password,
        })
        .to_string()
    };
    let (status, code, _) = refusal(
        daemon
            .send(put(&path, &session, &body("not-the-password")))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("current-password-wrong"))
    );
    // The wrong password made the address wait, as a failed sign-in does;
    // the token is still as it was once the wait is over.
    daemon.clock.advance(domain::SignedDuration::from_secs(1));
    let auth = [("authorization", token.as_str())];
    assert_eq!(
        daemon.send(get("/api/v1/plans", &auth)).await.status(),
        StatusCode::OK
    );
    let response = daemon.send(put(&path, &session, &body(PASSWORD))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["daemon-log"], "read");
    assert_needs(&daemon, get("/api/v1/plans", &auth), "backups").await;
    assert_eq!(
        daemon.send(get("/api/v1/log", &auth)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn no_token_changes_permissions_even_with_the_password_and_every_permission() {
    let daemon = Daemon::new();
    let token = daemon.token(&Permission::ALL).await;
    let path = permissions_path(&daemon).await;
    let body = serde_json::json!({
        "permissions": { "backups": "read" },
        "current-password": PASSWORD,
    })
    .to_string();
    assert_needs(
        &daemon,
        put(&path, &[("authorization", &token)], &body),
        "session",
    )
    .await;
    let kept = daemon.access.api_tokens().await.unwrap()[0].permissions;
    assert!(kept.get().grants(Permission::RepositoryCheck));
}

#[tokio::test]
async fn a_change_to_no_permission_or_to_an_unknown_token_is_refused() {
    let daemon = Daemon::new();
    daemon.token(&[Permission::Backups]).await;
    let path = permissions_path(&daemon).await;
    let (cookie, csrf) = daemon.signed_in().await;
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let none = serde_json::json!({ "permissions": {}, "current-password": PASSWORD }).to_string();
    let (status, code, _) = refusal(daemon.send(put(&path, &session, &none)).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::BAD_REQUEST, Some("api-token-no-permission"))
    );
    let some = serde_json::json!({
        "permissions": { "backups": "read" },
        "current-password": PASSWORD,
    })
    .to_string();
    let (status, code, _) = refusal(
        daemon
            .send(put(
                "/api/v1/tokens/abcdefghijkl/permissions",
                &session,
                &some,
            ))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::NOT_FOUND, Some("api-token-unknown"))
    );
}

#[tokio::test]
async fn a_token_reads_its_own_permissions_whatever_they_are() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::DaemonLog]).await;
    let response = daemon
        .send(get("/api/v1/permissions", &[("authorization", &token)]))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let permissions = json(response).await;
    assert_eq!(permissions["daemon-log"], "read", "{permissions}");
    assert_eq!(permissions["backups"], "none", "{permissions}");
}

/// The router with sign-in off over the default configuration.
async fn signed_off(request: Request) -> Response {
    router(Arc::new(NoUi), RequestChecks::default(), test_state())
        .oneshot(request)
        .await
        .unwrap()
}

#[tokio::test]
async fn with_sign_in_off_an_ai_agents_request_is_held_to_what_agents_may_do_without_a_token() {
    let agent = [(api::AI_AGENT_HEADER, "1")];
    let (status, code, args) = refusal(signed_off(get("/api/v1/log", &agent)).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("without-token-permission"))
    );
    assert_eq!(
        args,
        serde_json::json!({ "permission": { "type": "text", "value": "daemon-log" } })
    );
    assert_eq!(
        signed_off(get("/api/v1/plans", &agent)).await.status(),
        StatusCode::OK
    );
    let permissions = json(signed_off(get("/api/v1/permissions", &agent)).await).await;
    assert_eq!(permissions["backups"], "read", "{permissions}");
    assert_eq!(permissions["daemon-log"], "none", "{permissions}");
    // Without the mark, the request is a person's, as before.
    assert_ne!(
        signed_off(get("/api/v1/log", &[])).await.status(),
        StatusCode::FORBIDDEN
    );
    let permissions = json(signed_off(get("/api/v1/permissions", &[])).await).await;
    assert_eq!(permissions["daemon-log"], "read", "{permissions}");
}

/// With sign-in off, an AI agent's search is held to the `[without-token]`
/// table, which by default lists the snapshots but names nothing inside
/// them: Backups doesn't open the search (decision 132).
#[tokio::test]
async fn with_sign_in_off_an_ai_agent_searches_only_with_snapshot_contents() {
    let agent = [(api::AI_AGENT_HEADER, "1")];
    let (status, code, args) = refusal(signed_off(get(SEARCH, &agent)).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("without-token-permission"))
    );
    assert_eq!(
        args,
        serde_json::json!({ "permission": { "type": "text", "value": "snapshot-contents" } })
    );
    assert_eq!(
        signed_off(get("/api/v1/repositories/home/snapshots", &agent))
            .await
            .status(),
        StatusCode::OK
    );
}

/// A warning that names a file, twice: as its item and in the engine's own
/// words.
fn unreadable() -> api::OperationWarningDto {
    api::OperationWarningDto {
        code: api::WarningCode::SourceUnreadable,
        item: Some("/home/demo/tax-2026.pdf".to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied: /home/demo/tax-2026.pdf".to_owned(),
    }
}

/// The one warning of an operation's detail, as `request` is answered it.
async fn warning_shown_to(daemon: &Daemon, request: Request) -> serde_json::Value {
    let response = daemon.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await["warnings"][0].clone()
}

#[tokio::test]
async fn an_operations_warning_names_its_file_only_to_a_token_with_snapshot_contents() {
    let daemon = Daemon::new();
    let path = "/api/v1/operations/018f0000-0000-7000-8000-000000000001";
    let without = daemon.token(&[Permission::Backups]).await;
    let hidden = warning_shown_to(&daemon, get(path, &[("authorization", &without)])).await;
    assert_eq!(
        hidden,
        serde_json::json!({
            "code": "source-unreadable",
            "item": null,
            "during": "backup",
            "detail": "",
        })
    );
    let with = daemon
        .token(&[Permission::Backups, Permission::SnapshotContents])
        .await;
    let shown = warning_shown_to(&daemon, get(path, &[("authorization", &with)])).await;
    assert_eq!(shown["item"], "/home/demo/tax-2026.pdf");
    assert_eq!(
        shown["detail"],
        "permission denied: /home/demo/tax-2026.pdf"
    );
    let (cookie, _) = daemon.signed_in().await;
    let person = warning_shown_to(&daemon, get(path, &[("cookie", &cookie)])).await;
    assert_eq!(person["item"], "/home/demo/tax-2026.pdf");
}

#[tokio::test]
async fn with_sign_in_off_an_ai_agent_sees_a_warnings_file_only_once_the_table_allows_it() {
    let path = "/api/v1/operations/018f0000-0000-7000-8000-000000000001";
    let agent = [(api::AI_AGENT_HEADER, "1")];
    let config = Arc::new(app::fakes::InMemoryConfigStore::new());
    let serve = |request: Request| {
        let mut state = test_state();
        state.operations = Arc::new(Recorder::default());
        state.without_token = crate::request_checks::tests::without_token_over(config.clone());
        router(Arc::new(NoUi), RequestChecks::default(), state).oneshot(request)
    };
    let warning = |response: Response| async move {
        assert_eq!(response.status(), StatusCode::OK);
        json(response).await["warnings"][0].clone()
    };
    let hidden = warning(serve(get(path, &agent)).await.unwrap()).await;
    assert_eq!(hidden["item"], serde_json::Value::Null, "{hidden}");
    assert_eq!(hidden["detail"], "", "{hidden}");
    let person = warning(serve(get(path, &[])).await.unwrap()).await;
    assert_eq!(person["item"], "/home/demo/tax-2026.pdf", "{person}");
    let opened = domain::planning::AgentConfig::default().with_without_token(
        Permissions::default()
            .with(
                Permission::SnapshotContents,
                Some(domain::access::permissions::Level::Read),
            )
            .unwrap(),
    );
    app::ports::ConfigStore::save(config.as_ref(), &opened)
        .await
        .unwrap();
    let shown = warning(serve(get(path, &agent)).await.unwrap()).await;
    assert_eq!(shown["item"], "/home/demo/tax-2026.pdf", "{shown}");
}

#[tokio::test]
async fn with_sign_in_off_an_ai_agent_is_refused_a_route_only_a_browser_may_use() {
    let agent = [(api::AI_AGENT_HEADER, "1")];
    for request in [
        get(TOKENS_PATH, &agent),
        post("/api/v1/proposals/018f/approve", &agent, "{}"),
        get("/api/v1/settings/notifications", &agent),
    ] {
        let what = format!("{} {}", request.method(), request.uri());
        let (status, code, args) = refusal(signed_off(request).await).await;
        assert_eq!(
            (status, code.as_deref()),
            (StatusCode::FORBIDDEN, Some("without-token-permission")),
            "{what}"
        );
        assert_eq!(
            args,
            serde_json::json!({ "permission": { "type": "text", "value": "session" } }),
            "{what}"
        );
    }
    // Without the mark, the request is a person's, as before.
    let (status, code, _) = refusal(signed_off(get(TOKENS_PATH, &[])).await).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::CONFLICT, Some("sign-in-off"))
    );
}

#[tokio::test]
async fn with_sign_in_on_a_session_reads_every_permission_whatever_mark_it_carries() {
    let daemon = Daemon::new();
    let (cookie, _) = daemon.signed_in().await;
    let response = daemon
        .send(get(
            "/api/v1/permissions",
            &[("cookie", &cookie), (api::AI_AGENT_HEADER, "1")],
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let permissions = json(response).await;
    assert_eq!(
        permissions,
        serde_json::json!({
            "backups": "read",
            "backup-runs": "start-and-cancel",
            "previews": "preview",
            "configuration": "propose",
            "snapshot-contents": "read",
            "daemon-log": "read",
            "repository-check": "run",
        })
    );
}

#[tokio::test]
async fn a_change_without_the_csrf_token_or_with_a_wrong_password_leaves_the_permissions() {
    let daemon = Daemon::new();
    let token = daemon.token(&[Permission::Backups]).await;
    let path = permissions_path(&daemon).await;
    let (cookie, csrf) = daemon.signed_in().await;
    let body = |password: &str| {
        serde_json::json!({
            "permissions": { "backups": "read", "daemon-log": "read" },
            "current-password": password,
        })
        .to_string()
    };
    let (status, code, _) = refusal(
        daemon
            .send(put(&path, &[("cookie", cookie.as_str())], &body(PASSWORD)))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("csrf-token-invalid"))
    );
    let session = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let (status, code, _) = refusal(
        daemon
            .send(put(&path, &session, &body("not-the-password")))
            .await,
    )
    .await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::FORBIDDEN, Some("current-password-wrong"))
    );
    let kept = daemon.access.api_tokens().await.unwrap()[0].permissions;
    assert_eq!(kept.get(), granting(&[Permission::Backups]));
    daemon.clock.advance(domain::SignedDuration::from_secs(1));
    assert_needs(
        &daemon,
        get("/api/v1/log", &[("authorization", &token)]),
        "daemon-log",
    )
    .await;
}

/// Serves the router over `state`, with the connection's address; returns
/// where.
async fn serving_events(state: AppState) -> SocketAddr {
    let listener = crate::GatedListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(Arc::new(NoUi), RequestChecks::default(), state);
    tokio::spawn(async move {
        let _ = crate::serve_connections(listener, app, std::future::pending()).await;
    });
    addr
}

type EventSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// An events stream opened at `addr` with the header `(name, value)`.
async fn events_with(addr: SocketAddr, (name, value): (&'static str, &str)) -> EventSocket {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://{addr}/api/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(name, HeaderValue::from_str(value).unwrap());
    let socket = tokio_tungstenite::connect_async(request).await.unwrap().0;
    answers_a_ping(socket).await
}

/// Pings `socket` and waits for its pong: the stream is open and
/// subscribed.
async fn answers_a_ping(mut socket: EventSocket) -> EventSocket {
    use tokio_tungstenite::tungstenite::Message;
    futures::SinkExt::send(&mut socket, Message::Ping(vec![7].into()))
        .await
        .unwrap();
    match socket.next().await {
        Some(Ok(Message::Pong(payload))) => assert_eq!(payload.as_ref(), [7]),
        other => panic!("an open stream answers a ping: {other:?}"),
    }
    socket
}

/// Whether `socket` ends within a recheck and a little more.
async fn ends(socket: &mut EventSocket) -> bool {
    tokio::time::timeout(
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
    .await
    .is_ok()
}

#[tokio::test]
async fn an_events_stream_ends_once_its_token_no_longer_has_backups() {
    let daemon = Daemon::new();
    let token = daemon
        .token(&[Permission::Backups, Permission::BackupRuns])
        .await;
    let addr = serving_events(daemon.state()).await;
    let mut socket = events_with(addr, ("authorization", &token)).await;
    let id = daemon.access.api_tokens().await.unwrap()[0].id.clone();
    let current = domain::access::Password::parse(PASSWORD.to_owned()).unwrap();
    assert_eq!(
        daemon
            .access
            .change_api_token_permissions(
                HOME.ip(),
                &current,
                &id,
                TokenPermissions::new(granting(&[Permission::BackupRuns])).unwrap(),
            )
            .await,
        Ok(true)
    );
    assert!(ends(&mut socket).await, "the stream outlived its Backups");
}

#[tokio::test]
async fn with_sign_in_off_an_ai_agents_stream_ends_once_the_table_takes_backups_away() {
    let config = Arc::new(app::fakes::InMemoryConfigStore::new());
    let mut state = test_state();
    state.without_token = crate::request_checks::tests::without_token_over(config.clone());
    let addr = serving_events(state).await;
    let mut socket = events_with(addr, (api::AI_AGENT_HEADER, "1")).await;
    let closed = domain::planning::AgentConfig::default().with_without_token(
        Permissions::NONE
            .with(
                Permission::DaemonLog,
                Some(domain::access::permissions::Level::Read),
            )
            .unwrap(),
    );
    app::ports::ConfigStore::save(config.as_ref(), &closed)
        .await
        .unwrap();
    assert!(ends(&mut socket).await, "the stream outlived its Backups");
}

#[tokio::test]
async fn an_events_stream_shows_a_warnings_file_only_to_a_token_with_snapshot_contents() {
    use tokio_tungstenite::tungstenite::Message;

    let daemon = Daemon::new();
    let without = daemon.token(&[Permission::Backups]).await;
    let with = daemon
        .token(&[Permission::Backups, Permission::SnapshotContents])
        .await;
    let events = Arc::new(app::fakes::InMemoryEventBus::new());
    let mut state = daemon.state();
    state.events = events.clone();
    let addr = serving_events(state).await;
    let mut hidden = events_with(addr, ("authorization", &without)).await;
    let mut shown = events_with(addr, ("authorization", &with)).await;
    app::ports::EventBus::publish(
        events.as_ref(),
        api::UiEvent::OperationWarning {
            operation: "018f0000-0000-7000-8000-000000000001".to_owned(),
            code: api::WarningCode::SourceUnreadable,
            args: vec![api::MsgArg::Text("/home/demo/tax-2026.pdf".to_owned())],
        },
    )
    .await;
    for (socket, args) in [
        (&mut hidden, serde_json::json!([])),
        (
            &mut shown,
            serde_json::json!([{ "type": "text", "value": "/home/demo/tax-2026.pdf" }]),
        ),
    ] {
        let seen = match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                serde_json::from_str::<serde_json::Value>(text.as_str()).unwrap()
            }
            other => panic!("the warning arrives as text: {other:?}"),
        };
        assert_eq!(seen["code"], "source-unreadable", "{seen}");
        assert_eq!(seen["args"], args, "{seen}");
    }
}

/// A restore that failed on a file: its failure names the file as an
/// argument and in the engine's own words, and its entry is that file.
fn failed_on_a_file() -> api::OperationSummaryDto {
    api::OperationSummaryDto {
        kind: "restore".to_owned(),
        status: "failed".to_owned(),
        failure_code: Some(api::ErrorCode::EntryUnknown),
        failure: Some(api::ErrorDto::new(
            api::ErrorCode::EntryUnknown,
            [(
                "path",
                api::MsgArg::Text("/home/demo/tax-2026.pdf".to_owned()),
            )],
            Some("no entry /home/demo/tax-2026.pdf".to_owned()),
        )),
        result: None,
        entry: Some("/home/demo/tax-2026.pdf".to_owned()),
        restored_to: Some(api::RestoredToDto::Folder {
            path: "/home/demo/ladderback-restore/laptop-3f2a1b4c".to_owned(),
        }),
        ..crate::tests::canned_summary()
    }
}

/// The one operation `path` answers with (its detail's or a page's first),
/// as `request` is answered it.
async fn operation_shown_to(daemon: &Daemon, request: Request) -> serde_json::Value {
    let response = daemon.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    if body["operation"].is_object() {
        body["operation"].clone()
    } else {
        body["operations"][0].clone()
    }
}

#[tokio::test]
async fn an_operations_failure_is_only_its_code_to_a_token_without_snapshot_contents() {
    let daemon = Daemon::new();
    let without = daemon.token(&[Permission::Backups]).await;
    let with = daemon
        .token(&[Permission::Backups, Permission::SnapshotContents])
        .await;
    let (cookie, _) = daemon.signed_in().await;
    for path in [
        "/api/v1/operations/018f0000-0000-7000-8000-000000000001",
        "/api/v1/operations",
    ] {
        let hidden = operation_shown_to(&daemon, get(path, &[("authorization", &without)])).await;
        assert_eq!(
            hidden["failure"],
            serde_json::json!({ "code": "entry-unknown" }),
            "{path}: {hidden}"
        );
        assert_eq!(hidden["failure-code"], "entry-unknown", "{path}: {hidden}");
        assert_eq!(hidden.get("entry"), None, "{path}: {hidden}");
        assert_eq!(hidden.get("restored-to"), None, "{path}: {hidden}");
        for shown in [
            operation_shown_to(&daemon, get(path, &[("authorization", &with)])).await,
            operation_shown_to(&daemon, get(path, &[("cookie", &cookie)])).await,
        ] {
            assert_eq!(
                shown["failure"]["args"]["path"]["value"], "/home/demo/tax-2026.pdf",
                "{path}: {shown}"
            );
            assert_eq!(
                shown["failure"]["detail"], "no entry /home/demo/tax-2026.pdf",
                "{path}: {shown}"
            );
            assert_eq!(shown["entry"], "/home/demo/tax-2026.pdf", "{path}: {shown}");
            assert_eq!(
                shown["restored-to"]["path"], "/home/demo/ladderback-restore/laptop-3f2a1b4c",
                "{path}: {shown}"
            );
        }
    }
}

#[tokio::test]
async fn with_sign_in_off_an_ai_agent_reads_a_failures_words_only_once_the_table_allows_it() {
    let path = "/api/v1/operations/018f0000-0000-7000-8000-000000000001";
    let agent = [(api::AI_AGENT_HEADER, "1")];
    let config = Arc::new(app::fakes::InMemoryConfigStore::new());
    let serve = |request: Request| {
        let mut state = test_state();
        state.operations = Arc::new(Recorder::default());
        state.without_token = crate::request_checks::tests::without_token_over(config.clone());
        router(Arc::new(NoUi), RequestChecks::default(), state).oneshot(request)
    };
    let failure = |response: Response| async move {
        assert_eq!(response.status(), StatusCode::OK);
        json(response).await["operation"]["failure"].clone()
    };
    let hidden = failure(serve(get(path, &agent)).await.unwrap()).await;
    assert_eq!(hidden, serde_json::json!({ "code": "entry-unknown" }));
    let person = failure(serve(get(path, &[])).await.unwrap()).await;
    assert_eq!(
        person["detail"], "no entry /home/demo/tax-2026.pdf",
        "{person}"
    );
    let opened = domain::planning::AgentConfig::default().with_without_token(
        Permissions::default()
            .with(
                Permission::SnapshotContents,
                Some(domain::access::permissions::Level::Read),
            )
            .unwrap(),
    );
    app::ports::ConfigStore::save(config.as_ref(), &opened)
        .await
        .unwrap();
    let shown = failure(serve(get(path, &agent)).await.unwrap()).await;
    assert_eq!(shown, person);
}

#[test]
fn the_status_shows_each_operation_in_it_as_the_operation_itself_is_shown() {
    let status = api::StatusDto {
        plans: vec![api::PlanStatusDto {
            plan: api::PlanSummaryDto {
                id: "home".to_owned(),
                repository: "demo".to_owned(),
                scheduled: false,
                next_run: None,
                warnings: Vec::new(),
            },
            last: Some(failed_on_a_file()),
            last_success: None,
            overdue: false,
        }],
        running: vec![failed_on_a_file()],
        unreadable_repositories: vec![api::UnreadableRepositoryDto {
            repository: "demo".to_owned(),
            operation: failed_on_a_file(),
        }],
        full_disk_access: api::FullDiskAccessStateDto::NotApplicable,
    };
    let code_only = summary_shown(granting(&[Permission::Backups]), failed_on_a_file());
    assert_eq!(
        code_only.failure,
        Some(api::ErrorDto::new(api::ErrorCode::EntryUnknown, [], None))
    );
    assert_eq!(code_only.entry, None);
    assert_eq!(code_only.restored_to, None);
    let hidden = status_shown(granting(&[Permission::Backups]), status.clone());
    assert_eq!(hidden.plans[0].last.as_ref(), Some(&code_only));
    assert_eq!(hidden.running, std::slice::from_ref(&code_only));
    assert_eq!(hidden.unreadable_repositories[0].operation, code_only);
    let with = granting(&[Permission::Backups, Permission::SnapshotContents]);
    assert_eq!(status_shown(with, status.clone()), status);
}

#[tokio::test]
async fn an_events_stream_tells_a_failures_words_only_to_a_token_with_snapshot_contents() {
    use tokio_tungstenite::tungstenite::Message;

    let daemon = Daemon::new();
    let without = daemon.token(&[Permission::Backups]).await;
    let with = daemon
        .token(&[Permission::Backups, Permission::SnapshotContents])
        .await;
    let events = Arc::new(app::fakes::InMemoryEventBus::new());
    let mut state = daemon.state();
    state.events = events.clone();
    let addr = serving_events(state).await;
    let mut hidden = events_with(addr, ("authorization", &without)).await;
    let mut shown = events_with(addr, ("authorization", &with)).await;
    let words = [
        api::MsgArg::Text("/home/demo/tax-2026.pdf".to_owned()),
        api::MsgArg::Text("no entry /home/demo/tax-2026.pdf".to_owned()),
    ];
    app::ports::EventBus::publish(
        events.as_ref(),
        api::UiEvent::OperationFailed {
            operation: "018f0000-0000-7000-8000-000000000001".to_owned(),
            code: api::ErrorCode::EntryUnknown,
            args: words.to_vec(),
            finished_at: "2026-11-09T10:00:02Z".to_owned(),
        },
    )
    .await;
    for (socket, args) in [
        (&mut hidden, serde_json::json!([])),
        (&mut shown, serde_json::to_value(words).unwrap()),
    ] {
        let seen = match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                serde_json::from_str::<serde_json::Value>(text.as_str()).unwrap()
            }
            other => panic!("the failure arrives as text: {other:?}"),
        };
        assert_eq!(seen["code"], "entry-unknown", "{seen}");
        assert_eq!(seen["finished_at"], "2026-11-09T10:00:02Z", "{seen}");
        assert_eq!(seen["args"], args, "{seen}");
    }
}

/// A catalog whose status has a restore running that failed on a file.
struct FailedOnAFile;

#[async_trait]
impl crate::Catalog for FailedOnAFile {
    async fn status(
        &self,
        full_disk_access: api::FullDiskAccessStateDto,
    ) -> Result<api::StatusDto, crate::CatalogError> {
        Ok(api::StatusDto {
            plans: Vec::new(),
            running: vec![failed_on_a_file()],
            unreadable_repositories: Vec::new(),
            full_disk_access,
        })
    }

    async fn plans(&self) -> Result<api::PlansDto, crate::CatalogError> {
        Ok(api::PlansDto::default())
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, crate::CatalogError> {
        Ok(api::RepositoriesDto::default())
    }

    async fn snapshots(&self, _alias: String) -> Result<api::SnapshotsDto, crate::SnapshotsError> {
        Ok(api::SnapshotsDto::default())
    }

    async fn folder(
        &self,
        _alias: String,
        _snapshot: domain::SnapshotId,
        _folder: domain::browse::FolderPath,
        _from: u64,
        _most: u64,
    ) -> Result<api::FolderDto, crate::FolderError> {
        Ok(api::FolderDto::default())
    }

    async fn find(
        &self,
        _alias: String,
        _pattern: domain::find::NamePattern,
        _plan: Option<domain::PlanId>,
        _caps: domain::find::SearchCaps,
    ) -> Result<api::FindDto, crate::SnapshotsError> {
        Ok(api::FindDto::default())
    }
}

#[tokio::test]
async fn the_status_tells_a_failures_words_only_to_a_token_with_snapshot_contents() {
    let daemon = Daemon::new();
    let without = daemon.token(&[Permission::Backups]).await;
    let with = daemon
        .token(&[Permission::Backups, Permission::SnapshotContents])
        .await;
    let mut state = daemon.state();
    state.catalog = Arc::new(FailedOnAFile);
    let app = router(Arc::new(NoUi), RequestChecks::default(), state);
    for (token, failure, entry) in [
        (
            &without,
            serde_json::json!({ "code": "entry-unknown" }),
            None,
        ),
        (
            &with,
            serde_json::to_value(failed_on_a_file().failure).unwrap(),
            Some(serde_json::json!("/home/demo/tax-2026.pdf")),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(get("/api/v1/status", &[("authorization", token)]))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let running = json(response).await["running"][0].clone();
        assert_eq!(running["failure"], failure, "{running}");
        assert_eq!(running.get("entry"), entry.as_ref(), "{running}");
    }
}

#[tokio::test]
async fn an_open_events_stream_stops_telling_a_failures_words_once_its_token_loses_snapshot_contents()
 {
    use tokio_tungstenite::tungstenite::Message;

    let daemon = Daemon::new();
    let token = daemon
        .token(&[Permission::Backups, Permission::SnapshotContents])
        .await;
    let events = Arc::new(app::fakes::InMemoryEventBus::new());
    let mut state = daemon.state();
    state.events = events.clone();
    let addr = serving_events(state).await;
    let mut socket = events_with(addr, ("authorization", &token)).await;
    let id = daemon.access.api_tokens().await.unwrap()[0].id.clone();
    let current = domain::access::Password::parse(PASSWORD.to_owned()).unwrap();
    assert_eq!(
        daemon
            .access
            .change_api_token_permissions(
                HOME.ip(),
                &current,
                &id,
                TokenPermissions::new(granting(&[Permission::Backups])).unwrap(),
            )
            .await,
        Ok(true)
    );
    // The stream reads its token's permissions again within a recheck.
    tokio::time::sleep(crate::SESSION_RECHECK + std::time::Duration::from_secs(2)).await;
    app::ports::EventBus::publish(
        events.as_ref(),
        api::UiEvent::OperationFailed {
            operation: "018f0000-0000-7000-8000-000000000001".to_owned(),
            code: api::ErrorCode::EntryUnknown,
            args: vec![api::MsgArg::Text("/home/demo/tax-2026.pdf".to_owned())],
            finished_at: "2026-11-09T10:00:02Z".to_owned(),
        },
    )
    .await;
    let seen = match socket.next().await {
        Some(Ok(Message::Text(text))) => {
            serde_json::from_str::<serde_json::Value>(text.as_str()).unwrap()
        }
        other => panic!("the failure arrives as text: {other:?}"),
    };
    assert_eq!(seen["code"], "entry-unknown", "{seen}");
    assert_eq!(seen["args"], serde_json::json!([]), "{seen}");
}

/// A configuration that can't be read, as a file the daemon may not open.
struct Unreadable;

fn denied() -> app::ports::ConfigError {
    app::ports::ConfigError::Unavailable {
        file: "config.toml".to_owned(),
        detail: "permission denied".to_owned(),
    }
}

#[async_trait]
impl app::ports::ConfigStore for Unreadable {
    async fn load(&self) -> Result<domain::planning::AgentConfig, app::ports::ConfigError> {
        Err(denied())
    }

    async fn save(
        &self,
        _config: &domain::planning::AgentConfig,
    ) -> Result<(), app::ports::ConfigError> {
        Err(denied())
    }

    async fn record_id(
        &self,
        _alias: &domain::RepositoryAlias,
        _read: &domain::planning::RepositoryConfig,
        _id: &domain::RepositoryId,
    ) -> Result<(), app::ports::RecordError> {
        Err(app::ports::RecordError::Store(denied()))
    }
}

#[tokio::test]
async fn with_sign_in_off_an_ai_agent_is_told_which_file_couldnt_be_read() {
    let mut state = test_state();
    state.without_token = crate::request_checks::tests::without_token_over(Arc::new(Unreadable));
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(get("/api/v1/plans", &[(api::AI_AGENT_HEADER, "1")]))
        .await
        .unwrap();
    let (status, code, args) = refusal(response).await;
    assert_eq!(
        (status, code.as_deref()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("config-unavailable"))
    );
    assert_eq!(
        args,
        serde_json::json!({ "file": { "type": "text", "value": "config.toml" } })
    );
}
