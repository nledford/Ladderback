use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use app::access::Access;
use app::fakes::{
    FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, InMemorySessionStore, SeededRandom,
};
use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::header::{HOST, SET_COOKIE};
use axum::http::{Method, StatusCode};
use domain::Timestamp;
use domain::access::api_token::{ApiTokenLabel, TokenPermissions};
use domain::access::permissions::{Permission, Permissions};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::api_tokens::{Needs, required_permission};
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, SignIn, router};

const PASSWORD: &str = "demo-admin-pass";
const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// Answers every request with one proposal, or with the refusal it was
/// given, and records who asked, and each approval and rejection.
#[derive(Default)]
struct Scripted {
    askers: Mutex<Vec<Proposer>>,
    refusal: Mutex<Option<ProposalsError>>,
    decisions: Mutex<Vec<Decision>>,
}

/// What a route asked of the proposals beyond a proposer's requests; an
/// approval as its revision, its diff's hash, its address and its
/// confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Decision {
    Review(String),
    ForgetPreview(String),
    Approve(String, String),
    Reject(String, Vec<String>),
}

impl Scripted {
    fn answer(&self, asker: Proposer) -> Result<api::ProposalDto, ProposalsError> {
        self.askers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(asker);
        match self
            .refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            Some(refusal) => Err(refusal),
            None => Ok(proposal()),
        }
    }

    fn askers(&self) -> Vec<Proposer> {
        self.askers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records `decision`, then answers as [`Scripted::answer`] does, with
    /// no asker.
    fn decide(&self, decision: Decision) -> Result<api::ProposalDto, ProposalsError> {
        self.decisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(decision);
        match self
            .refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            Some(refusal) => Err(refusal),
            None => Ok(proposal()),
        }
    }

    fn decisions(&self) -> Vec<Decision> {
        self.decisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn proposal() -> api::ProposalDto {
    api::ProposalDto {
        id: "abcdefghijkl".to_owned(),
        proposed_by: Some("assistant".to_owned()),
        status: api::ProposalStatusDto::Pending,
        created_at: "2027-01-15T08:00:00Z".to_owned(),
        expires_at: "2027-01-17T08:00:00Z".to_owned(),
        changes: vec![api::ProposalChangeDto::RemovePlan {
            plan: "home".to_owned(),
        }],
        rationale: String::new(),
        decided_at: None,
        reasons: Vec::new(),
        warnings: Vec::new(),
    }
}

/// A forget preview of one repository: the plan `home` forgets one
/// snapshot now and none once it is removed.
fn forget_preview() -> api::ProposalForgetPreviewDto {
    let forgotten = api::ForgetPlanDto {
        plan: "home".to_owned(),
        total: 3,
        remove: 1,
        listed: vec![api::SnapshotDto {
            id: "a".repeat(64),
            time: "2027-01-14T08:00:00Z".to_owned(),
            offset: None,
            hostname: "demo-host".to_owned(),
            label: String::new(),
            tags: Vec::new(),
            tags_truncated: false,
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: false,
        }],
    };
    api::ProposalForgetPreviewDto {
        repositories: vec![api::RepositoryForgetDto::Compared {
            repository: "demo".to_owned(),
            before: api::ForgetSideDto::Forgets {
                plans: vec![forgotten.clone()],
            },
            after: api::ForgetSideDto::Forgets { plans: Vec::new() },
            forgets_more: Vec::new(),
            forgets_less: vec![forgotten],
        }],
        digest: Some("c".repeat(64)),
    }
}

#[async_trait]
impl Proposals for Scripted {
    async fn propose(
        &self,
        asker: Proposer,
        _proposal: api::NewProposalDto,
    ) -> Result<api::ProposalDto, ProposalsError> {
        self.answer(asker)
    }

    async fn list(&self, asker: Proposer) -> Result<api::ProposalsDto, ProposalsError> {
        Ok(api::ProposalsDto {
            proposals: vec![self.answer(asker)?],
        })
    }

    async fn proposal(
        &self,
        asker: Proposer,
        _id: String,
    ) -> Result<api::ProposalDto, ProposalsError> {
        self.answer(asker)
    }

    async fn withdraw(
        &self,
        asker: Proposer,
        _id: String,
    ) -> Result<api::ProposalDto, ProposalsError> {
        self.answer(asker)
    }

    async fn review(&self, id: String) -> Result<api::ProposalReviewDto, ProposalsError> {
        let proposal = self.decide(Decision::Review(id))?;
        Ok(api::ProposalReviewDto {
            proposal,
            summary: vec![api::SummaryLineDto::RemovesPlan {
                plan: "home".to_owned(),
            }],
            diff: "-[[plans]]\n".to_owned(),
            revision: "a".repeat(64),
            diff_hash: "b".repeat(64),
            changed_since_drafted: false,
            needs_forget_preview: true,
            rejected: 0,
            short_id: "abcdef".to_owned(),
        })
    }

    async fn forget_preview(
        &self,
        id: String,
    ) -> Result<api::ProposalForgetPreviewDto, ProposalsError> {
        self.decide(Decision::ForgetPreview(id))?;
        Ok(forget_preview())
    }

    async fn approve(
        &self,
        id: String,
        approval: Approval,
    ) -> Result<api::ProposalDto, ProposalsError> {
        let Approval {
            revision,
            diff_hash,
            forget_digest,
            address,
            confirmation,
        } = approval;
        self.decide(Decision::Approve(
            id,
            format!(
                "{revision}:{diff_hash}:{}:{address}:{}",
                forget_digest.as_deref().unwrap_or("none"),
                confirmation.as_password().expose()
            ),
        ))
    }

    async fn reject(
        &self,
        id: String,
        reasons: Vec<String>,
    ) -> Result<api::ProposalDto, ProposalsError> {
        self.decide(Decision::Reject(id, reasons))
    }
}

/// A daemon with sign-in on and the scripted proposals.
struct Daemon {
    access: Arc<Access>,
    proposals: Arc<Scripted>,
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
            proposals: Arc::default(),
        }
    }

    async fn send(&self, request: Request) -> Response {
        let mut state = test_state();
        state.sign_in = SignIn::On(Arc::clone(&self.access));
        state.proposals = self.proposals.clone();
        router(Arc::new(NoUi), RequestChecks::default(), state)
            .oneshot(request)
            .await
            .unwrap()
    }

    /// A token with `permissions` labeled `assistant`, as a program sends it,
    /// and who it is.
    async fn token(&self, permissions: &[Permission]) -> (String, Proposer) {
        let created = self
            .access
            .create_api_token(
                ApiTokenLabel::new("assistant".to_owned()).unwrap(),
                TokenPermissions::new(permissions.iter().fold(Permissions::NONE, |all, row| {
                    all.with(*row, Some(row.level())).unwrap()
                }))
                .unwrap(),
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

fn build(method: Method, path: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, "127.0.0.1:8080");
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

async fn json(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

const PROPOSED: &str = r#"{"changes":[{"kind":"remove-plan","plan":"home"}]}"#;

#[test]
fn the_proposal_routes_need_the_configuration_permission_and_no_others_open_to_it() {
    let configuration = Some(Permission::Configuration);
    for (method, path, permission) in [
        (Method::GET, "/api/v1/proposals", configuration),
        (Method::HEAD, "/api/v1/proposals", configuration),
        (Method::POST, "/api/v1/proposals", configuration),
        (Method::GET, "/api/v1/proposals/abcdefghijkl", configuration),
        (
            Method::POST,
            "/api/v1/proposals/abcdefghijkl/withdraw",
            configuration,
        ),
        (Method::DELETE, "/api/v1/proposals/abcdefghijkl", None),
        (Method::POST, "/api/v1/proposals/abcdefghijkl", None),
        (Method::PUT, "/api/v1/proposals", None),
        (Method::GET, "/api/v1/proposals/abcdefghijkl/withdraw", None),
        (Method::POST, "/api/v1/proposals/abcdefghijkl/approve", None),
        (Method::GET, "/api/v1/proposals/abcdefghijkl/review", None),
        (
            Method::POST,
            "/api/v1/proposals/abcdefghijkl/forget-preview",
            None,
        ),
        (
            Method::GET,
            "/api/v1/proposals/abcdefghijkl/forget-preview",
            None,
        ),
        (Method::POST, "/api/v1/proposals/abcdefghijkl/reject", None),
        (Method::POST, "/api/v1/proposals//withdraw", None),
    ] {
        let needs = permission.map_or(Needs::Session, Needs::Permission);
        assert_eq!(required_permission(&method, path), needs, "{method} {path}");
    }
}

#[tokio::test]
async fn a_token_with_the_propose_scope_asks_as_itself_on_every_proposal_route() {
    let daemon = Daemon::new();
    let (bearer, itself) = daemon.token(&[Permission::Configuration]).await;
    let auth = [("authorization", bearer.as_str())];
    let made = daemon
        .send(post("/api/v1/proposals", &auth, PROPOSED))
        .await;
    assert_eq!(made.status(), StatusCode::CREATED);
    assert_eq!(json(made).await["proposed-by"], "assistant");
    for (request, status) in [
        (get("/api/v1/proposals", &auth), StatusCode::OK),
        (get("/api/v1/proposals/abcdefghijkl", &auth), StatusCode::OK),
        (
            post("/api/v1/proposals/abcdefghijkl/withdraw", &auth, ""),
            StatusCode::OK,
        ),
    ] {
        let what = format!("{} {}", request.method(), request.uri());
        assert_eq!(daemon.send(request).await.status(), status, "{what}");
    }
    assert_eq!(daemon.proposals.askers(), vec![itself; 4]);
}

#[tokio::test]
async fn a_token_without_the_configuration_permission_is_refused_naming_it() {
    let daemon = Daemon::new();
    let (bearer, _) = daemon
        .token(&[Permission::Backups, Permission::BackupRuns])
        .await;
    let response = daemon
        .send(post(
            "/api/v1/proposals",
            &[("authorization", bearer.as_str())],
            PROPOSED,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json(response).await,
        serde_json::json!({
            "code": "api-token-permission",
            "args": { "permission": { "type": "text", "value": "configuration" } }
        })
    );
    assert!(daemon.proposals.askers().is_empty());
}

#[tokio::test]
async fn a_session_asks_with_no_token() {
    let daemon = Daemon::new();
    let (cookie, csrf) = daemon.signed_in().await;
    let csrf_header = crate::CSRF_HEADER;
    let headers = [
        ("cookie", cookie.as_str()),
        (csrf_header.as_str(), csrf.as_str()),
    ];
    let response = daemon
        .send(post("/api/v1/proposals", &headers, PROPOSED))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let listed = daemon.send(get("/api/v1/proposals", &headers)).await;
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json(listed).await["proposals"][0]["id"], "abcdefghijkl");
    assert_eq!(
        daemon.proposals.askers(),
        [Proposer::NoToken, Proposer::NoToken]
    );
}

#[tokio::test]
async fn a_body_that_is_not_a_proposal_is_refused_unread() {
    let daemon = Daemon::new();
    let (bearer, _) = daemon.token(&[Permission::Configuration]).await;
    let auth = [("authorization", bearer.as_str())];
    for body in [
        "not json",
        r#"{"changes":[{"kind":"rename-plan","plan":"home"}]}"#,
        r#"{"changes":[],"colour":"red"}"#,
        r#"{"rationale":"no changes key"}"#,
    ] {
        let response = daemon.send(post("/api/v1/proposals", &auth, body)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json(response).await["code"], "invalid-body", "{body}");
    }
    assert!(daemon.proposals.askers().is_empty());
}

#[tokio::test]
async fn each_refusal_is_answered_with_the_status_and_code_of_its_kind() {
    let text = |value: &str| serde_json::json!({ "type": "text", "value": value });
    let count = |value: u64| serde_json::json!({ "type": "count", "value": value });
    for (refusal, status, expected) in [
        (
            ProposalsError::InvalidBody,
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "code": "invalid-body" }),
        ),
        (
            ProposalsError::Empty,
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({ "code": "proposal-empty" }),
        ),
        (
            ProposalsError::Invalid {
                change: 2,
                field: "schedule.cron".to_owned(),
                problem: api::ChangeProblem::Plan(api::PlanProblem::Invalid),
            },
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({ "code": "proposal-invalid", "args": {
                "change": count(2), "field": text("schedule.cron"), "problem": text("invalid"),
            }}),
        ),
        (
            ProposalsError::TooLarge {
                what: "changes",
                limit: 50,
            },
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({ "code": "proposal-too-large", "args": {
                "what": text("changes"), "limit": count(50),
            }}),
        ),
        (
            ProposalsError::Limit { limit: 20 },
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "proposal-limit", "args": { "limit": count(20) } }),
        ),
        (
            ProposalsError::Unknown {
                proposal: "abcdefghijkl".to_owned(),
            },
            StatusCode::NOT_FOUND,
            serde_json::json!({ "code": "proposal-unknown", "args": {
                "proposal": text("abcdefghijkl"),
            }}),
        ),
        (
            ProposalsError::NotPending {
                status: ProposalStatus::Expired,
            },
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "proposal-not-pending", "args": {
                "status": text("expired"),
            }}),
        ),
        (
            ProposalsError::Unavailable {
                code: api::ErrorCode::ProposalsUnavailable,
                args: Vec::new(),
                detail: None,
            },
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "code": "proposals-unavailable" }),
        ),
    ] {
        let daemon = Daemon::new();
        *daemon
            .proposals
            .refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(refusal.clone());
        let (bearer, _) = daemon.token(&[Permission::Configuration]).await;
        let response = daemon
            .send(post(
                "/api/v1/proposals/abcdefghijkl/withdraw",
                &[("authorization", bearer.as_str())],
                "",
            ))
            .await;
        assert_eq!(response.status(), status, "{refusal:?}");
        assert_eq!(json(response).await, expected, "{refusal:?}");
    }
}

/// A signed-in browser's headers: its session cookie and CSRF token.
async fn session_headers(daemon: &Daemon) -> Vec<(String, String)> {
    let (cookie, csrf) = daemon.signed_in().await;
    vec![
        ("cookie".to_owned(), cookie),
        (crate::CSRF_HEADER.as_str().to_owned(), csrf),
    ]
}

fn borrowed(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

const APPROVAL: &str = r#"{"revision":"aaaa","diff-hash":"bbbb","forget-digest":"cccc","confirmation":"demo-admin-pass"}"#;

#[tokio::test]
async fn no_token_reviews_approves_or_rejects_whatever_its_permissions() {
    let daemon = Daemon::new();
    let (bearer, _) = daemon.token(&Permission::ALL).await;
    let auth = [("authorization", bearer.as_str())];
    for request in [
        get("/api/v1/proposals/abcdefghijkl/review", &auth),
        post("/api/v1/proposals/abcdefghijkl/forget-preview", &auth, ""),
        post("/api/v1/proposals/abcdefghijkl/approve", &auth, APPROVAL),
        post(
            "/api/v1/proposals/abcdefghijkl/reject",
            &auth,
            r#"{"reasons":[""]}"#,
        ),
    ] {
        let what = format!("{} {}", request.method(), request.uri());
        let response = daemon.send(request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{what}");
        assert_eq!(
            json(response).await,
            serde_json::json!({
                "code": "api-token-permission",
                "args": { "permission": { "type": "text", "value": "session" } }
            }),
            "{what}"
        );
    }
    assert!(daemon.proposals.decisions().is_empty());
}

#[tokio::test]
async fn a_session_reviews_approves_and_rejects_as_its_requests_say() {
    let daemon = Daemon::new();
    let headers = session_headers(&daemon).await;
    let headers = borrowed(&headers);
    let reviewed = daemon
        .send(get("/api/v1/proposals/abcdefghijkl/review", &headers))
        .await;
    assert_eq!(reviewed.status(), StatusCode::OK);
    let reviewed = json(reviewed).await;
    assert_eq!(reviewed["short-id"], "abcdef");
    assert_eq!(reviewed["summary"][0]["kind"], "removes-plan");
    let approved = daemon
        .send(post(
            "/api/v1/proposals/abcdefghijkl/approve",
            &headers,
            APPROVAL,
        ))
        .await;
    assert_eq!(approved.status(), StatusCode::OK);
    let rejected = daemon
        .send(post(
            "/api/v1/proposals/abcdefghijkl/reject",
            &headers,
            r#"{"reasons":["Back up weekly instead"]}"#,
        ))
        .await;
    assert_eq!(rejected.status(), StatusCode::OK);
    assert_eq!(
        daemon.proposals.decisions(),
        [
            Decision::Review("abcdefghijkl".to_owned()),
            Decision::Approve(
                "abcdefghijkl".to_owned(),
                format!("aaaa:bbbb:cccc:{}:{PASSWORD}", HOME.ip())
            ),
            Decision::Reject(
                "abcdefghijkl".to_owned(),
                vec!["Back up weekly instead".to_owned()]
            ),
        ]
    );
}

/// The forget preview answers a session what the proposal would forget in
/// each repository, by snapshot ID, time and count, as the snapshot
/// listing shows them (decision 89, point 8). It asks nothing of the
/// proposal but its preview.
#[tokio::test]
async fn a_session_previews_what_a_proposal_would_forget() {
    let daemon = Daemon::new();
    let headers = session_headers(&daemon).await;
    let headers = borrowed(&headers);
    let previewed = daemon
        .send(post(
            "/api/v1/proposals/abcdefghijkl/forget-preview",
            &headers,
            "",
        ))
        .await;
    assert_eq!(previewed.status(), StatusCode::OK);
    let previewed = json(previewed).await;
    assert_eq!(
        serde_json::from_value::<api::ProposalForgetPreviewDto>(previewed.clone()).ok(),
        Some(forget_preview()),
        "{previewed}"
    );
    assert_eq!(previewed["digest"], "c".repeat(64));
    let repository = &previewed["repositories"][0];
    assert_eq!(repository["kind"], "compared");
    assert_eq!(repository["forgets-less"][0]["remove"], 1);
    assert_eq!(
        daemon.proposals.decisions(),
        [Decision::ForgetPreview("abcdefghijkl".to_owned())]
    );
}

#[tokio::test]
async fn an_approval_or_rejection_that_is_not_one_is_refused_unread() {
    let daemon = Daemon::new();
    let headers = session_headers(&daemon).await;
    let headers = borrowed(&headers);
    for (path, body) in [
        ("approve", "not json"),
        ("approve", r#"{"revision":"aaaa","diff-hash":"bbbb"}"#),
        (
            "approve",
            r#"{"revision":"aaaa","diff-hash":"bbbb","confirmation":"x","colour":"red"}"#,
        ),
        ("reject", r#"{"reasons":"none"}"#),
        ("reject", "{}"),
    ] {
        let response = daemon
            .send(post(
                &format!("/api/v1/proposals/abcdefghijkl/{path}"),
                &headers,
                body,
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json(response).await["code"], "invalid-body", "{body}");
    }
    assert!(daemon.proposals.decisions().is_empty());
}

#[tokio::test]
async fn an_approval_confirmed_by_nothing_or_more_than_any_password_fails_unasked() {
    let daemon = Daemon::new();
    let headers = session_headers(&daemon).await;
    let headers = borrowed(&headers);
    let longest = domain::access::PASSWORD_MAX_BYTES;
    // The last is the proposal's short ID, padded with spaces.
    for confirmation in [
        String::new(),
        "a".repeat(longest + 1),
        format!("{}abcdef", " ".repeat(longest)),
    ] {
        let body = serde_json::json!({
            "revision": "aaaa",
            "diff-hash": "bbbb",
            "confirmation": confirmation,
        })
        .to_string();
        let response = daemon
            .send(post(
                "/api/v1/proposals/abcdefghijkl/approve",
                &headers,
                &body,
            ))
            .await;
        let length = confirmation.len();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{length} bytes");
        assert_eq!(
            json(response).await["code"],
            "step-up-failed",
            "{length} bytes"
        );
    }
    assert!(daemon.proposals.decisions().is_empty());
}

#[tokio::test]
async fn each_refusal_of_a_decision_is_answered_with_the_status_and_code_of_its_kind() {
    let count = |value: u64| serde_json::json!({ "type": "count", "value": value });
    let text = |value: &str| serde_json::json!({ "type": "text", "value": value });
    for (refusal, status, expected, retry_after) in [
        (
            ProposalsError::Changed,
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "proposal-changed" }),
            None,
        ),
        (
            ProposalsError::NeedsForgetPreview,
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "proposal-needs-forget-preview" }),
            None,
        ),
        (
            ProposalsError::ForgetPreviewChanged,
            StatusCode::CONFLICT,
            serde_json::json!({ "code": "proposal-forget-preview-changed" }),
            None,
        ),
        (
            ProposalsError::ReasonCount { changes: 2 },
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({ "code": "proposal-reasons", "args": {
                "problem": text("count"), "change": count(2),
            }}),
            None,
        ),
        (
            ProposalsError::ReasonTooLong { change: 3 },
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({ "code": "proposal-reasons", "args": {
                "problem": text("too-long"), "change": count(3),
            }}),
            None,
        ),
        (
            ProposalsError::StepUpFailed,
            StatusCode::FORBIDDEN,
            serde_json::json!({ "code": "step-up-failed" }),
            None,
        ),
        (
            ProposalsError::StepUpWait { seconds: 4 },
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "code": "sign-in-wait", "args": { "seconds": count(4) } }),
            Some("4"),
        ),
        (
            ProposalsError::StepUpUnavailable("the data folder can't be read".to_owned()),
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({ "code": "sign-in-unavailable" }),
            None,
        ),
        (
            ProposalsError::TooSoon {
                after_rejection: false,
                seconds: 600,
            },
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "code": "proposal-rate", "args": { "seconds": count(600) } }),
            Some("600"),
        ),
        (
            ProposalsError::TooSoon {
                after_rejection: true,
                seconds: 300,
            },
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "code": "proposal-after-rejection", "args": {
                "seconds": count(300),
            }}),
            Some("300"),
        ),
    ] {
        let daemon = Daemon::new();
        *daemon
            .proposals
            .refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(refusal.clone());
        let headers = session_headers(&daemon).await;
        let response = daemon
            .send(post(
                "/api/v1/proposals/abcdefghijkl/approve",
                &borrowed(&headers),
                APPROVAL,
            ))
            .await;
        assert_eq!(response.status(), status, "{refusal:?}");
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .map(|value| value.to_str().unwrap()),
            retry_after,
            "{refusal:?}"
        );
        assert_eq!(json(response).await, expected, "{refusal:?}");
    }
}
