use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use app::access::Access;
use app::fakes::{
    FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, InMemorySessionStore, SeededRandom,
};
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::header::{HOST, RETRY_AFTER, SET_COOKIE};
use axum::http::{Method, StatusCode};
use domain::Timestamp;
use domain::access::api_token::{ApiTokenLabel, Scope, Scopes};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::api_tokens::required_scope;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{CSRF_HEADER, RequestChecks, SignIn, router};

const PASSWORD: &str = "demo-admin-pass";
const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const DRAFT: &str = r#"{"plan":{"id":"home","repository":"demo","sources":["/srv/demo"]}}"#;

/// Answers each dry run with one count, or with the refusal it was given,
/// and records who asked and each allowlist saved.
#[derive(Default)]
struct Scripted {
    askers: Mutex<Vec<Proposer>>,
    saved: Mutex<Vec<Vec<String>>>,
    refusal: Mutex<Option<DryRunsError>>,
}

impl Scripted {
    fn askers(&self) -> Vec<Proposer> {
        self.askers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn saved(&self) -> Vec<Vec<String>> {
        self.saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn answer<T>(&self, answer: T) -> Result<T, DryRunsError> {
        match self
            .refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            Some(refusal) => Err(refusal),
            None => Ok(answer),
        }
    }
}

#[async_trait]
impl DryRuns for Scripted {
    async fn dry_run(
        &self,
        asker: Proposer,
        _plan: api::PlanDto,
    ) -> Result<api::PlanDryRunDto, DryRunsError> {
        self.askers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(asker);
        self.answer(api::PlanDryRunDto {
            files: 3,
            bytes: 300,
            unreadable: 0,
            excludes: vec![api::ExcludeTallyDto {
                setting: "globs".to_owned(),
                entry: Some(1),
                files: 1,
                bytes: 100,
            }],
            stopped: None,
        })
    }

    async fn allowlist(&self) -> Result<api::DryRunAllowlistDto, DryRunsError> {
        self.answer(api::DryRunAllowlistDto {
            folders: vec!["/srv/demo".to_owned()],
            home: Some("/home/demo".to_owned()),
        })
    }

    async fn set_allowlist(
        &self,
        folders: Vec<String>,
    ) -> Result<api::DryRunAllowlistDto, DryRunsError> {
        self.saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(folders.clone());
        self.answer(api::DryRunAllowlistDto {
            folders,
            home: None,
        })
    }
}

/// A daemon with sign-in on and the scripted dry runs.
struct Daemon {
    access: Arc<Access>,
    dry_runs: Arc<Scripted>,
}

impl Daemon {
    fn new() -> Self {
        Self {
            access: Arc::new(Access::new(
                Arc::new(InMemoryAdminPassword::with(PASSWORD)),
                Arc::new(SeededRandom::new(7)),
                Arc::new(FakeClock::new(
                    Timestamp::from_second(1_800_000_000).unwrap(),
                )),
                Arc::new(InMemoryApiTokenStore::new()),
                Arc::new(InMemorySessionStore::new()),
            )),
            dry_runs: Arc::default(),
        }
    }

    async fn send(&self, request: Request) -> Response {
        let mut state = test_state();
        state.sign_in = SignIn::On(Arc::clone(&self.access));
        state.dry_runs = self.dry_runs.clone();
        router(Arc::new(NoUi), RequestChecks::default(), state)
            .oneshot(request)
            .await
            .unwrap()
    }

    /// A token with `scopes`, as a program sends it, and who it is.
    async fn token(&self, scopes: &[Scope]) -> (String, Proposer) {
        let created = self
            .access
            .create_api_token(
                ApiTokenLabel::new("assistant".to_owned()).unwrap(),
                Scopes::new(scopes.iter().copied()).unwrap(),
            )
            .await
            .unwrap();
        (
            format!("Bearer {}", created.text.expose()),
            Proposer::Token {
                id: created.token.id.clone(),
                label: created.token.label.clone(),
            },
        )
    }

    /// A session's `Cookie` header and its CSRF token.
    async fn signed_in(&self) -> (String, String) {
        let response = self
            .send(build(
                Method::POST,
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

fn build(method: Method, path: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, "127.0.0.1:8080")
        .header("content-type", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let mut request = builder.body(Body::from(body.to_owned())).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(crate::PeerAddr(HOME)));
    request
}

async fn json(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

#[test]
fn the_dry_run_needs_the_propose_scope_and_the_allowlist_a_session() {
    for (method, path, scope) in [
        (Method::POST, "/api/v1/dry-runs", Some(Scope::Propose)),
        (Method::GET, "/api/v1/dry-runs", None),
        (Method::POST, "/api/v1/plans/dry-run", None),
        (Method::GET, "/api/v1/settings/dry-run-allowlist", None),
        (Method::PUT, "/api/v1/settings/dry-run-allowlist", None),
        (Method::POST, "/api/v1/settings/dry-run-allowlist", None),
    ] {
        assert_eq!(required_scope(&method, path), scope, "{method} {path}");
    }
}

#[tokio::test]
async fn a_token_with_the_propose_scope_dry_runs_as_itself_and_is_told_counts() {
    let daemon = Daemon::new();
    let (bearer, itself) = daemon.token(&[Scope::Propose]).await;
    let response = daemon
        .send(build(
            Method::POST,
            "/api/v1/dry-runs",
            &[("authorization", bearer.as_str())],
            DRAFT,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json(response).await,
        serde_json::json!({
            "files": 3,
            "bytes": 300,
            "unreadable": 0,
            "excludes": [{ "setting": "globs", "entry": 1, "files": 1, "bytes": 100 }],
        })
    );
    assert_eq!(daemon.dry_runs.askers(), [itself]);
}

#[tokio::test]
async fn a_token_never_reads_or_sets_the_allowlist_nor_dry_runs_without_the_scope() {
    let daemon = Daemon::new();
    let (proposer, _) = daemon.token(&[Scope::Propose]).await;
    let (reader, _) = daemon.token(&[Scope::Read, Scope::Run]).await;
    for (request, scope) in [
        (
            build(
                Method::PUT,
                "/api/v1/settings/dry-run-allowlist",
                &[("authorization", proposer.as_str())],
                r#"{"folders":["/"]}"#,
            ),
            "session",
        ),
        (
            build(
                Method::GET,
                "/api/v1/settings/dry-run-allowlist",
                &[("authorization", proposer.as_str())],
                "",
            ),
            "session",
        ),
        (
            build(
                Method::POST,
                "/api/v1/dry-runs",
                &[("authorization", reader.as_str())],
                DRAFT,
            ),
            "propose",
        ),
    ] {
        let what = format!("{} {}", request.method(), request.uri());
        let response = daemon.send(request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{what}");
        assert_eq!(
            json(response).await["args"]["scope"]["value"],
            scope,
            "{what}"
        );
    }
    assert!(daemon.dry_runs.saved().is_empty());
    assert!(daemon.dry_runs.askers().is_empty());
}

#[tokio::test]
async fn a_session_reads_and_sets_the_allowlist_and_dry_runs_with_no_token() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let csrf_header = CSRF_HEADER.to_string();
    let headers = [
        ("cookie", cookie.as_str()),
        (csrf_header.as_str(), csrf.as_str()),
    ];
    let read = daemon
        .send(build(
            Method::GET,
            "/api/v1/settings/dry-run-allowlist",
            &headers,
            "",
        ))
        .await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(
        json(read).await,
        serde_json::json!({ "folders": ["/srv/demo"], "home": "/home/demo" })
    );
    let set = daemon
        .send(build(
            Method::PUT,
            "/api/v1/settings/dry-run-allowlist",
            &headers,
            r#"{"folders":["/srv/demo","/home/demo"]}"#,
        ))
        .await;
    assert_eq!(set.status(), StatusCode::OK);
    assert_eq!(
        daemon.dry_runs.saved(),
        [vec!["/srv/demo".to_owned(), "/home/demo".to_owned()]]
    );
    let ran = daemon
        .send(build(Method::POST, "/api/v1/dry-runs", &headers, DRAFT))
        .await;
    assert_eq!(ran.status(), StatusCode::OK);
    assert_eq!(daemon.dry_runs.askers(), [Proposer::NoToken]);
}

#[tokio::test]
async fn a_body_that_is_not_the_request_is_refused_unread() {
    let daemon = Daemon::new();
    let (bearer, _) = daemon.token(&[Scope::Propose]).await;
    for body in [
        "not json",
        r#"{"plan":{"id":"home"},"colour":"red"}"#,
        r#"{"plan":"id = 'home'"}"#,
    ] {
        let response = daemon
            .send(build(
                Method::POST,
                "/api/v1/dry-runs",
                &[("authorization", bearer.as_str())],
                body,
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json(response).await["code"], "invalid-body", "{body}");
    }
    assert!(daemon.dry_runs.askers().is_empty());
}

#[tokio::test]
async fn each_refusal_is_answered_with_the_status_and_code_of_its_kind() {
    let text = |value: &str| serde_json::json!({ "type": "text", "value": value });
    let count = |value: u64| serde_json::json!({ "type": "count", "value": value });
    for (refusal, status, expected) in [
        (
            DryRunsError::Busy,
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "dry-run-busy" }),
        ),
        (
            DryRunsError::GivesWay,
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "dry-run-gives-way" }),
        ),
        (
            DryRunsError::NotAllowed {
                setting: "glob-files",
                entry: 2,
            },
            StatusCode::FORBIDDEN,
            serde_json::json!({
                "code": "dry-run-not-allowed",
                "args": { "setting": text("glob-files"), "entry": count(2) },
            }),
        ),
        (
            DryRunsError::ReadsAbove {
                setting: "git-ignore",
            },
            StatusCode::FORBIDDEN,
            serde_json::json!({
                "code": "dry-run-reads-above",
                "args": { "setting": text("git-ignore") },
            }),
        ),
        (
            DryRunsError::Filter,
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({ "code": "dry-run-filter" }),
        ),
        (
            DryRunsError::Failed,
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "code": "dry-run-failed" }),
        ),
        (
            DryRunsError::Plan(PlansError::Invalid {
                field: "sources".to_owned(),
                problem: api::PlanProblem::NoSources,
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({
                "code": "plan-invalid",
                "args": { "field": text("sources"), "problem": text("no-sources") },
            }),
        ),
        (
            DryRunsError::FolderInvalid { entry: 3 },
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({
                "code": "dry-run-folder-invalid",
                "args": { "entry": count(3) },
            }),
        ),
        (
            DryRunsError::TooLong { limit: 64 },
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({
                "code": "dry-run-allowlist-too-long",
                "args": { "limit": count(64) },
            }),
        ),
        (
            DryRunsError::Unavailable {
                code: api::ErrorCode::ConfigUnavailable,
                args: Vec::new(),
                detail: None,
            },
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "code": "config-unavailable" }),
        ),
    ] {
        let response = refusal_of(&refusal);
        assert_eq!(response.status(), status, "{refusal:?}");
        assert_eq!(json(response).await, expected, "{refusal:?}");
    }
    let waits = refusal_of(&DryRunsError::TooSoon { seconds: 90 });
    assert_eq!(waits.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(waits.headers()[RETRY_AFTER], "90");
    assert_eq!(
        json(waits).await,
        serde_json::json!({ "code": "dry-run-rate", "args": { "seconds": count(90) } })
    );
}

fn refusal_of(error: &DryRunsError) -> Response {
    refusal(error)
}
