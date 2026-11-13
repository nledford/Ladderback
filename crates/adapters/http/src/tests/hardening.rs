//! Body limits and security response headers (technical design §10.1),
//! checked on the router as a client sees it.

use std::assert_matches;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::{SinkExt, StreamExt, stream};

use super::*;

/// The most a request to the API may carry.
const API_LIMIT: usize = 128 * 1024;

/// A body the operations route reads, padded to `size` bytes.
fn plan_body(size: usize) -> String {
    let frame = r#"{"plan":""}"#.len();
    format!(r#"{{"plan":"{}"}}"#, "a".repeat(size - frame))
}

fn too_large(limit: u64) -> serde_json::Value {
    serde_json::json!({
        "code": "body-too-large",
        "args": {"limit": {"type": "bytes", "value": limit}},
    })
}

async fn json(response: Response) -> serde_json::Value {
    serde_json::from_str(&body(response).await).unwrap()
}

#[tokio::test]
async fn a_body_of_exactly_the_limit_is_read() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        &plan_body(API_LIMIT),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn a_body_one_byte_over_the_limit_is_refused_with_a_typed_code() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        &plan_body(API_LIMIT + 1),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(response).await, too_large(API_LIMIT as u64));
}

#[tokio::test]
async fn every_route_that_reads_a_body_refuses_one_over_the_limit() {
    let oversize = plan_body(API_LIMIT + 1);
    for path in [
        "/api/v1/operations",
        "/api/v1/operations/some-id/cancel",
        "/api/v1/repositories",
        "/api/v1/repositories/_test",
        "/api/v1/repositories/_init",
        "/api/v1/repositories/demo/maintenance",
    ] {
        let response = request(ui(), Method::POST, path, &oversize).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE, "{path}");
        assert_eq!(json(response).await, too_large(API_LIMIT as u64), "{path}");
    }
}

#[tokio::test]
async fn a_body_without_a_declared_length_is_bounded_too() {
    // A chunked body has no Content-Length to check up front.
    let chunks = (0..=API_LIMIT / 1024)
        .map(|_| Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![b'a'; 1024])));
    let response = router(ui(), RequestChecks::default(), state())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/operations")
                .header(header::HOST, "localhost:9898")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from_stream(stream::iter(chunks)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(response).await, too_large(API_LIMIT as u64));
}

#[tokio::test]
async fn the_web_ui_takes_no_body() {
    let response = request(ui(), Method::GET, "/", "x").await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(response).await, too_large(0));
    let empty = request(ui(), Method::GET, "/", "").await;
    assert_eq!(empty.status(), StatusCode::OK);
}

/// What every response says a browser may do with it, whatever answered.
fn assert_secured(response: &Response, what: &str) {
    let headers = response.headers();
    assert_eq!(headers["x-content-type-options"], "nosniff", "{what}");
    assert_eq!(headers["referrer-policy"], "no-referrer", "{what}");
    let policy = headers["content-security-policy"].to_str().unwrap();
    for directive in [
        "default-src 'self'",
        "script-src 'self' 'wasm-unsafe-eval'",
        "frame-ancestors 'none'",
    ] {
        assert!(policy.contains(directive), "{what}: {policy}");
    }
    assert!(!headers.contains_key("strict-transport-security"), "{what}");
}

#[tokio::test]
async fn every_kind_of_response_carries_the_security_headers() {
    let oversize = plan_body(API_LIMIT + 1);
    for (what, response) in [
        ("the entry page", get("/").await),
        ("an asset", get("/web-1a2b.js").await),
        ("a missing file", get("/missing.js").await),
        ("an API answer", get("/api/v1/plans").await),
        ("an API refusal", get("/api/v1/operations/nope").await),
        (
            "a wrong method",
            request(ui(), Method::DELETE, "/api/v1/plans", "").await,
        ),
        (
            "a body over the limit",
            request(ui(), Method::POST, "/api/v1/operations", &oversize).await,
        ),
    ] {
        assert_secured(&response, what);
    }
}

#[tokio::test]
async fn a_request_the_checks_refuse_gets_the_security_headers() {
    let response = router(ui(), RequestChecks::default(), state())
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::HOST, "evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_secured(&response, "a refusal of the request checks");
}

#[tokio::test]
async fn the_entry_page_names_its_own_inline_script_by_hash_and_nothing_else_does() {
    let assets: Arc<dyn AssetSource> = Arc::new(Files(HashMap::from([
        (
            "index.html",
            "<!doctype html><script type=\"module\">start()</script>",
        ),
        ("web-1a2b.js", "export default 1;"),
    ])));
    // The script's hash, as a browser computes it (the base64 of its SHA-256).
    let hash = "'sha256-DIm7WJS6ZKDYe5qFLPy+h4JFI9Bol5QmYC57mt3Fb00='";
    for path in ["/", "/machines/laptop"] {
        let response = request(Arc::clone(&assets), Method::GET, path, "").await;
        let policy = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(
            policy.contains(&format!("script-src 'self' 'wasm-unsafe-eval' {hash};")),
            "{path}: {policy}"
        );
        assert!(!policy.contains("unsafe-inline"), "{path}: {policy}");
    }
    for path in ["/web-1a2b.js", "/api/v1/plans", "/missing.js"] {
        let response = request(Arc::clone(&assets), Method::GET, path, "").await;
        let policy = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(!policy.contains("sha256-"), "{path}: {policy}");
    }
}

#[tokio::test]
async fn a_declared_length_over_the_limit_is_refused_without_reading_the_body() {
    // A client that sent `Expect: 100-continue` sends its body only once the
    // daemon polls it, so a body that was read is a body that was asked for.
    let polled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&polled);
    let body = Body::from_stream(stream::poll_fn(move |_| {
        flag.store(true, Ordering::SeqCst);
        std::task::Poll::Ready(None::<Result<axum::body::Bytes, std::io::Error>>)
    }));
    let response = router(ui(), RequestChecks::default(), state())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/operations")
                .header(header::HOST, "localhost:9898")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_LENGTH, API_LIMIT + 1)
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(response).await, too_large(API_LIMIT as u64));
    assert!(!polled.load(Ordering::SeqCst), "the body was read");
}

#[tokio::test]
async fn a_message_from_the_events_client_is_bounded() {
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::header::ORIGIN;

    let (addr, _events) = serving().await;
    let mut request = format!("ws://{addr}/api/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ORIGIN, format!("http://{addr}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    // What the UI sends: a ping, which a control frame keeps to 125 bytes.
    let ping = vec![b'p'; 125];
    socket
        .send(Message::Ping(ping.clone().into()))
        .await
        .unwrap();
    let answer = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next()).await;
    assert_matches!(
        answer,
        Ok(Some(Ok(Message::Pong(ref payload)))) if payload.as_ref() == ping.as_slice()
    );
    // Anything larger is nothing the UI says: the daemon ends the connection
    // rather than hold a message of whatever size in memory.
    let _ = socket
        .send(Message::Text("a".repeat(64 * 1024).into()))
        .await;
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the connection stayed open");
}
