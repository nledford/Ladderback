//! Connect mode against a stand-in daemon: the API's routes on a loopback
//! port, answering as the daemon's do.

use std::collections::HashMap;
use std::sync::Arc;

use api::{BuildDto, ErrorCode, ExposureDto, OperationSummaryDto};
use axum::Router;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::{Value, json};

use super::*;

/// Serves `routes` on a free loopback port, for as long as the test runs.
async fn daemon(routes: Router) -> DaemonUrl {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, routes).await });
    DaemonUrl::parse(&format!("http://{addr}")).unwrap()
}

async fn status_of(daemon: DaemonUrl) -> Result<BackendStatus, ErrorDto> {
    Connect::new(daemon).unwrap().status().await
}

fn operation(id: u8, plan: &str, status: &str) -> Value {
    json!({
        "id": format!("00000000-0000-7000-8000-0000000000{id:02}"),
        "plan": plan,
        "kind": "backup",
        "status": status,
        "queued-at": "2026-11-14T10:00:00Z",
        "started-at": null,
        "finished-at": null,
        "warning-codes": [],
        "failure-code": null,
        "result": null,
    })
}

fn page(operations: Vec<Value>) -> Value {
    json!({"operations": operations, "next": null})
}

/// A daemon with sign-in off on loopback, the plans `home` (last run
/// succeeded) and `photos` (never run), and three recent operations, two of
/// them unfinished.
fn a_daemon_with_two_plans() -> Router {
    two_plans().merge(session_answering("off"))
}

/// A session route that answers `sign-in` as `sign_in`, with the build, as
/// the daemon answers with sign-in off or to a `read` token.
fn session_answering(sign_in: &'static str) -> Router {
    Router::new().route(
        "/api/v1/session",
        get(move || async move {
            axum::Json(json!({
                "sign-in": sign_in,
                "version": "0.3.0",
                "commit": "3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90",
                "dirty": false,
                "release": false,
            }))
        }),
    )
}

/// The plans and history routes of [`a_daemon_with_two_plans`].
fn two_plans() -> Router {
    Router::new()
        .route(
            "/api/v1/plans",
            get(|| async {
                axum::Json(json!({"plans": [
                    {"id": "home", "repository": "nas", "scheduled": true, "next-run": "2026-11-15T02:00:00Z"},
                    {"id": "photos", "repository": "nas", "scheduled": false, "next-run": null},
                ]}))
            }),
        )
        .route(
            "/api/v1/operations",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                let limit = query.get("limit").map(String::as_str);
                let answer = match (query.get("plan").map(String::as_str), limit) {
                    (Some("home"), Some("1")) => page(vec![operation(1, "home", "succeeded")]),
                    (Some(_), Some("1")) => page(Vec::new()),
                    (None, Some("200")) => page(vec![
                        operation(3, "photos", "running"),
                        operation(2, "home", "queued"),
                        operation(1, "home", "succeeded"),
                    ]),
                    _ => return StatusCode::BAD_REQUEST.into_response(),
                };
                axum::Json(answer).into_response()
            }),
        )
}

fn summary(value: Value) -> OperationSummaryDto {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn status_reads_the_daemons_build_plans_last_operations_and_unfinished_operations() {
    let status = status_of(daemon(a_daemon_with_two_plans()).await)
        .await
        .unwrap();
    assert_eq!(
        status.daemon,
        Some(DaemonStatus {
            build: Some(BuildDto {
                version: "0.3.0".to_owned(),
                commit: Some("3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90".to_owned()),
                dirty: false,
                release: false,
            }),
            exposure: ExposureDto::Loopback,
        })
    );
    let plans: Vec<(&str, Option<&OperationSummaryDto>)> = status
        .plans
        .iter()
        .map(|plan| (plan.plan.id.as_str(), plan.last.as_ref()))
        .collect();
    assert_eq!(
        plans,
        [
            ("home", Some(&summary(operation(1, "home", "succeeded")))),
            ("photos", None),
        ]
    );
    assert_eq!(
        status.plans[0].plan.next_run.as_deref(),
        Some("2026-11-15T02:00:00Z")
    );
    assert_eq!(
        status.running,
        [
            summary(operation(3, "photos", "running")),
            summary(operation(2, "home", "queued")),
        ]
    );
}

#[tokio::test]
async fn access_is_told_by_how_this_program_connected_never_as_signed_out() {
    let token = || ApiToken::parse(TOKEN).expect("a token");
    // A token is ignored with sign-in off (decision 93).
    for (sign_in, with_token, access) in [
        ("off", false, Access::SignInOff),
        ("off", true, Access::SignInOff),
        ("signed-out", true, Access::ApiToken),
    ] {
        let url = daemon(two_plans().merge(session_answering(sign_in))).await;
        let mut connect = Connect::new(url).unwrap();
        if with_token {
            connect = connect.with_token(token()).unwrap();
        }
        let status = connect.status().await.unwrap();
        assert_eq!(status.access, access, "{sign_in}, token: {with_token}");
    }
}

#[tokio::test]
async fn sign_in_on_without_a_token_is_sign_in_required_before_any_list_is_asked() {
    // No plans route: asking it would be `daemon-unreachable`.
    let refusal = status_of(daemon(session_answering("signed-out")).await)
        .await
        .unwrap_err();
    assert_eq!(refusal, ErrorDto::new(ErrorCode::SignInRequired, [], None));
}

#[tokio::test]
async fn a_refusal_from_the_daemon_is_its_own_code_and_arguments() {
    let routes = Router::new().route(
        "/api/v1/session",
        get(|| async {
            (
                StatusCode::FORBIDDEN,
                axum::Json(json!({"code": "api-token-scope", "args": {"scope": {"type": "text", "value": "read"}}})),
            )
        }),
    );
    let refusal = status_of(daemon(routes).await).await.unwrap_err();
    assert_eq!(refusal.code, ErrorCode::ApiTokenScope);
    assert_eq!(
        serde_json::to_value(&refusal).unwrap(),
        json!({"code": "api-token-scope", "args": {"scope": {"type": "text", "value": "read"}}})
    );
}

#[tokio::test]
async fn a_refusal_that_isnt_the_apis_is_daemon_unreachable() {
    let routes = Router::new().route(
        "/api/v1/session",
        get(|| async { (StatusCode::BAD_GATEWAY, "<html>Bad gateway</html>") }),
    );
    let refusal = status_of(daemon(routes).await).await.unwrap_err();
    assert_eq!(
        refusal,
        ErrorDto::new(ErrorCode::DaemonUnreachable, [], None)
    );
}

#[tokio::test]
async fn an_answer_that_isnt_the_apis_is_daemon_unreachable() {
    let routes = Router::new().route(
        "/api/v1/session",
        get(|| async { "<html>Welcome to nginx</html>" }),
    );
    let refusal = status_of(daemon(routes).await).await.unwrap_err();
    assert_eq!(refusal.code, ErrorCode::DaemonUnreachable);
}

#[tokio::test]
async fn an_answer_over_the_limit_is_daemon_unreachable() {
    let routes = Router::new().route(
        "/api/v1/session",
        get(|| async {
            let padding = " ".repeat(MAX_ANSWER_BYTES);
            format!(r#"{{"sign-in": "off"{padding}}}"#)
        }),
    );
    let refusal = status_of(daemon(routes).await).await.unwrap_err();
    assert_eq!(refusal.code, ErrorCode::DaemonUnreachable);
}

#[tokio::test]
async fn an_answer_at_the_limit_is_read() {
    let routes = two_plans().route(
        "/api/v1/session",
        get(|| async {
            let head = r#"{"sign-in": "off""#;
            let padding = " ".repeat(MAX_ANSWER_BYTES - head.len() - 1);
            format!("{head}{padding}}}")
        }),
    );
    let status = status_of(daemon(routes).await).await.unwrap();
    assert_eq!(status.access, Access::SignInOff);
    assert_eq!(status.daemon.map(|daemon| daemon.build), Some(None));
}

#[tokio::test]
async fn nothing_listening_is_daemon_unreachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = DaemonUrl::parse(&format!("http://{addr}")).unwrap();
    let refusal = status_of(url).await.unwrap_err();
    assert_eq!(refusal.code, ErrorCode::DaemonUnreachable);
}

#[test]
fn a_daemon_url_is_http_or_https_with_a_host() {
    for text in [
        "http://127.0.0.1:9898",
        "https://nas.example.com/",
        DEFAULT_DAEMON_URL,
    ] {
        assert!(DaemonUrl::parse(text).is_ok(), "{text}");
    }
    for text in [
        "ftp://nas.example.com/",
        "file:///run/ladderback",
        "127.0.0.1:9898",
        "http://admin:canary-password@127.0.0.1:9898/",
        "http://admin@127.0.0.1:9898/",
        "http://127.0.0.1:9898/?token=x",
        "http://127.0.0.1:9898/#top",
        "not a url",
    ] {
        assert_eq!(DaemonUrl::parse(text), Err(InvalidDaemonUrl), "{text}");
    }
}

#[test]
fn routes_are_joined_below_the_daemon_urls_path() {
    for text in [
        "https://nas.example.com/ladderback",
        "https://nas.example.com/ladderback/",
    ] {
        let url = DaemonUrl::parse(text).unwrap();
        assert_eq!(
            url.route("api/v1/plans").map(String::from).as_deref(),
            Some("https://nas.example.com/ladderback/api/v1/plans"),
            "{text}"
        );
    }
}

/// A daemon whose snapshot listing names, as each snapshot's host, the
/// repository the request's path named, and whose operation route answers
/// for the operation `…01` alone.
fn a_daemon_that_echoes_the_path() -> Router {
    use axum::extract::Path;
    Router::new()
        .route(
            "/api/v1/repositories/{alias}/snapshots",
            get(|Path(alias): Path<String>| async move {
                axum::Json(json!({"snapshots": [{
                    "id": format!("{:064x}", 1),
                    "time": "2026-11-14T10:00:00Z",
                    "hostname": alias,
                    "label": "",
                    "tags": [],
                    "paths": [],
                }]}))
            }),
        )
        .route(
            "/api/v1/operations/{id}",
            get(|Path(id): Path<String>| async move {
                if id == "00000000-0000-7000-8000-000000000001" {
                    axum::Json(json!({
                        "operation": operation(1, "home", "failed"),
                        "warnings": [],
                    }))
                    .into_response()
                } else {
                    let refusal = json!({
                        "code": "operation-unknown",
                        "args": {"operation": {"type": "text", "value": id}},
                    });
                    (StatusCode::NOT_FOUND, axum::Json(refusal)).into_response()
                }
            }),
        )
}

#[tokio::test]
async fn the_repository_is_sent_as_one_segment_of_the_path_whatever_it_holds() {
    let connect = Connect::new(daemon(a_daemon_that_echoes_the_path()).await).unwrap();
    for alias in ["nas", "a b/c?d#e%2F", "../plans"] {
        let listed = connect.snapshots(alias.to_owned()).await.unwrap();
        let hosts: Vec<&str> = listed
            .snapshots
            .iter()
            .map(|snapshot| snapshot.hostname.as_str())
            .collect();
        assert_eq!(hosts, [alias]);
    }
}

#[tokio::test]
async fn a_name_no_path_segment_can_hold_is_refused_without_asking_the_daemon() {
    let connect = Connect::new(daemon(a_daemon_that_echoes_the_path()).await).unwrap();
    for name in ["", ".", ".."] {
        let text = api::MsgArg::Text(name.to_owned());
        assert_eq!(
            connect.snapshots(name.to_owned()).await,
            Err(ErrorDto::new(
                ErrorCode::RepositoryUnknown,
                [("alias", text.clone())],
                None
            )),
            "{name:?}"
        );
        assert_eq!(
            connect.operation(name.to_owned()).await,
            Err(ErrorDto::new(
                ErrorCode::OperationUnknown,
                [("operation", text.clone())],
                None
            )),
            "{name:?}"
        );
        let unknown_repository = ErrorDto::new(
            ErrorCode::RepositoryUnknown,
            [("alias", text.clone())],
            None,
        );
        assert_eq!(
            connect.test_repository(name.to_owned()).await.unwrap_err(),
            unknown_repository,
            "{name:?}"
        );
        assert_eq!(
            connect.preview_forget(name.to_owned()).await.unwrap_err(),
            unknown_repository,
            "{name:?}"
        );
        assert_eq!(
            connect.export_profile(name.to_owned()).await,
            Err(ErrorDto::new(
                ErrorCode::PlanUnknown,
                [("plan", text)],
                None
            )),
            "{name:?}"
        );
    }
}

#[tokio::test]
async fn an_operation_is_read_by_its_id_and_one_the_daemon_doesnt_hold_is_its_refusal() {
    let connect = Connect::new(daemon(a_daemon_that_echoes_the_path()).await).unwrap();
    let detail = connect
        .operation("00000000-0000-7000-8000-000000000001".to_owned())
        .await
        .unwrap();
    assert_eq!(detail.operation, summary(operation(1, "home", "failed")));
    let refused = connect.operation("a/b".to_owned()).await;
    assert_eq!(
        refused,
        Err(ErrorDto::new(
            ErrorCode::OperationUnknown,
            [("operation", api::MsgArg::Text("a/b".to_owned()))],
            None
        ))
    );
}

#[tokio::test]
async fn the_history_asks_for_the_plan_the_cursor_and_the_limit() {
    let routes = Router::new().route(
        "/api/v1/operations",
        get(|Query(query): Query<HashMap<String, String>>| async move {
            let asked = format!(
                "{:?} {:?} {:?}",
                query.get("plan"),
                query.get("after"),
                query.get("limit")
            );
            axum::Json(json!({"operations": [], "next": asked}))
        }),
    );
    let connect = Connect::new(daemon(routes).await).unwrap();
    let page = connect
        .history(Some("home & work".to_owned()), Some("x=1".to_owned()), 7)
        .await
        .unwrap();
    assert_eq!(
        page.next.as_deref(),
        Some(r#"Some("home & work") Some("x=1") Some("7")"#)
    );
    let page = connect.history(None, None, 200).await.unwrap();
    assert_eq!(page.next.as_deref(), Some(r#"None None Some("200")"#));
}

#[tokio::test]
async fn the_plans_and_repositories_are_the_daemons_lists() {
    let routes = two_plans().route(
        "/api/v1/repositories",
        get(|| async {
            axum::Json(json!({"repositories": [{
                "alias": "nas",
                "repository": "rest:https://demo:***@nas.example.lan/",
                "password": {"source": "inline"},
            }]}))
        }),
    );
    let connect = Connect::new(daemon(routes).await).unwrap();
    let plans: Vec<String> = connect
        .plans()
        .await
        .unwrap()
        .plans
        .into_iter()
        .map(|plan| plan.id)
        .collect();
    assert_eq!(plans, ["home", "photos"]);
    let repositories = connect.repositories().await.unwrap().repositories;
    assert_eq!(repositories.len(), 1);
    assert_eq!(repositories[0].alias, "nas");
}

/// A token's text, well formed; the stand-in daemons don't check it.
const TOKEN: &str = "ladderback_abcdefghijkl_c2VjcmV0LW5vdC1hLXJlYWwtb25l";

/// A daemon whose plans route answers with the `Authorization` headers the
/// request carried, as the IDs of its plans.
fn told_authorization() -> Router {
    Router::new().route(
        "/api/v1/plans",
        get(|headers: axum::http::HeaderMap| async move {
            let plans: Vec<Value> = headers
                .get_all("authorization")
                .iter()
                .map(|value| {
                    json!({"id": value.to_str().unwrap(), "repository": "nas", "scheduled": false, "next-run": null})
                })
                .collect();
            axum::Json(json!({ "plans": plans }))
        }),
    )
}

async fn authorization_sent(connect: &Connect) -> Vec<String> {
    let plans = connect.plans().await.unwrap().plans;
    plans.into_iter().map(|plan| plan.id).collect()
}

#[tokio::test]
async fn a_token_is_sent_as_bearer_with_each_request_and_none_without_one() {
    let url = daemon(told_authorization()).await;
    let without = Connect::new(url.clone()).unwrap();
    assert_eq!(authorization_sent(&without).await, Vec::<String>::new());

    let token = ApiToken::parse(&format!("  {TOKEN}\n")).expect("a token");
    let with = Connect::new(url).unwrap().with_token(token).unwrap();
    let bearer = format!("Bearer {TOKEN}");
    for _ in 0..2 {
        assert_eq!(
            authorization_sent(&with).await,
            std::slice::from_ref(&bearer)
        );
    }
}

#[test]
fn a_token_is_sent_only_over_https_or_to_this_computer() {
    let token = || ApiToken::parse(TOKEN).expect("a token");
    for (address, sent) in [
        ("https://nas.example.com", true),
        ("https://192.0.2.10:9898/ladderback/", true),
        ("http://127.0.0.1:9898", true),
        ("http://127.20.30.40:9898", true),
        ("http://[::1]:9898", true),
        ("http://localhost:9898", true),
        ("http://LOCALHOST:9898", true),
        ("http://nas.example.com:9898", false),
        ("http://192.0.2.10:9898", false),
        ("http://[2001:db8::1]:9898", false),
        ("http://0.0.0.0:9898", false),
        ("http://localhost.example.com:9898", false),
        ("http://127.0.0.1.example.com:9898", false),
    ] {
        let connect = Connect::new(DaemonUrl::parse(address).unwrap()).unwrap();
        assert_eq!(connect.with_token(token()).is_ok(), sent, "{address}");
    }
}

#[test]
fn a_token_is_its_text_without_the_spaces_around_it_and_is_never_shown() {
    let token = ApiToken::parse(&format!("\t{TOKEN}\r\n")).expect("a token");
    assert_eq!(token.0.to_str().unwrap(), format!("Bearer {TOKEN}"));
    assert!(token.0.is_sensitive());
    let shown = format!("{token:?}");
    assert!(!shown.contains("abcdefghijkl"), "{shown}");
    for text in ["", " \n", "ladderback_a b", "ladderback_\u{e9}", "a\u{0}b"] {
        assert!(ApiToken::parse(text).is_none(), "{text:?}");
    }
}

/// What a stand-in daemon was sent: each request's method, path,
/// `Content-Type`, `Authorization` and body, in order.
type Sent = Arc<std::sync::Mutex<Vec<[String; 5]>>>;

/// A daemon that records each request it is sent and answers it as
/// `answer` does for its path.
fn recording(sent: Sent, answer: fn(&str) -> axum::response::Response) -> Router {
    let record = move |method: axum::http::Method,
                       uri: axum::http::Uri,
                       headers: axum::http::HeaderMap,
                       body: String| {
        let sent = Arc::clone(&sent);
        async move {
            let header = |name: &str| {
                headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned()
            };
            sent.lock().unwrap().push([
                method.to_string(),
                uri.path().to_owned(),
                header("content-type"),
                header("authorization"),
                body,
            ]);
            answer(uri.path())
        }
    };
    Router::new().fallback(record)
}

/// A daemon that queues every run as the operation `…01`, whose operation
/// `…01` is a backup and `…02` a check, and whose cancel route answers `{}`.
/// It records each request it is sent.
fn a_daemon_that_runs_and_cancels(sent: Sent) -> Router {
    recording(sent, |path| {
        let id = |n: u8| format!("00000000-0000-7000-8000-0000000000{n:02}");
        if path == "/api/v1/operations" {
            axum::Json(json!({"operation": id(1)})).into_response()
        } else if path.ends_with("/cancel") {
            axum::Json(json!({})).into_response()
        } else if path.ends_with(&id(1)) {
            axum::Json(json!({"operation": operation(1, "home", "running"), "warnings": []}))
                .into_response()
        } else {
            let mut check = operation(2, "home", "running");
            check["kind"] = json!("check");
            axum::Json(json!({"operation": check, "warnings": []})).into_response()
        }
    })
}

/// A forget preview as the daemon answers one, with its selection.
fn forget_preview() -> Value {
    json!({
        "kind": "forget",
        "plans": [{"plan": "home", "total": 3, "remove": 0, "listed": []}],
        "left-alone": {
            "orphaned": {"groups": 0, "snapshots": 0},
            "unmanaged": {"groups": 1, "snapshots": 2},
        },
        "selection": "a-selection",
    })
}

/// A daemon that answers a test, a forget preview, a profile's preview and
/// a plan's export as the daemon's routes do, recording each request.
fn a_daemon_that_checks(sent: Sent) -> Router {
    recording(sent, |path| match path {
        "/api/v1/repositories/nas/test" => axum::Json(json!({"locations": [
            {"position": 1, "address": "/srv/nas", "outcome": "used"},
        ]}))
        .into_response(),
        "/api/v1/repositories/nas/maintenance/preview" => {
            axum::Json(forget_preview()).into_response()
        }
        "/api/v1/profiles/preview" => {
            axum::Json(json!({"repository": "nas", "plans": ["home"], "notices": []}))
                .into_response()
        }
        "/api/v1/plans/home/profile" => {
            axum::Json(json!({"text": "[repository]\n", "notices": []})).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    })
}

#[tokio::test]
async fn each_check_is_sent_to_its_route_and_reads_the_daemons_answer() {
    let sent = Sent::default();
    let connect = Connect::new(daemon(a_daemon_that_checks(Arc::clone(&sent))).await).unwrap();
    let tested = connect.test_repository("nas".to_owned()).await.unwrap();
    assert_eq!(tested.locations.len(), 1);
    let preview = connect.preview_forget("nas".to_owned()).await.unwrap();
    assert_eq!(serde_json::to_value(preview).unwrap(), forget_preview());
    let imported = connect
        .preview_import("[repository]\n".to_owned(), "nas".to_owned())
        .await
        .unwrap();
    assert_eq!(imported.plans, ["home"]);
    let exported = connect.export_profile("home".to_owned()).await.unwrap();
    assert_eq!(exported.text, "[repository]\n");
    let asked: Vec<(String, String, Value)> = sent
        .lock()
        .unwrap()
        .iter()
        .map(|[method, path, _, _, body]| {
            let body = serde_json::from_str(body).unwrap_or(Value::Null);
            (method.clone(), path.clone(), body)
        })
        .collect();
    let post = |path: &str, body: Value| ("POST".to_owned(), path.to_owned(), body);
    assert_eq!(
        asked,
        [
            post("/api/v1/repositories/nas/test", json!({})),
            post(
                "/api/v1/repositories/nas/maintenance/preview",
                json!({"kind": "forget"})
            ),
            post(
                "/api/v1/profiles/preview",
                json!({"content": "[repository]\n", "repository": "nas"})
            ),
            (
                "GET".to_owned(),
                "/api/v1/plans/home/profile".to_owned(),
                Value::Null
            ),
        ]
    );
}

#[tokio::test]
async fn a_check_whose_answer_is_lost_is_daemon_unreachable_as_it_changes_nothing() {
    let (url, _) = a_daemon_that_never_answers().await;
    let connect = Connect::new(url).unwrap();
    let unreachable = ErrorDto::new(ErrorCode::DaemonUnreachable, [], None);
    assert_eq!(
        connect.test_repository("nas".to_owned()).await,
        Err(unreachable.clone())
    );
    assert_eq!(
        connect.preview_forget("nas".to_owned()).await,
        Err(unreachable.clone())
    );
    assert_eq!(
        connect
            .preview_import("[repository]\n".to_owned(), "nas".to_owned())
            .await,
        Err(unreachable)
    );
}

#[tokio::test]
async fn a_run_is_posted_as_json_with_the_token_and_returns_the_operation_queued() {
    let sent = Sent::default();
    let url = daemon(a_daemon_that_runs_and_cancels(Arc::clone(&sent))).await;
    let token = ApiToken::parse(TOKEN).expect("a token");
    let connect = Connect::new(url).unwrap().with_token(token).unwrap();
    let operation = connect.run("home".to_owned()).await.unwrap();
    assert_eq!(operation, "00000000-0000-7000-8000-000000000001");
    let body = json!({"plan": "home"}).to_string();
    assert_eq!(
        *sent.lock().unwrap(),
        [[
            "POST".to_owned(),
            "/api/v1/operations".to_owned(),
            "application/json".to_owned(),
            format!("Bearer {TOKEN}"),
            body,
        ]]
    );
}

#[tokio::test]
async fn a_backup_is_read_then_its_cancel_posted() {
    let sent = Sent::default();
    let connect =
        Connect::new(daemon(a_daemon_that_runs_and_cancels(Arc::clone(&sent))).await).unwrap();
    let id = "00000000-0000-7000-8000-000000000001";
    connect.cancel(id.to_owned()).await.unwrap();
    let asked: Vec<(String, String, String)> = sent
        .lock()
        .unwrap()
        .iter()
        .map(|[method, path, _, _, body]| (method.clone(), path.clone(), body.clone()))
        .collect();
    assert_eq!(
        asked,
        [
            (
                "GET".to_owned(),
                format!("/api/v1/operations/{id}"),
                String::new()
            ),
            (
                "POST".to_owned(),
                format!("/api/v1/operations/{id}/cancel"),
                "{}".to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn a_cancel_of_anything_but_a_backup_is_refused_without_posting_it() {
    let sent = Sent::default();
    let connect =
        Connect::new(daemon(a_daemon_that_runs_and_cancels(Arc::clone(&sent))).await).unwrap();
    let refused = connect
        .cancel("00000000-0000-7000-8000-000000000002".to_owned())
        .await;
    assert_eq!(refused, Err(not_cancellable()));
    let methods: Vec<String> = sent
        .lock()
        .unwrap()
        .iter()
        .map(|[method, ..]| method.clone())
        .collect();
    assert_eq!(methods, ["GET"]);
}

#[tokio::test]
async fn a_run_the_daemon_refuses_is_its_own_code_and_arguments() {
    let routes = Router::new().route(
        "/api/v1/operations",
        axum::routing::post(|| async {
            (
                StatusCode::NOT_FOUND,
                axum::Json(json!({"code": "plan-unknown", "args": {"plan": {"type": "text", "value": "home"}}})),
            )
        }),
    );
    let connect = Connect::new(daemon(routes).await).unwrap();
    let refused = connect.run("home".to_owned()).await;
    assert_eq!(
        refused,
        Err(ErrorDto::new(
            ErrorCode::PlanUnknown,
            [("plan", api::MsgArg::Text("home".to_owned()))],
            None
        ))
    );
}

#[tokio::test]
async fn a_run_with_nothing_listening_is_daemon_unreachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let connect = Connect::new(DaemonUrl::parse(&format!("http://{addr}")).unwrap()).unwrap();
    let refused = connect.run("home".to_owned()).await.unwrap_err();
    assert_eq!(refused.code, ErrorCode::DaemonUnreachable);
}

/// A daemon that reads each request whole and closes the connection without
/// answering, counting the requests it was sent.
async fn a_daemon_that_never_answers() -> (DaemonUrl, Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&sent);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            // Headers and the short body arrive together; then hang up.
            while !request.ends_with(b"}") {
                match stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => request.extend_from_slice(&buffer[..read]),
                }
            }
            drop(stream);
        }
    });
    (DaemonUrl::parse(&format!("http://{addr}")).unwrap(), sent)
}

#[tokio::test]
async fn a_run_whose_answer_is_lost_is_daemon_answer_lost_and_never_sent_again() {
    let (url, sent) = a_daemon_that_never_answers().await;
    let connect = Connect::new(url).unwrap();
    let refused = connect.run("home".to_owned()).await;
    assert_eq!(
        refused,
        Err(ErrorDto::new(ErrorCode::DaemonAnswerLost, [], None))
    );
    assert_eq!(sent.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_run_answered_with_what_isnt_the_apis_is_daemon_answer_lost() {
    for (status, body) in [
        (StatusCode::OK, "<html>Welcome to nginx</html>"),
        (StatusCode::BAD_GATEWAY, "<html>Bad gateway</html>"),
    ] {
        let routes = Router::new().route(
            "/api/v1/operations",
            axum::routing::post(move || async move { (status, body) }),
        );
        let connect = Connect::new(daemon(routes).await).unwrap();
        let refused = connect.run("home".to_owned()).await.unwrap_err();
        assert_eq!(refused.code, ErrorCode::DaemonAnswerLost, "{status}");
    }
}
