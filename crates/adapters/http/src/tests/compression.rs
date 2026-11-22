//! The web UI's precompressed copies (technical design §10.1), served as a
//! client sees them: the copy the request accepts, or the original.

use super::*;

const PAGE: &str = "<!doctype html><title>Ladderback</title><script>start()</script>";
const MODULE: &str = "\0asm the page's module, long enough to compress";
const MODULE_PATH: &str = "/web-1a2b3c4d_bg.wasm";

/// A UI whose page and module have both copies, a script only its Brotli
/// copy (gzip didn't make it smaller), and an icon none (compression didn't
/// make it smaller).
fn compressed_ui() -> Arc<dyn AssetSource> {
    Arc::new(Files(HashMap::from([
        ("index.html", PAGE),
        ("index.html.br", "brotli copy of the page"),
        ("index.html.gz", "gzip copy of the page"),
        ("web-1a2b3c4d_bg.wasm", MODULE),
        ("web-1a2b3c4d_bg.wasm.br", "brotli copy of the module"),
        ("web-1a2b3c4d_bg.wasm.gz", "gzip copy of the module"),
        ("web-1a2b3c4d.js", "export default 1;"),
        ("web-1a2b3c4d.js.br", "brotli copy of the script"),
        ("icon-1a2b3c4d.png", "\u{89}PNG"),
    ])))
}

/// The answer to `method path` from a client whose `Accept-Encoding` is
/// `accepts`, or that sends none.
async fn fetched(method: Method, path: &str, accepts: Option<&str>) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "localhost:9898");
    if let Some(accepts) = accepts {
        request = request.header(header::ACCEPT_ENCODING, accepts);
    }
    router(compressed_ui(), RequestChecks::default(), state())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn header_of(response: &Response, name: header::HeaderName) -> Option<&str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
}

/// Asserts `response` is the copy `body` of the module in `encoding`, with
/// the module's type and caching and its own length.
async fn assert_module_copy(response: Response, encoding: Option<&str>, body_text: &str) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_ENCODING), encoding);
    assert_eq!(content_type(&response), "application/wasm");
    assert_eq!(
        header_of(&response, header::CONTENT_LENGTH),
        Some(body_text.len().to_string().as_str())
    );
    assert_eq!(
        header_of(&response, header::VARY),
        Some("accept-encoding"),
        "a cache must keep one answer per Accept-Encoding"
    );
    assert_eq!(
        header_of(&response, header::CACHE_CONTROL),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(body(response).await, body_text);
}

#[tokio::test]
async fn a_client_that_accepts_brotli_gets_the_brotli_copy() {
    let response = fetched(Method::GET, MODULE_PATH, Some("gzip, deflate, br")).await;
    assert_module_copy(response, Some("br"), "brotli copy of the module").await;
}

#[tokio::test]
async fn a_client_that_accepts_only_gzip_gets_the_gzip_copy() {
    let response = fetched(Method::GET, MODULE_PATH, Some("gzip")).await;
    assert_module_copy(response, Some("gzip"), "gzip copy of the module").await;
    let refused = fetched(Method::GET, MODULE_PATH, Some("br;q=0, gzip")).await;
    assert_module_copy(refused, Some("gzip"), "gzip copy of the module").await;
}

#[tokio::test]
async fn a_client_that_accepts_neither_gets_the_original() {
    for accepts in [None, Some("identity"), Some("deflate"), Some("*;q=0")] {
        let response = fetched(Method::GET, MODULE_PATH, accepts).await;
        assert_module_copy(response, None, MODULE).await;
    }
}

#[tokio::test]
async fn a_client_whose_coding_has_no_copy_gets_the_original_and_the_vary() {
    let response = fetched(Method::GET, "/web-1a2b3c4d.js", Some("gzip")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_ENCODING), None);
    assert_eq!(header_of(&response, header::VARY), Some("accept-encoding"));
    assert_eq!(body(response).await, "export default 1;");
}

#[tokio::test]
async fn the_page_s_copy_keeps_the_type_and_the_policy_of_the_page() {
    for path in ["/", "/plans"] {
        let response = fetched(Method::GET, path, Some("br")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header_of(&response, header::CONTENT_ENCODING), Some("br"));
        assert_eq!(content_type(&response), "text/html; charset=utf-8");
        // The policy names the page's own script by the hash of the page
        // itself, never of its copy.
        assert_eq!(
            response.headers()[header::CONTENT_SECURITY_POLICY],
            response_headers::page_policy(PAGE.as_bytes())
        );
        assert_eq!(header_of(&response, header::VARY), Some("accept-encoding"));
        assert_eq!(body(response).await, "brotli copy of the page");
    }
}

#[tokio::test]
async fn a_head_request_gets_the_copy_s_headers() {
    let response = fetched(Method::HEAD, MODULE_PATH, Some("br")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_ENCODING), Some("br"));
    assert_eq!(
        header_of(&response, header::CONTENT_LENGTH),
        Some("brotli copy of the module".len().to_string().as_str())
    );
}

#[tokio::test]
async fn a_file_the_build_kept_no_copy_of_is_sent_as_it_is_without_a_vary() {
    let response = fetched(Method::GET, "/icon-1a2b3c4d.png", Some("br, gzip")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_ENCODING), None);
    assert_eq!(header_of(&response, header::VARY), None);
    assert_eq!(body(response).await, "\u{89}PNG");
}

#[tokio::test]
async fn no_answer_of_the_api_is_compressed_whatever_the_request_accepts() {
    for path in ["/api/v1/plans", "/api/v1/session", "/api/v1/nope", "/api"] {
        let response = fetched(Method::GET, path, Some("br, gzip, *")).await;
        assert_eq!(
            header_of(&response, header::CONTENT_ENCODING),
            None,
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_events_stream_is_never_compressed() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::header::{
        ACCEPT_ENCODING, ORIGIN, SEC_WEBSOCKET_EXTENSIONS,
    };

    let (addr, _events) = serving().await;
    let mut request = format!("ws://{addr}/api/v1/events")
        .into_client_request()
        .unwrap();
    let headers = request.headers_mut();
    headers.insert(ORIGIN, format!("http://{addr}").parse().unwrap());
    headers.insert(ACCEPT_ENCODING, "br, gzip".parse().unwrap());
    headers.insert(
        SEC_WEBSOCKET_EXTENSIONS,
        "permessage-deflate; client_max_window_bits"
            .parse()
            .unwrap(),
    );
    let (_socket, handshake) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(handshake.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(handshake.headers().get(header::CONTENT_ENCODING), None);
    // Nor are its messages: the daemon takes up no compression the client
    // offers for them.
    assert_eq!(handshake.headers().get(SEC_WEBSOCKET_EXTENSIONS), None);
}
