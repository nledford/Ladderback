use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Body;
use axum::http::{Method, Request, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, router};

const OPERATION: &str = "018f0000-0000-7000-8000-000000000001";
const FOLDER: &str = "/home/demo/ladderback-restore/laptop-3f2a1b4c-20270115-080000";

/// A start that answered, naming `folder` when it makes one.
fn started(folder: Option<&str>) -> Result<api::RestoreStartedDto, RestoreRefusal> {
    Ok(api::RestoreStartedDto {
        operation: OPERATION.to_owned(),
        folder: folder.map(str::to_owned),
    })
}

/// The snapshot the requests name: 64 hex digits.
fn snapshot() -> String {
    "3f2a1b4c".repeat(8)
}

const HOME: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), 51000);

/// Answers each route with its scripted answer, and remembers what it was
/// asked, the confirmation included.
struct Scripted {
    preview: Result<api::RestorePreviewDto, RestoreRefusal>,
    start: Result<api::RestoreStartedDto, RestoreRefusal>,
    asked: Mutex<Vec<(String, String)>>,
}

impl Scripted {
    fn answering(start: Result<api::RestoreStartedDto, RestoreRefusal>) -> Arc<Self> {
        Arc::new(Self {
            preview: Err(RestoreRefusal::SnapshotUnknown),
            start,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<(String, String)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Restores for Scripted {
    async fn preview(
        &self,
        alias: String,
        snapshot: SnapshotId,
        entry: FolderPath,
    ) -> Result<api::RestorePreviewDto, RestoreRefusal> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((alias, format!("{}:{entry}", snapshot.as_str())));
        self.preview.clone()
    }

    async fn start(
        &self,
        alias: String,
        start: RestoreStart,
    ) -> Result<api::RestoreStartedDto, RestoreRefusal> {
        let RestoreStart {
            snapshot,
            entry,
            destination,
            address,
            confirmation,
        } = start;
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((
                alias,
                format!(
                    "{}:{entry}:{destination:?}:{address}:{confirmation}",
                    snapshot.as_str()
                ),
            ));
        self.start.clone()
    }
}

/// A request from `HOME`, as the listener gives every request its
/// connection's address.
fn post(path: &str, body: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, "127.0.0.1:8080")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(crate::PeerAddr(HOME)));
    request
}

fn start_path(snapshot: &str) -> String {
    format!("/api/v1/repositories/demo/snapshots/{snapshot}/restore")
}

fn preview_path(snapshot: &str) -> String {
    format!("/api/v1/repositories/demo/snapshots/{snapshot}/restore/preview")
}

const NEW_FOLDER: &str =
    r#"{"path":"home/docs","destination":"new-folder","confirmation":"canary-admin-password"}"#;

async fn send(restores: Arc<Scripted>, request: Request<Body>) -> (StatusCode, Response) {
    let mut state = test_state();
    state.restores = restores;
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(request)
        .await
        .unwrap();
    (response.status(), response)
}

async fn body_of(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn a_start_reads_the_request_and_answers_the_operation_and_its_folder() {
    let restores = Scripted::answering(started(Some(FOLDER)));
    let (status, response) = send(
        Arc::clone(&restores),
        post(&start_path(&snapshot()), NEW_FOLDER),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body_of(response).await,
        serde_json::json!({ "operation": OPERATION, "folder": FOLDER })
    );
    assert_eq!(
        restores.asked(),
        [(
            "demo".to_owned(),
            format!(
                "{}:home/docs:NewFolder:192.0.2.7:canary-admin-password",
                snapshot()
            )
        )]
    );
}

#[tokio::test]
async fn an_in_place_start_asks_for_in_place_and_names_no_folder() {
    let restores = Scripted::answering(started(None));
    let body = r#"{"path":"","destination":"in-place","confirmation":"3f2a1b4c"}"#;
    let (status, response) =
        send(Arc::clone(&restores), post(&start_path(&snapshot()), body)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body_of(response).await,
        serde_json::json!({ "operation": OPERATION })
    );
    let asked = restores.asked();
    assert!(
        asked
            .first()
            .is_some_and(|(_, what)| what.contains(":InPlace:")),
        "{asked:?}"
    );
}

#[tokio::test]
async fn each_refusal_is_answered_with_its_code_and_status() {
    for (refusal, status, code) in [
        (
            RestoreRefusal::StepUpFailed,
            StatusCode::FORBIDDEN,
            "step-up-failed",
        ),
        (
            RestoreRefusal::StepUpUnavailable("Permission denied".to_owned()),
            StatusCode::SERVICE_UNAVAILABLE,
            "sign-in-unavailable",
        ),
        (
            RestoreRefusal::InPlaceNotHere,
            StatusCode::FORBIDDEN,
            "restore-in-place-not-here",
        ),
        (
            RestoreRefusal::HubNotAllowed,
            StatusCode::FORBIDDEN,
            "restore-hub-not-allowed",
        ),
        (
            RestoreRefusal::NoRestoreFolder,
            StatusCode::CONFLICT,
            "restore-folder-unset",
        ),
        (
            RestoreRefusal::SnapshotUnknown,
            StatusCode::NOT_FOUND,
            "snapshot-unknown",
        ),
        (
            RestoreRefusal::Repository(MaintenanceRefusal::UnknownRepository),
            StatusCode::NOT_FOUND,
            "repository-unknown",
        ),
    ] {
        let restores = Scripted::answering(Err(refusal.clone()));
        let (answered, response) = send(restores, post(&start_path(&snapshot()), NEW_FOLDER)).await;
        assert_eq!(answered, status, "{refusal:?}");
        let body = body_of(response).await;
        assert_eq!(body["code"], code, "{refusal:?}");
        // The platform's words stay in the daemon's log.
        assert!(!body.to_string().contains("Permission denied"), "{body}");
    }
}

#[tokio::test]
async fn a_step_up_during_a_wait_is_refused_as_a_sign_in_is() {
    let restores = Scripted::answering(Err(RestoreRefusal::StepUpWait { seconds: 4 }));
    let (status, response) = send(restores, post(&start_path(&snapshot()), NEW_FOLDER)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "4");
    let body = body_of(response).await;
    assert_eq!(body["code"], "sign-in-wait");
    assert_eq!(body["args"]["seconds"]["value"], 4);
}

#[tokio::test]
async fn a_second_restore_names_the_one_that_runs() {
    let restores = Scripted::answering(Err(RestoreRefusal::Running {
        operation: OPERATION.to_owned(),
    }));
    let (status, response) = send(restores, post(&start_path(&snapshot()), NEW_FOLDER)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let body = body_of(response).await;
    assert_eq!(body["code"], "restore-running");
    assert_eq!(body["args"]["operation"]["value"], OPERATION);
}

#[tokio::test]
async fn what_no_restore_could_name_is_refused_unasked() {
    let restores = Scripted::answering(started(Some(FOLDER)));
    for (path, body, status, code) in [
        (
            start_path("not-a-snapshot"),
            NEW_FOLDER,
            StatusCode::NOT_FOUND,
            "snapshot-unknown",
        ),
        (
            start_path(&snapshot()),
            r#"{"path":"home//docs","destination":"new-folder","confirmation":"x"}"#,
            StatusCode::NOT_FOUND,
            "folder-unknown",
        ),
        (
            start_path(&snapshot()),
            r#"{"path":"home","destination":"elsewhere","confirmation":"x"}"#,
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
        (
            start_path(&snapshot()),
            r#"{"path":"home","destination":"new-folder"}"#,
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
        (
            preview_path(&snapshot()),
            r#"{"path":"home","to":"/etc"}"#,
            StatusCode::BAD_REQUEST,
            "invalid-body",
        ),
    ] {
        let (answered, response) = send(Arc::clone(&restores), post(&path, body)).await;
        assert_eq!(answered, status, "{path} {body}");
        assert_eq!(body_of(response).await["code"], code, "{path} {body}");
    }
    assert!(restores.asked().is_empty());
}

#[tokio::test]
async fn a_start_without_its_connection_s_address_is_refused() {
    let restores = Scripted::answering(started(Some(FOLDER)));
    let mut request = post(&start_path(&snapshot()), NEW_FOLDER);
    request
        .extensions_mut()
        .remove::<axum::extract::ConnectInfo<crate::PeerAddr>>();
    let (status, response) = send(Arc::clone(&restores), request).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_of(response).await["code"], "sign-in-unavailable");
    assert!(restores.asked().is_empty());
}

#[tokio::test]
async fn a_preview_answers_what_the_dry_run_found() {
    let found = api::RestorePreviewDto {
        added: 1,
        overwritten: 2,
        unchanged: 3,
        kept: 4,
        bytes: 5,
    };
    let restores = Arc::new(Scripted {
        preview: Ok(found),
        start: Err(RestoreRefusal::StepUpFailed),
        asked: Mutex::new(Vec::new()),
    });
    let (status, response) = send(
        Arc::clone(&restores),
        post(&preview_path(&snapshot()), r#"{"path":"home/docs"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body_of(response).await,
        serde_json::json!({ "added": 1, "overwritten": 2, "unchanged": 3, "kept": 4, "bytes": 5 })
    );
    assert_eq!(
        restores.asked(),
        [("demo".to_owned(), format!("{}:home/docs", snapshot()))]
    );
}

#[test]
fn a_restore_start_never_shows_its_confirmation() {
    let start = RestoreStart {
        snapshot: SnapshotId::parse(&snapshot()).unwrap(),
        entry: FolderPath::top(),
        destination: Destination::NewFolder,
        address: HOME.ip(),
        confirmation: "canary-admin-password".to_owned(),
    };
    let shown = format!("{start:?}");
    assert!(!shown.contains("canary-admin-password"), "{shown}");
}
