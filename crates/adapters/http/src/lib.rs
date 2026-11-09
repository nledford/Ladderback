//! HTTP adapter (technical design §4.2). In M0 it serves the embedded web
//! UI; the REST API and the events WebSocket follow in M1b. Every request
//! passes the request checks of §10.1 first.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod request_checks;

use std::borrow::Cow;
#[cfg(debug_assertions)]
use std::path::{Component, Path};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

pub use request_checks::{
    InvalidHostName, NonLoopbackBind, RequestChecks, require_loopback, validate_host_name,
};

use api::UiEvent;
use app::ports::EventBus;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;

/// What the `/api/v1` routes run against (technical design §4.2): the
/// daemon's event bus and operations, wired by the composition root.
#[derive(Clone)]
pub struct AppState {
    /// Where the daemon's live updates come from.
    pub events: Arc<dyn EventBus>,
    /// How a request starts an operation.
    pub operations: Arc<dyn Operations>,
}

/// Starts operations from the API. M1b's slice is one route: a backup of a
/// named plan. Cancel, polling and history are the operations page's work
/// (milestone 2's E2), which grows this.
#[async_trait]
pub trait Operations: Send + Sync {
    /// Starts a backup of the plan named in the request, returning the
    /// operation's ID.
    async fn start_backup(&self, plan: String) -> Result<String, StartError>;
}

/// Why a request couldn't start an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// No plan by that name is configured.
    UnknownPlan,
    /// A plan is there but cannot be started just now, for example because
    /// its configuration or repository cannot be read.
    Unavailable,
}

/// The body `POST /api/v1/operations` takes.
#[derive(Deserialize)]
struct RunPlanBody {
    plan: String,
}

/// What the same route answers: the started operation's ID, unenveloped.
#[derive(Serialize)]
struct StartedOperation {
    operation: String,
}

/// A file of the built web UI.
#[derive(Clone, Debug)]
pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
}

/// Where the web UI's files come from: the UI build, or a map in tests.
pub trait AssetSource: Send + Sync + 'static {
    /// `path` is relative to the UI root, without a leading slash.
    fn get(&self, path: &str) -> Option<Asset>;
}

/// The trunk build output of `crates/web`. A release build embeds it in the
/// binary; a debug build reads it as the daemon runs, from the
/// `crates/web/dist` of the checkout the crate was compiled in.
pub struct EmbeddedUi;

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct ReleaseUi;

// A debug build doesn't use rust-embed's disk mode (#47). That mode takes the
// folder from CARGO_MANIFEST_DIR inside its derive macro, a read rustc doesn't
// record, and checks each file against the folder path as it was when the
// macro ran. A build cache that shares compiled crates between checkouts (mbx
// does) therefore keyed the crate on its sources alone and restored the copy
// another checkout had compiled, which looks for the UI in that checkout. And
// a crate compiled before `trunk build` has a folder path with `..` in it that
// no file matches, so its daemon answered 503 after the build. Here `env!`
// puts CARGO_MANIFEST_DIR in the crate's dep-info, so a cache keys the crate
// on its checkout's path, and nothing in the crate depends on whether the
// folder existed when it was compiled. A release build embeds the files, and a
// cache keys the crate on their contents.
#[cfg(debug_assertions)]
const UI_FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist");

impl AssetSource for EmbeddedUi {
    #[cfg(not(debug_assertions))]
    fn get(&self, path: &str) -> Option<Asset> {
        ReleaseUi::get(path).map(|file| Asset { bytes: file.data })
    }

    #[cfg(debug_assertions)]
    fn get(&self, path: &str) -> Option<Asset> {
        read_ui_file(Path::new(UI_FOLDER), path)
    }
}

/// A file of the web UI as a debug build reads it: from `folder`, when it is
/// asked for. `None` when the UI isn't built, and for a `path` that is not a
/// plain relative file path.
#[cfg(debug_assertions)]
fn read_ui_file(folder: &Path, path: &str) -> Option<Asset> {
    // `serve` already refuses `..`, `%` and backslashes. This is the function
    // that touches the disk, so it doesn't rely on its caller to keep a lookup
    // inside the folder.
    if !Path::new(path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let bytes = std::fs::read(folder.join(path)).ok()?;
    Some(Asset {
        bytes: Cow::Owned(bytes),
    })
}

/// Routes for the web UI and the API: files by path, `index.html` for any
/// other extensionless path (the client-side router handles it), the
/// live-updates WebSocket under `/api/v1`, and the one operations route of
/// this milestone. A request that fails `checks` never reaches a route.
pub fn router(assets: Arc<dyn AssetSource>, checks: RequestChecks, state: AppState) -> Router {
    Router::new()
        .route("/api/v1/events", axum::routing::get(events))
        .route("/api/v1/operations", axum::routing::post(run_plan))
        .with_state(state)
        .fallback(move |method: Method, uri: Uri| {
            let assets = Arc::clone(&assets);
            async move { serve(assets.as_ref(), &method, &uri) }
        })
        .layer(from_fn_with_state(checks, request_checks::enforce))
}

/// The live-updates WebSocket (technical design §4.4): one per tab, carrying
/// [`UiEvent`]s. It sends nothing until an event arrives — the first state
/// comes over REST — and the client is dropped when a send fails. The
/// request checks have already allowed this handshake: a foreign `Origin`
/// never reaches here.
async fn events(upgrade: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let stream = state.events.subscribe().await;
    upgrade.on_upgrade(move |socket| forward(socket, stream))
}

/// Reads the bus and writes each event to the client, until the client goes
/// or the daemon does.
async fn forward(mut socket: WebSocket, mut stream: BoxStream<'static, UiEvent>) {
    while let Some(event) = stream.next().await {
        // An event that cannot be written is not retryable here: the client
        // is gone, so the connection ends rather than falling behind.
        let Ok(text) = serde_json::to_string(&event) else {
            break;
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}

/// Starts a backup of the plan the body names: the M1b slice of the
/// operations API. The answer is the operation's ID; what the operation
/// reports arrives over the events WebSocket. Cancel, polling and history
/// belong to the operations page's milestone, which grows this route.
async fn run_plan(
    State(state): State<AppState>,
    body: Result<axum::Json<RunPlanBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let plan = match body {
        Ok(axum::Json(RunPlanBody { plan })) => plan,
        Err(_) => return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody),
    };
    match state.operations.start_backup(plan).await {
        Ok(operation) => (
            StatusCode::ACCEPTED,
            axum::Json(StartedOperation { operation }),
        )
            .into_response(),
        Err(StartError::UnknownPlan) => refused(StatusCode::NOT_FOUND, api::ErrorCode::PlanUnknown),
        Err(StartError::Unavailable) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::ConfigUnavailable,
        ),
    }
}

/// The body of a refusal: the code alone, for the UI to render (§8.4).
#[derive(Serialize)]
struct TypedRefusal {
    code: api::ErrorCode,
}

/// A typed refusal (§10.1) with `status`.
fn refused(status: StatusCode, code: api::ErrorCode) -> Response {
    (status, axum::Json(TypedRefusal { code })).into_response()
}

fn serve(assets: &dyn AssetSource, method: &Method, uri: &Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, HeaderValue::from_static("GET, HEAD"))],
        )
            .into_response();
    }
    let path = uri.path().trim_start_matches('/');
    // UI file names are plain: no parent segments, escapes or backslashes.
    // Refusing these keeps every lookup inside the UI folder whatever the
    // asset source does with a path.
    if path.split('/').any(|segment| segment == "..") || path.contains(['%', '\\']) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !path.is_empty()
        && let Some(asset) = assets.get(path)
    {
        return file(path, asset);
    }
    // A missing UI file is a 404; any other path is a client-side route, and
    // routes can contain dots (hostnames such as `nas.example.com`).
    let is_file_request = path
        .rsplit_once('.')
        .is_some_and(|(_, extension)| UI_FILE_EXTENSIONS.contains(&extension));
    if is_file_request {
        return StatusCode::NOT_FOUND.into_response();
    }
    match assets.get(INDEX) {
        Some(index) => file(INDEX, index),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "This build of Ladderback has no web UI. Build crates/web with trunk, then rebuild Ladderback.",
        )
            .into_response(),
    }
}

const INDEX: &str = "index.html";

/// The kinds of file a trunk build of the UI contains.
const UI_FILE_EXTENSIONS: &[&str] = &[
    "css",
    "html",
    "ico",
    "js",
    "json",
    "map",
    "png",
    "svg",
    "txt",
    "wasm",
    "webmanifest",
    "woff2",
];

fn file(path: &str, asset: Asset) -> Response {
    let content_type = if path == INDEX {
        HeaderValue::from_static("text/html; charset=utf-8")
    } else {
        // A MIME type is always valid header text; a file served with the
        // generic type is the worst a failure here can cause.
        HeaderValue::from_str(
            mime_guess::from_path(path)
                .first_or_octet_stream()
                .essence_str(),
        )
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
    };
    // Trunk puts a content hash in every asset name except index.html, so
    // only the entry page must be revalidated.
    let cache_control = HeaderValue::from_static(if path == INDEX {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    });
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(asset.bytes),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use app::fakes::InMemoryEventBus;

    struct Files(HashMap<&'static str, &'static str>);

    impl AssetSource for Files {
        fn get(&self, path: &str) -> Option<Asset> {
            self.0.get(path).map(|s| Asset {
                bytes: Cow::Borrowed(s.as_bytes()),
            })
        }
    }

    fn ui() -> Arc<dyn AssetSource> {
        Arc::new(Files(HashMap::from([
            ("index.html", "<!doctype html><title>Ladderback</title>"),
            ("web-1a2b.js", "export default 1;"),
            ("web-1a2b_bg.wasm", "\0asm"),
        ])))
    }

    /// Answers every plan but `missing`.
    struct Plans;

    #[async_trait]
    impl Operations for Plans {
        async fn start_backup(&self, plan: String) -> Result<String, StartError> {
            if plan == "missing" {
                Err(StartError::UnknownPlan)
            } else {
                Ok("018f0000-0000-7000-8000-000000000001".to_owned())
            }
        }
    }

    fn state() -> AppState {
        AppState {
            events: Arc::new(InMemoryEventBus::new()),
            operations: Arc::new(Plans),
        }
    }

    async fn request(
        assets: Arc<dyn AssetSource>,
        method: Method,
        path: &str,
        body: &str,
    ) -> Response {
        router(assets, RequestChecks::default(), state())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, "localhost:9898")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Serves the router on a loopback port, and returns where and the bus
    /// its state holds.
    async fn serving() -> (std::net::SocketAddr, Arc<app::fakes::InMemoryEventBus>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let events = Arc::new(InMemoryEventBus::new());
        let state = AppState {
            events: events.clone(),
            operations: Arc::new(Plans),
        };
        let app = router(ui(), RequestChecks::default(), state);
        tokio::spawn(async move {
            let serving = axum::serve(listener, app);
            let _ = serving.await;
        });
        (addr, events)
    }

    async fn get(path: &str) -> Response {
        request(ui(), Method::GET, path, "").await
    }

    fn content_type(response: &Response) -> &str {
        response.headers()[header::CONTENT_TYPE].to_str().unwrap()
    }

    async fn body(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn the_events_handshake_upgrades_and_carries_events() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::tungstenite::http::header::ORIGIN;

        let (addr, events) = serving().await;
        let mut request = format!("ws://{addr}/api/v1/events")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(ORIGIN, format!("http://{addr}").parse().unwrap());
        let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert!(
            response
                .headers()
                .contains_key(header::SEC_WEBSOCKET_ACCEPT),
            "an upgrade without the accept key is not a WebSocket"
        );
        // Nothing arrives before any event does; what is published arrives
        // as it is, in order.
        events
            .publish(UiEvent::OperationStarted {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                repository: None,
                started_at: "2026-11-09T10:00:00Z".into(),
            })
            .await;
        let text = match socket.next().await {
            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => text,
            other => panic!("the first event arrives as text: {other:?}"),
        };
        let event: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        assert_eq!(
            event,
            serde_json::json!({
                "type": "operation-started",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "repository": null,
                "started_at": "2026-11-09T10:00:00Z",
            })
        );
    }

    #[tokio::test]
    async fn a_named_plan_is_accepted_with_its_operation() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/operations",
            r#"{"plan":"home"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            body(response).await,
            r#"{"operation":"018f0000-0000-7000-8000-000000000001"}"#
        );
    }

    #[tokio::test]
    async fn a_plan_the_configuration_does_not_have_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/operations",
            r#"{"plan":"missing"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body(response).await, r#"{"code":"plan-unknown"}"#);
    }

    #[tokio::test]
    async fn a_body_that_is_not_the_json_the_route_takes_is_refused() {
        let response = request(ui(), Method::POST, "/api/v1/operations", "not json").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await, r#"{"code":"invalid-body"}"#);
    }

    #[tokio::test]
    async fn the_root_serves_the_ui_entry_page() {
        let response = get("/").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(content_type(&response), "text/html; charset=utf-8");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-cache",
            "a new release must not be hidden behind a cached entry page"
        );
        assert_eq!(
            body(response).await,
            "<!doctype html><title>Ladderback</title>"
        );
    }

    #[tokio::test]
    async fn assets_are_served_with_their_content_type() {
        let js = get("/web-1a2b.js").await;
        assert_eq!(js.status(), StatusCode::OK);
        assert_eq!(content_type(&js), "text/javascript");
        let wasm = get("/web-1a2b_bg.wasm").await;
        assert_eq!(content_type(&wasm), "application/wasm");
    }

    #[tokio::test]
    async fn client_side_routes_get_the_entry_page() {
        let response = get("/machines/laptop").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(content_type(&response), "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn client_side_routes_may_contain_dots() {
        for route in ["/machines/laptop.local", "/machines/nas.example.com"] {
            let response = get(route).await;
            assert_eq!(response.status(), StatusCode::OK, "{route}");
            assert_eq!(content_type(&response), "text/html; charset=utf-8");
        }
    }

    #[tokio::test]
    async fn a_missing_file_is_not_found() {
        assert_eq!(get("/missing.js").await.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_reads_are_allowed() {
        let response = request(ui(), Method::POST, "/", "").await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    }

    #[tokio::test]
    async fn a_binary_without_the_ui_says_so() {
        let response = request(Arc::new(Files(HashMap::new())), Method::GET, "/", "").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(body(response).await.contains("web UI"));
    }

    /// Answers every path, as a careless file source would.
    struct Anything;

    impl AssetSource for Anything {
        fn get(&self, _path: &str) -> Option<Asset> {
            Some(Asset {
                bytes: Cow::Borrowed(b"secret"),
            })
        }
    }

    #[tokio::test]
    async fn paths_never_leave_the_ui_folder() {
        for path in [
            "/../Cargo.toml",
            "/assets/../../secret.txt",
            "/%2e%2e/Cargo.toml",
            "/..%2fCargo.toml",
            "/..\\Cargo.toml",
        ] {
            let response = request(Arc::new(Anything), Method::GET, path, "").await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    /// What a debug build reads the UI from: the folder, as it is when a file
    /// is asked for.
    #[cfg(debug_assertions)]
    mod ui_folder {
        use std::fs;

        use super::*;

        #[test]
        fn files_built_after_the_binary_are_served() {
            let ui = tempfile::tempdir().unwrap();
            assert!(read_ui_file(ui.path(), "index.html").is_none());
            fs::create_dir(ui.path().join("assets")).unwrap();
            fs::write(ui.path().join("index.html"), "<title>built</title>").unwrap();
            fs::write(ui.path().join("assets/app.js"), "export default 1;").unwrap();

            let index = read_ui_file(ui.path(), "index.html").unwrap();
            assert_eq!(index.bytes.as_ref(), b"<title>built</title>");
            let script = read_ui_file(ui.path(), "assets/app.js").unwrap();
            assert_eq!(script.bytes.as_ref(), b"export default 1;");
        }

        #[test]
        fn an_unbuilt_ui_has_no_files() {
            let scratch = tempfile::tempdir().unwrap();
            let missing = scratch.path().join("dist");
            assert!(read_ui_file(&missing, "index.html").is_none());
        }

        #[test]
        fn paths_that_leave_the_folder_are_refused() {
            let scratch = tempfile::tempdir().unwrap();
            let ui = scratch.path().join("dist");
            fs::create_dir_all(ui.join("assets")).unwrap();
            let secret = scratch.path().join("secret.txt");
            fs::write(&secret, "secret").unwrap();
            let absolute = secret.to_str().unwrap();
            for path in ["../secret.txt", "assets/../../secret.txt", absolute] {
                assert!(read_ui_file(&ui, path).is_none(), "{path}");
            }
        }
    }
}
