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
use domain::access::api_token::{ApiTokenLabel, Scope, Scopes};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::api_tokens::required_scope;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, SignIn, router};

const PASSWORD: &str = "demo-admin-pass";
const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// Answers every request with one proposal, or with the refusal it was
/// given, and records who asked.
#[derive(Default)]
struct Scripted {
    askers: Mutex<Vec<Proposer>>,
    refusal: Mutex<Option<ProposalsError>>,
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

    /// A token with `scopes` labelled `assistant`, as a program sends it,
    /// and who it is.
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
fn the_proposal_routes_need_the_propose_scope_and_no_others_open_to_it() {
    for (method, path, scope) in [
        (Method::GET, "/api/v1/proposals", Some(Scope::Propose)),
        (Method::HEAD, "/api/v1/proposals", Some(Scope::Propose)),
        (Method::POST, "/api/v1/proposals", Some(Scope::Propose)),
        (
            Method::GET,
            "/api/v1/proposals/abcdefghijkl",
            Some(Scope::Propose),
        ),
        (
            Method::POST,
            "/api/v1/proposals/abcdefghijkl/withdraw",
            Some(Scope::Propose),
        ),
        (Method::DELETE, "/api/v1/proposals/abcdefghijkl", None),
        (Method::POST, "/api/v1/proposals/abcdefghijkl", None),
        (Method::PUT, "/api/v1/proposals", None),
        (Method::GET, "/api/v1/proposals/abcdefghijkl/withdraw", None),
        (Method::POST, "/api/v1/proposals/abcdefghijkl/approve", None),
        (Method::POST, "/api/v1/proposals//withdraw", None),
    ] {
        assert_eq!(required_scope(&method, path), scope, "{method} {path}");
    }
}

#[tokio::test]
async fn a_token_with_the_propose_scope_asks_as_itself_on_every_proposal_route() {
    let daemon = Daemon::new();
    let (bearer, itself) = daemon.token(&[Scope::Propose]).await;
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
async fn a_token_without_the_propose_scope_is_refused_naming_it() {
    let daemon = Daemon::new();
    let (bearer, _) = daemon.token(&[Scope::Read, Scope::Run]).await;
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
            "code": "api-token-scope",
            "args": { "scope": { "type": "text", "value": "propose" } }
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
    let (bearer, _) = daemon.token(&[Scope::Propose]).await;
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
        let (bearer, _) = daemon.token(&[Scope::Propose]).await;
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
