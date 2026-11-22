//! A handler that panics (technical design §13.5): the client gets the API's
//! error envelope, the log one line that holds nothing the request or the
//! panic carried (I4), and the daemon goes on serving.

use std::cell::RefCell;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// What the request and the panic carry, which neither the answer nor the
/// log may show.
const CANARY: &str = "canary-not-a-real-secret-5d1e";

/// Routes that sit where the router's own do: one that panics with what its
/// request carried, and one that answers.
fn panicking_app() -> Router {
    let routes = Router::new()
        .route(
            "/api/v1/boom",
            axum::routing::post(|body: String| async move {
                panic!("the handler broke on {body}");
            }),
        )
        .route("/api/v1/fine", axum::routing::get(|| async { "fine" }));
    outer_layers(routes)
}

type Logged = (String, log::Level, String);

thread_local! {
    /// The lines logged on this thread: a `#[tokio::test]` runs the router
    /// on the test's own thread, so tests that run at once never see each
    /// other's lines.
    static LOGGED: RefCell<Vec<Logged>> = const { RefCell::new(Vec::new()) };
}

struct CapturingLogger;

impl log::Log for CapturingLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        LOGGED.with_borrow_mut(|lines| {
            lines.push((
                record.target().to_owned(),
                record.level(),
                record.args().to_string(),
            ));
        });
    }

    fn flush(&self) {}
}

fn capture_the_log() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        log::set_logger(&CapturingLogger).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    LOGGED.with_borrow_mut(Vec::clear);
}

/// What this crate logged on this thread since the capture began.
fn logged_here() -> Vec<(log::Level, String)> {
    LOGGED
        .with_borrow_mut(std::mem::take)
        .into_iter()
        .filter(|(target, _, _)| target.starts_with("http_adapter"))
        .map(|(_, level, line)| (level, line))
        .collect()
}

#[tokio::test]
async fn a_handler_that_panics_answers_500_with_the_error_envelope_and_one_log_line() {
    capture_the_log();
    let response = panicking_app()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/boom")
                .body(Body::from(CANARY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff",
        "the answer carries the security headers"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(content_type(&response), "application/json");
    let answer = body(response).await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&answer).unwrap(),
        serde_json::json!({"code": "internal-error"})
    );
    let logged = logged_here();
    assert_eq!(logged.len(), 1, "{logged:?}");
    let (level, line) = &logged[0];
    assert_eq!(*level, log::Level::Error);
    assert!(!line.contains(CANARY), "the log shows the request: {line}");
    assert!(!line.contains("broke"), "the log shows the panic: {line}");
}

/// Sends `request` on a connection of its own and reads the answer to its
/// end.
async fn exchange(at: std::net::SocketAddr, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(at).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_string(&mut answer),
    )
    .await
    .unwrap()
    .unwrap();
    answer
}

#[tokio::test]
async fn the_daemon_keeps_serving_after_a_handler_panics() {
    let loopback = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
    let listener = GatedListener::bind(loopback).await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(crate::serve_connections(
        listener,
        panicking_app(),
        std::future::pending(),
    ));
    let broke = exchange(
        at,
        &format!(
            "POST /api/v1/boom HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{CANARY}",
            CANARY.len()
        ),
    )
    .await;
    assert!(broke.starts_with("HTTP/1.1 500 "), "{broke}");
    assert!(
        !broke.contains(CANARY),
        "the answer shows the request: {broke}"
    );
    let next = exchange(
        at,
        "GET /api/v1/fine HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(next.starts_with("HTTP/1.1 200 "), "{next}");
    assert!(next.ends_with("fine"), "{next}");
}
