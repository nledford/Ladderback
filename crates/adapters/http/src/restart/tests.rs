//! The restart route with sign-in off; with sign-in on, `api_tokens/tests.rs`
//! asks it in a session and with a token.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use futures::FutureExt;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::{Restart, RestartAsker, Work};
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, router};

const HOST: &str = "127.0.0.1:8080";

/// Operations a test sets the running count of, and whether they are held.
#[derive(Default)]
struct Counted {
    running: AtomicUsize,
    held: AtomicBool,
}

impl Work for Counted {
    fn hold(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    fn release(&self) {
        self.held.store(false, Ordering::SeqCst);
    }

    fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }
}

/// A daemon run by a service manager (`offered`), with `running` operations.
fn daemon(offered: bool, running: usize) -> (Restart, Arc<Counted>) {
    let work = Arc::new(Counted::default());
    work.running.store(running, Ordering::SeqCst);
    (
        Restart::new(offered, Arc::clone(&work) as Arc<dyn Work>),
        work,
    )
}

async fn send(restart: &Restart, request: Request) -> (StatusCode, serde_json::Value) {
    let mut state = test_state();
    state.restart = restart.clone();
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn get() -> Request {
    Request::get("/api/v1/restart")
        .header("host", HOST)
        .body(Body::empty())
        .unwrap()
}

fn post(body: &str) -> Request {
    Request::post("/api/v1/restart")
        .header("host", HOST)
        .header("origin", format!("http://{HOST}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// A cancel, sent as the page sends every change: JSON, here an empty
/// object, as the token routes' `DELETE` takes.
fn delete() -> Request {
    Request::delete("/api/v1/restart")
        .header("host", HOST)
        .header("origin", format!("http://{HOST}"))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap()
}

/// Whether the restart is due now, with who asked.
fn due_now(restart: &Restart) -> Option<RestartAsker> {
    restart.due().now_or_never()
}

#[tokio::test]
async fn a_daemon_no_service_manager_runs_offers_no_restart_and_refuses_one() {
    let (restart, work) = daemon(false, 0);
    let (status, body) = send(&restart, get()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "offered": false, "pending": false, "running": 0 })
    );
    for asked in [r#"{"when":"now"}"#, r#"{"when":"finished"}"#, "{}"] {
        let (status, body) = send(&restart, post(asked)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{asked}");
        assert_eq!(body["code"], "restart-unavailable", "{asked}");
    }
    assert!(!work.held.load(Ordering::SeqCst), "the queue was held");
    assert_eq!(due_now(&restart), None, "a refused restart is due");
}

#[tokio::test]
async fn a_restart_now_is_due_at_once_whatever_runs() {
    let (restart, _) = daemon(true, 2);
    assert_eq!(due_now(&restart), None, "a restart nobody asked for is due");
    let (status, body) = send(&restart, post(r#"{"when":"now"}"#)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body,
        serde_json::json!({ "offered": true, "pending": false, "running": 2 })
    );
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
}

// Pending until the daemon dies, whatever runs: Settings keeps saying a
// restart waits until it sees the daemon go down.
#[tokio::test]
async fn a_restart_when_they_finish_with_none_running_is_due_at_once_and_pending_until_then() {
    let (restart, work) = daemon(true, 0);
    let (status, body) = send(&restart, post(r#"{"when":"finished"}"#)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["pending"], true);
    assert!(work.held.load(Ordering::SeqCst), "the queue wasn't held");
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
}

#[tokio::test]
async fn a_restart_when_they_finish_waits_for_the_running_operations_and_holds_the_queue() {
    let (restart, work) = daemon(true, 1);
    let (status, body) = send(&restart, post(r#"{"when":"finished"}"#)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body,
        serde_json::json!({ "offered": true, "pending": true, "running": 1 })
    );
    assert!(work.held.load(Ordering::SeqCst), "the queue wasn't held");
    let (_, read) = send(&restart, get()).await;
    assert_eq!(read["pending"], true, "Settings doesn't say it waits");
    let due = tokio::spawn({
        let restart = restart.clone();
        async move { restart.due().await }
    });
    // Past a recheck, which finds one still running.
    tokio::time::sleep(super::IDLE_RECHECK + Duration::from_millis(200)).await;
    assert!(!due.is_finished(), "the restart didn't wait");
    work.running.store(0, Ordering::SeqCst);
    // Within a recheck, and some to spare.
    let asker = tokio::time::timeout(super::IDLE_RECHECK * 3, due)
        .await
        .expect("the restart came once nothing ran")
        .unwrap();
    assert_eq!(asker, RestartAsker::SignInOff);
}

#[tokio::test]
async fn a_restart_now_cuts_short_one_that_waits_and_a_second_wait_changes_nothing() {
    let (restart, _) = daemon(true, 1);
    send(&restart, post(r#"{"when":"finished"}"#)).await;
    let (status, _) = send(&restart, post(r#"{"when":"finished"}"#)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "a second request is refused");
    assert_eq!(due_now(&restart), None);
    send(&restart, post(r#"{"when":"now"}"#)).await;
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
    // A request to wait after one for now leaves it due.
    send(&restart, post(r#"{"when":"finished"}"#)).await;
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
}

#[tokio::test]
async fn a_request_that_isnt_a_restart_is_refused() {
    let (restart, work) = daemon(true, 0);
    for body in [
        "{}",
        r#"{"when":"later"}"#,
        r#"{"when":"now","x":1}"#,
        "now",
    ] {
        let (status, refusal) = send(&restart, post(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(refusal["code"], "invalid-body", "{body}");
    }
    assert!(!work.held.load(Ordering::SeqCst));
    assert_eq!(due_now(&restart), None);
}

#[tokio::test]
async fn a_canceled_restart_lets_go_of_the_hold_and_never_comes() {
    let (restart, work) = daemon(true, 1);
    send(&restart, post(r#"{"when":"finished"}"#)).await;
    let due = tokio::spawn({
        let restart = restart.clone();
        async move { restart.due().await }
    });
    let (status, body) = send(&restart, delete()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "offered": true, "pending": false, "running": 1 })
    );
    assert!(!work.held.load(Ordering::SeqCst), "the hold stayed");
    work.running.store(0, Ordering::SeqCst);
    // Past a recheck, which would find nothing running.
    tokio::time::sleep(super::IDLE_RECHECK * 2).await;
    assert!(!due.is_finished(), "the cancelled restart came");
    // Canceled twice, or with none asked for: nothing to cancel.
    let (status, _) = send(&restart, delete()).await;
    assert_eq!(status, StatusCode::OK);
    // Asked for again, it holds the work again.
    work.running.store(1, Ordering::SeqCst);
    send(&restart, post(r#"{"when":"finished"}"#)).await;
    assert!(
        work.held.load(Ordering::SeqCst),
        "the second ask didn't hold"
    );
}

#[tokio::test]
async fn a_restart_asked_for_now_cannot_be_canceled() {
    let (restart, work) = daemon(true, 1);
    send(&restart, post(r#"{"when":"finished"}"#)).await;
    send(&restart, post(r#"{"when":"now"}"#)).await;
    let (status, _) = send(&restart, delete()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(work.held.load(Ordering::SeqCst), "the hold was let go");
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
}

// Once due, the daemon is on its way down: a cancel that comes before it
// stops is too late, so it neither lets go of the hold (a queued backup
// would start only to be cut off) nor tells Settings the restart is off.
#[tokio::test]
async fn a_restart_that_has_come_is_not_canceled() {
    let (restart, work) = daemon(true, 0);
    send(&restart, post(r#"{"when":"finished"}"#)).await;
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
    let (status, body) = send(&restart, delete()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["pending"], true, "Settings says it was cancelled");
    assert!(work.held.load(Ordering::SeqCst), "the hold was let go");
    assert_eq!(due_now(&restart), Some(RestartAsker::SignInOff));
}

#[tokio::test]
async fn a_daemon_no_service_manager_runs_refuses_a_cancel() {
    let (restart, _) = daemon(false, 0);
    let (status, body) = send(&restart, delete()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "restart-unavailable");
}
