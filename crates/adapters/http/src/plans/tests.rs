use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, HOST};
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, router};

/// What a route asked of the plans: the call, the path's ID and the body.
type Call = (&'static str, Option<String>, Option<Value>);

/// Plans that answer each call with the plan sent (or a demo plan), or with
/// `refusal`, and keep what each was asked.
#[derive(Default)]
struct Scripted {
    refusal: Option<PlansError>,
    calls: Mutex<Vec<Call>>,
}

impl Scripted {
    fn refusing(refusal: PlansError) -> Self {
        Self {
            refusal: Some(refusal),
            calls: Mutex::default(),
        }
    }

    fn answer<T>(
        &self,
        call: &'static str,
        id: Option<String>,
        plan: Option<&api::PlanDto>,
        ok: T,
    ) -> Result<T, PlansError> {
        let body = plan.map(|plan| Value::Object(plan.0.clone()));
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((call, id, body));
        self.refusal.clone().map_or(Ok(ok), Err)
    }

    fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn demo() -> api::PlanDto {
    let table = json!({"id": "home", "repository": "demo", "sources": ["/home/demo"]});
    let Value::Object(table) = table else {
        panic!("a JSON object")
    };
    api::PlanDto(table)
}

#[async_trait]
impl Plans for Scripted {
    async fn plan(&self, id: String) -> Result<api::PlanDto, PlansError> {
        self.answer("plan", Some(id), None, demo())
    }

    async fn create(&self, plan: api::PlanDto) -> Result<api::PlanDto, PlansError> {
        self.answer("create", None, Some(&plan), plan.clone())
    }

    async fn change(&self, id: String, plan: api::PlanDto) -> Result<api::PlanDto, PlansError> {
        self.answer("change", Some(id), Some(&plan), plan.clone())
    }

    async fn delete(&self, id: String) -> Result<(), PlansError> {
        self.answer("delete", Some(id), None, ())
    }
}

/// Sends `method` on `path` with `body` to a daemon over `plans`, and
/// answers the status and the body read as JSON (`Null` when empty).
async fn send(
    plans: &Arc<Scripted>,
    method: Method,
    path: &str,
    body: &str,
) -> (StatusCode, Value) {
    let mut state = test_state();
    state.plans = Arc::<Scripted>::clone(plans);
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, "localhost:9898")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn text(value: &str) -> Value {
    json!({"type": "text", "value": value})
}

const PLAN: &str = r#"{"id":"home","repository":"demo","sources":["/home/demo"]}"#;

#[tokio::test]
async fn a_plan_is_created_from_its_table_and_answered_as_saved() {
    let plans = Arc::new(Scripted::default());
    let (status, body) = send(&plans, Method::POST, "/api/v1/plans", PLAN).await;
    assert_eq!(status, StatusCode::CREATED);
    let sent: Value = serde_json::from_str(PLAN).unwrap();
    assert_eq!(body, sent);
    assert_eq!(plans.calls(), [("create", None, Some(sent))]);
}

#[tokio::test]
async fn a_plan_is_changed_by_the_id_its_path_names() {
    let plans = Arc::new(Scripted::default());
    let (status, body) = send(&plans, Method::PUT, "/api/v1/plans/home", PLAN).await;
    assert_eq!(status, StatusCode::OK);
    let sent: Value = serde_json::from_str(PLAN).unwrap();
    assert_eq!(body, sent);
    assert_eq!(
        plans.calls(),
        [("change", Some("home".to_owned()), Some(sent))]
    );
}

#[tokio::test]
async fn a_plan_is_read_and_deleted_by_the_id_its_path_names() {
    let plans = Arc::new(Scripted::default());
    let (status, body) = send(&plans, Method::GET, "/api/v1/plans/home", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, Value::Object(demo().0));
    let (status, body) = send(&plans, Method::DELETE, "/api/v1/plans/home", "{}").await;
    assert_eq!((status, body), (StatusCode::NO_CONTENT, Value::Null));
    assert_eq!(
        plans.calls(),
        [
            ("plan", Some("home".to_owned()), None),
            ("delete", Some("home".to_owned()), None),
        ]
    );
}

#[tokio::test]
async fn a_body_that_is_not_a_plans_table_is_refused_unasked() {
    let plans = Arc::new(Scripted::default());
    for (method, path) in [
        (Method::POST, "/api/v1/plans"),
        (Method::PUT, "/api/v1/plans/home"),
    ] {
        for body in ["[1]", r#""home""#, "{", ""] {
            let (status, refusal) = send(&plans, method.clone(), path, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {body}");
            assert_eq!(refusal["code"], "invalid-body", "{method} {body}");
        }
    }
    assert_eq!(plans.calls(), []);
}

#[tokio::test]
async fn a_plan_the_file_would_not_load_is_refused_at_its_field_with_no_value() {
    let plans = Arc::new(Scripted::refusing(PlansError::Invalid {
        field: "schedule.cron".to_owned(),
        problem: api::PlanProblem::Invalid,
    }));
    let (status, refusal) = send(&plans, Method::POST, "/api/v1/plans", PLAN).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        refusal,
        json!({
            "code": "plan-invalid",
            "args": {"field": text("schedule.cron"), "problem": text("invalid")},
        })
    );
}

#[tokio::test]
async fn a_plan_the_configuration_does_not_hold_is_not_found() {
    let plans = Arc::new(Scripted::refusing(PlansError::Unknown {
        plan: "gone".to_owned(),
    }));
    for (method, body) in [
        (Method::GET, ""),
        (Method::PUT, PLAN),
        (Method::DELETE, "{}"),
    ] {
        let (status, refusal) = send(&plans, method.clone(), "/api/v1/plans/gone", body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method}");
        assert_eq!(
            refusal,
            json!({"code": "plan-unknown", "args": {"plan": text("gone")}}),
            "{method}"
        );
    }
}

#[tokio::test]
async fn a_plan_a_rule_names_is_refused_naming_the_rule() {
    let plans = Arc::new(Scripted::refusing(PlansError::InUse {
        plan: "home".to_owned(),
        rule: "failures".to_owned(),
    }));
    let (status, refusal) = send(&plans, Method::DELETE, "/api/v1/plans/home", "{}").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        refusal,
        json!({
            "code": "plan-in-use",
            "args": {"plan": text("home"), "rule": text("failures")},
        })
    );
}

#[tokio::test]
async fn a_configuration_that_cannot_be_used_is_unavailable_with_its_words() {
    let plans = Arc::new(Scripted::refusing(PlansError::Unavailable {
        code: api::ErrorCode::ConfigUnavailable,
        args: vec![("file", api::MsgArg::Text("config.toml".to_owned()))],
        detail: Some("permission denied".to_owned()),
    }));
    let (status, refusal) = send(&plans, Method::POST, "/api/v1/plans", PLAN).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        refusal,
        json!({
            "code": "config-unavailable",
            "args": {"file": text("config.toml")},
            "detail": "permission denied",
        })
    );
}
