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
use axum::extract::ws::WebSocketUpgrade;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

pub use request_checks::{
    InvalidHostName, NonLoopbackBind, RequestChecks, require_loopback, validate_host_name,
};

use app::ports::EventBus;

/// What the `/api/v1` routes run against (technical design §4.2): the
/// daemon's event bus, wired by the composition root.
#[derive(Clone)]
pub struct AppState {
    /// Where the daemon's live updates come from.
    pub events: Arc<dyn EventBus>,
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
/// other extensionless path (the client-side router handles it), and the
/// live-updates WebSocket under `/api/v1`. A request that fails `checks`
/// never reaches a route.
pub fn router(assets: Arc<dyn AssetSource>, checks: RequestChecks, state: AppState) -> Router {
    Router::new()
        .route("/api/v1/events", axum::routing::get(events))
        .with_state(state)
        .fallback(move |method: Method, uri: Uri| {
            let assets = Arc::clone(&assets);
            async move { serve(assets.as_ref(), &method, &uri) }
        })
        .layer(from_fn_with_state(checks, request_checks::enforce))
}

/// The live-updates WebSocket (technical design §4.4): one per tab, carrying
/// [`api::UiEvent`]s. It sends nothing until an event arrives — the first
/// state comes over REST — and the client is dropped when a send fails. The
/// request checks have already allowed this handshake: a foreign `Origin`
/// never reaches here.
///
/// This is commit 1 of the M1b event stream: the route is wired so the
/// handshake tests have something to reach, and the body is a `todo!()`
/// stub until the implementation commits (development.md §13.1).
#[allow(clippy::todo)] // todo!() stubs are commit-1 only (development.md §13.1)
async fn events(_upgrade: WebSocketUpgrade, State(_state): State<AppState>) -> Response {
    todo!()
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

    fn state() -> AppState {
        AppState {
            events: Arc::new(InMemoryEventBus::new()),
        }
    }

    async fn request(assets: Arc<dyn AssetSource>, method: Method, path: &str) -> Response {
        router(assets, RequestChecks::default(), state())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, "localhost:9898")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// A WebSocket handshake for `/api/v1/events`, from the UI's own origin,
    /// exactly as a browser's `new WebSocket(...)` sends it.
    async fn handshake() -> Response {
        router(ui(), RequestChecks::default(), state())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/events")
                    .header(header::HOST, "localhost:9898")
                    .header(header::ORIGIN, "http://localhost:9898")
                    .header(header::CONNECTION, "Upgrade")
                    .header(header::UPGRADE, "websocket")
                    .header(header::SEC_WEBSOCKET_VERSION, "13")
                    .header(header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn get(path: &str) -> Response {
        request(ui(), Method::GET, path).await
    }

    fn content_type(response: &Response) -> &str {
        response.headers()[header::CONTENT_TYPE].to_str().unwrap()
    }

    async fn body(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn the_events_handshake_upgrades_to_a_websocket() {
        let response = handshake().await;
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert!(
            response
                .headers()
                .contains_key(header::SEC_WEBSOCKET_ACCEPT),
            "an upgrade without the accept key is not a WebSocket"
        );
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
        let response = request(ui(), Method::POST, "/").await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
    }

    #[tokio::test]
    async fn a_binary_without_the_ui_says_so() {
        let response = request(Arc::new(Files(HashMap::new())), Method::GET, "/").await;
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
            let response = request(Arc::new(Anything), Method::GET, path).await;
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
