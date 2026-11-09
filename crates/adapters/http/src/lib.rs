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
use axum::extract::Path as AxumPath;
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
/// daemon's event bus, operations and catalog, wired by the composition
/// root.
#[derive(Clone)]
pub struct AppState {
    /// Where the daemon's live updates come from.
    pub events: Arc<dyn EventBus>,
    /// How a request starts an operation.
    pub operations: Arc<dyn Operations>,
    /// How a request reads the configuration's plans, repositories and
    /// snapshots.
    pub catalog: Arc<dyn Catalog>,
}

/// The operations the API asks of the daemon (technical design §4.2): the
/// M1b start-by-plan route, and E2's cancel and history.
#[async_trait]
pub trait Operations: Send + Sync {
    /// Starts a backup of the plan named in the request, returning the
    /// operation's ID.
    async fn start_backup(&self, plan: String) -> Result<String, StartError>;
    /// Starts maintenance (forget, prune or check) on the repository named
    /// in the request, returning the operation's ID. Refusals are typed: an
    /// unknown repository or kind, a busy repository (I7), or a policy that
    /// would keep nothing.
    async fn start_maintenance(
        &self,
        repository: String,
        kind: String,
    ) -> Result<String, MaintenanceStartError>;

    /// One page of the operation history, newest first, including running
    /// operations. `after` continues from a page's `next`; `plan` lists one
    /// plan's operations only; `limit` is clamped, never an error.
    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: Option<u64>,
    ) -> Result<api::OperationsPageDto, HistoryError>;

    /// One operation's detail.
    async fn operation(&self, id: String) -> Result<api::OperationDetailDto, LookupError>;

    /// Cancels a running operation. Finished operations and unknown IDs are
    /// typed refusals, not errors.
    async fn cancel(&self, id: String) -> Result<(), CancelError>;
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

/// The catalog the API asks of the daemon (§4.2): the plans the operations
/// page can start, the repositories the snapshots page can show, and one
/// repository's snapshots, fetched live — nothing stores them.
#[async_trait]
pub trait Catalog: Send + Sync {
    /// The configured plans, in the order they were written.
    async fn plans(&self) -> Result<api::PlansDto, CatalogError>;

    /// The configured repositories' aliases, in the order of their aliases.
    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError>;

    /// One repository's snapshots, newest first.
    async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError>;
}

/// Why a catalog request couldn't be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// The configuration cannot be read, so nothing is known.
    Unavailable,
}

/// Why a snapshot listing couldn't be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotsError {
    /// The configuration has no repository by that name.
    Unknown,
    /// The repository is there but its snapshots cannot be read just now,
    /// for example because its secrets cannot be resolved or the engine
    /// cannot be started.
    Unavailable,
}

/// Why a history request couldn't be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryError {
    /// `after` isn't an operation's ID, so it can name no place to continue
    /// from. The text as sent becomes the refusal's argument.
    BadCursor,
}

/// Why an operation couldn't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// The log has no operation with this ID.
    Unknown,
}

/// Why an operation couldn't be cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelError {
    /// The log has no operation with this ID.
    Unknown,
    /// The operation has already ended, so there is nothing to stop.
    NotRunning,
}

/// Why a maintenance request couldn't start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaintenanceStartError {
    /// No repository by that alias is configured.
    UnknownRepository,
    /// The request asks for a maintenance kind this version doesn't run.
    UnknownKind,
    /// Another maintenance operation holds the repository (I7). Names the
    /// one that does.
    Busy { running: String },
    /// A plan's retention policy would keep nothing; nothing was removed.
    /// Names the plan.
    KeepsNothing { plan: String },
    /// The repository or its configuration can't be read just now.
    Unavailable,
}

/// The body `POST /api/v1/repositories/{alias}/maintenance` takes.
#[derive(Deserialize)]
struct RunMaintenanceBody {
    kind: String,
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
/// live-updates WebSocket under `/api/v1`, the operations routes — start,
/// history, one operation, cancel — and the catalog routes the pages read:
/// the plans, the repositories, one repository's snapshots. A request that
/// fails `checks` never reaches a route.
pub fn router(assets: Arc<dyn AssetSource>, checks: RequestChecks, state: AppState) -> Router {
    Router::new()
        .route("/api/v1/events", axum::routing::get(events))
        .route(
            "/api/v1/operations",
            axum::routing::get(list_operations).post(run_plan),
        )
        .route("/api/v1/operations/{id}", axum::routing::get(get_operation))
        .route(
            "/api/v1/operations/{id}/cancel",
            axum::routing::post(cancel_operation),
        )
        .route("/api/v1/plans", axum::routing::get(list_plans))
        .route(
            "/api/v1/repositories",
            axum::routing::get(list_repositories),
        )
        .route(
            "/api/v1/repositories/{alias}/snapshots",
            axum::routing::get(list_snapshots),
        )
        .route(
            "/api/v1/repositories/{alias}/maintenance",
            axum::routing::post(run_maintenance),
        )
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
/// or the daemon does. A client's `Ping` is answered with a `Pong`: the
/// events stream never waits on the client, so the reply races fairly with
/// the next event.
async fn forward(mut socket: WebSocket, mut stream: BoxStream<'static, UiEvent>) {
    loop {
        tokio::select! {
            event = stream.next() => {
                let Some(event) = event else { break };
                // An event that cannot be written is not retryable here: the
                // client is gone, so the connection ends rather than falling
                // behind.
                let Ok(text) = serde_json::to_string(&event) else {
                    break;
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
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
    match state.operations.start_backup(plan.clone()).await {
        Ok(operation) => (
            StatusCode::ACCEPTED,
            axum::Json(StartedOperation { operation }),
        )
            .into_response(),
        Err(StartError::UnknownPlan) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::PlanUnknown,
            vec![("plan", api::MsgArg::Text(plan))],
        ),
        Err(StartError::Unavailable) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::ConfigUnavailable,
        ),
    }
}

/// Starts maintenance on the repository the path names, of the kind the
/// body names: forget, prune or check. The answer is the operation's ID,
/// and the refusals are typed — an unknown repository or kind, a busy
/// repository (I7), or a retention policy that would keep nothing.
async fn run_maintenance(
    State(state): State<AppState>,
    AxumPath(alias): AxumPath<String>,
    body: Result<axum::Json<RunMaintenanceBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let kind = match body {
        Ok(axum::Json(RunMaintenanceBody { kind })) => kind,
        Err(_) => return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody),
    };
    match state
        .operations
        .start_maintenance(alias.clone(), kind.clone())
        .await
    {
        Ok(operation) => (
            StatusCode::ACCEPTED,
            axum::Json(StartedOperation { operation }),
        )
            .into_response(),
        Err(MaintenanceStartError::UnknownRepository) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::RepositoryUnknown,
            vec![("alias", api::MsgArg::Text(alias))],
        ),
        Err(MaintenanceStartError::UnknownKind) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::MaintenanceKindUnknown,
            vec![("kind", api::MsgArg::Text(kind))],
        ),
        Err(MaintenanceStartError::Busy { running }) => refused_with(
            StatusCode::CONFLICT,
            api::ErrorCode::MaintenanceBusy,
            vec![("operation", api::MsgArg::Text(running))],
        ),
        Err(MaintenanceStartError::KeepsNothing { plan }) => refused_with(
            StatusCode::UNPROCESSABLE_ENTITY,
            api::ErrorCode::RetentionKeepsNothing,
            vec![("plan", api::MsgArg::Text(plan))],
        ),
        Err(MaintenanceStartError::Unavailable) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::ConfigUnavailable,
        ),
    }
}

/// The query the history route takes: the keyset cursor, the page size and
/// the plan filter (technical design §3.2). All optional.
#[derive(Deserialize, Default)]
struct HistoryParams {
    after: Option<String>,
    limit: Option<u64>,
    plan: Option<String>,
}

/// One page of the operation history, newest first, including running
/// operations: the page's initial state, which a client that then subscribes
/// closes the gap a late subscriber has (§4.2).
async fn list_operations(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HistoryParams>,
) -> Response {
    match state
        .operations
        .history(params.plan, params.after.clone(), params.limit)
        .await
    {
        Ok(page) => (StatusCode::OK, axum::Json(page)).into_response(),
        Err(HistoryError::BadCursor) => refused_with(
            StatusCode::BAD_REQUEST,
            api::ErrorCode::InvalidCursor,
            params
                .after
                .map(|cursor| vec![("cursor", api::MsgArg::Text(cursor))])
                .unwrap_or_default(),
        ),
    }
}

/// One operation's detail: what happened, with its warnings.
async fn get_operation(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match state.operations.operation(id.clone()).await {
        Ok(detail) => (StatusCode::OK, axum::Json(detail)).into_response(),
        Err(LookupError::Unknown) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::OperationUnknown,
            vec![("operation", api::MsgArg::Text(id))],
        ),
    }
}

/// Cancels a running operation (§4.2): the empty JSON body the JSON-body
/// rule asks of every mutating method, and a polite cancel — the worker
/// stops, the finally-hooks run, the operation ends cancelled.
async fn cancel_operation(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Result<axum::Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if body.is_err() {
        return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody);
    }
    match state.operations.cancel(id.clone()).await {
        Ok(()) => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
        Err(CancelError::Unknown) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::OperationUnknown,
            vec![("operation", api::MsgArg::Text(id))],
        ),
        Err(CancelError::NotRunning) => refused_with(
            StatusCode::CONFLICT,
            api::ErrorCode::OperationNotRunning,
            vec![("operation", api::MsgArg::Text(id))],
        ),
    }
}

/// The configured plans: what the operations page can start.
async fn list_plans(State(state): State<AppState>) -> Response {
    match state.catalog.plans().await {
        Ok(plans) => (StatusCode::OK, axum::Json(plans)).into_response(),
        Err(CatalogError::Unavailable) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::ConfigUnavailable,
        ),
    }
}

/// The configured repositories' aliases: what the snapshots page can show.
async fn list_repositories(State(state): State<AppState>) -> Response {
    match state.catalog.repositories().await {
        Ok(repositories) => (StatusCode::OK, axum::Json(repositories)).into_response(),
        Err(CatalogError::Unavailable) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::ConfigUnavailable,
        ),
    }
}

/// One repository's snapshots, newest first, fetched live (§9): a read is
/// safe to run alongside anything, so it takes no slot and stores nothing.
async fn list_snapshots(
    State(state): State<AppState>,
    axum::extract::Path(alias): axum::extract::Path<String>,
) -> Response {
    match state.catalog.snapshots(alias.clone()).await {
        Ok(snapshots) => (StatusCode::OK, axum::Json(snapshots)).into_response(),
        Err(SnapshotsError::Unknown) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::RepositoryUnknown,
            vec![("alias", api::MsgArg::Text(alias))],
        ),
        Err(SnapshotsError::Unavailable) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::EngineUnavailable,
        ),
    }
}

/// The body of a refusal: the code, and the arguments its message takes, for
/// the UI to render (§8.4).
#[derive(Serialize)]
struct TypedRefusal {
    code: api::ErrorCode,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    args: std::collections::BTreeMap<&'static str, api::MsgArg>,
}

/// A typed refusal (§10.1) with `status`.
fn refused(status: StatusCode, code: api::ErrorCode) -> Response {
    refused_with(status, code, Vec::new())
}

/// A typed refusal carrying the arguments its code declares, named as
/// [`ErrorCode::args`](api::ErrorCode::args) names them.
fn refused_with(
    status: StatusCode,
    code: api::ErrorCode,
    args: Vec<(&'static str, api::MsgArg)>,
) -> Response {
    (
        status,
        axum::Json(TypedRefusal {
            code,
            args: args.into_iter().collect(),
        }),
    )
        .into_response()
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

    /// The one operation the fake log holds, as the wire shows it.
    fn canned_summary() -> api::OperationSummaryDto {
        api::OperationSummaryDto::new(
            "018f0000-0000-7000-8000-000000000001".to_owned(),
            "home".to_owned(),
            "backup".to_owned(),
            "succeeded".to_owned(),
            "2026-11-09T10:00:00Z".to_owned(),
            Some("2026-11-09T10:00:01Z".to_owned()),
            Some("2026-11-09T10:00:02Z".to_owned()),
            Vec::new(),
            None,
            Some(api::OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000,
            }),
        )
        .unwrap()
    }

    #[async_trait]
    impl Operations for Plans {
        async fn start_backup(&self, plan: String) -> Result<String, StartError> {
            if plan == "missing" {
                Err(StartError::UnknownPlan)
            } else {
                Ok("018f0000-0000-7000-8000-000000000001".to_owned())
            }
        }

        async fn start_maintenance(
            &self,
            repository: String,
            kind: String,
        ) -> Result<String, MaintenanceStartError> {
            match (repository.as_str(), kind.as_str()) {
                ("demo", "forget") => Ok("018f0000-0000-7000-8000-000000000002".to_owned()),
                ("demo", _) => Err(MaintenanceStartError::UnknownKind),
                ("missing", _) => Err(MaintenanceStartError::UnknownRepository),
                ("busy", _) => Err(MaintenanceStartError::Busy {
                    running: "018f0000-0000-7000-8000-000000000003".to_owned(),
                }),
                ("keeps-nothing", _) => Err(MaintenanceStartError::KeepsNothing {
                    plan: "home".to_owned(),
                }),
                _ => Err(MaintenanceStartError::Unavailable),
            }
        }

        async fn history(
            &self,
            _plan: Option<String>,
            after: Option<String>,
            _limit: Option<u64>,
        ) -> Result<api::OperationsPageDto, HistoryError> {
            // A cursor that names no place to continue from is refused, as
            // the real log refuses it.
            if after.is_some_and(|cursor| cursor.parse::<uuid::Uuid>().is_err()) {
                return Err(HistoryError::BadCursor);
            }
            Ok(api::OperationsPageDto {
                operations: vec![canned_summary()],
                next: None,
            })
        }

        async fn operation(&self, id: String) -> Result<api::OperationDetailDto, LookupError> {
            if id == "018f0000-0000-7000-8000-000000000001" {
                Ok(api::OperationDetailDto {
                    operation: canned_summary(),
                    warnings: Vec::new(),
                })
            } else {
                Err(LookupError::Unknown)
            }
        }

        async fn cancel(&self, id: String) -> Result<(), CancelError> {
            if id == "018f0000-0000-7000-8000-000000000001" {
                // The one logged operation finished long ago.
                Err(CancelError::NotRunning)
            } else {
                Err(CancelError::Unknown)
            }
        }
    }

    /// Answers every repository but `missing`, with one snapshot.
    struct DemoCatalog;

    fn demo_snapshot() -> api::SnapshotDto {
        api::SnapshotDto::new(
            "ab".repeat(32),
            "2026-11-09T10:15:00Z".to_owned(),
            "demo-host".to_owned(),
            "home".to_owned(),
            vec!["ladderback-plan:demo-home".to_owned()],
            vec!["/home/demo".to_owned()],
        )
        .unwrap()
    }

    #[async_trait]
    impl Catalog for DemoCatalog {
        async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
            Ok(api::PlansDto {
                plans: vec![api::PlanSummaryDto {
                    id: "home".to_owned(),
                    repository: "demo".to_owned(),
                }],
            })
        }

        async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
            Ok(api::RepositoriesDto {
                repositories: vec!["demo".to_owned()],
            })
        }

        async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
            if alias == "missing" {
                Err(SnapshotsError::Unknown)
            } else {
                Ok(api::SnapshotsDto {
                    snapshots: vec![demo_snapshot()],
                })
            }
        }
    }

    fn state() -> AppState {
        AppState {
            events: Arc::new(InMemoryEventBus::new()),
            operations: Arc::new(Plans),
            catalog: Arc::new(DemoCatalog),
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
            catalog: Arc::new(DemoCatalog),
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
        // The refusal names the plan, as `ErrorCode::PlanUnknown` declares.
        assert_eq!(
            body(response).await,
            r#"{"code":"plan-unknown","args":{"plan":{"type":"text","value":"missing"}}}"#
        );
    }

    #[tokio::test]
    async fn maintenance_on_a_repository_is_accepted_with_its_operation() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"forget"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            body(response).await,
            r#"{"operation":"018f0000-0000-7000-8000-000000000002"}"#
        );
    }

    #[tokio::test]
    async fn a_repository_the_configuration_does_not_have_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/missing/maintenance",
            r#"{"kind":"forget"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(response).await,
            r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
        );
    }

    #[tokio::test]
    async fn a_maintenance_kind_this_version_does_not_run_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"rewrite"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(response).await,
            r#"{"code":"maintenance-kind-unknown","args":{"kind":{"type":"text","value":"rewrite"}}}"#
        );
    }

    #[tokio::test]
    async fn a_busy_repository_is_refused_with_the_operation_that_holds_it() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/busy/maintenance",
            r#"{"kind":"prune"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(response).await,
            r#"{"code":"maintenance-busy","args":{"operation":{"type":"text","value":"018f0000-0000-7000-8000-000000000003"}}}"#
        );
    }

    #[tokio::test]
    async fn a_policy_that_keeps_nothing_is_refused_with_its_plan() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/keeps-nothing/maintenance",
            r#"{"kind":"forget"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body(response).await,
            r#"{"code":"retention-keeps-nothing","args":{"plan":{"type":"text","value":"home"}}}"#
        );
    }

    #[tokio::test]
    async fn the_history_route_answers_one_page() {
        let response = get("/api/v1/operations").await;
        assert_eq!(response.status(), StatusCode::OK);
        let page: api::OperationsPageDto = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(page.operations.len(), 1);
        assert_eq!(page.next, None);
    }

    #[tokio::test]
    async fn a_cursor_that_is_not_an_operation_is_refused() {
        let response = get("/api/v1/operations?after=not-an-operation").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body(response).await,
            r#"{"code":"invalid-cursor","args":{"cursor":{"type":"text","value":"not-an-operation"}}}"#
        );
    }

    #[tokio::test]
    async fn an_unknown_operation_is_refused_by_detail_and_cancel() {
        let unknown = "018f0000-0000-7000-8000-00000000dead";
        let refusal = format!(
            r#"{{"code":"operation-unknown","args":{{"operation":{{"type":"text","value":"{unknown}"}}}}}}"#
        );
        let detail = get(&format!("/api/v1/operations/{unknown}")).await;
        assert_eq!(detail.status(), StatusCode::NOT_FOUND);
        assert_eq!(body(detail).await, refusal);
        let cancel = request(
            ui(),
            Method::POST,
            &format!("/api/v1/operations/{unknown}/cancel"),
            "{}",
        )
        .await;
        assert_eq!(cancel.status(), StatusCode::NOT_FOUND);
        assert_eq!(body(cancel).await, refusal);
    }

    #[tokio::test]
    async fn a_finished_operation_is_refused_by_cancel() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/operations/018f0000-0000-7000-8000-000000000001/cancel",
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(response).await,
            r#"{"code":"operation-not-running","args":{"operation":{"type":"text","value":"018f0000-0000-7000-8000-000000000001"}}}"#
        );
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

    #[tokio::test]
    async fn the_plans_and_repositories_are_listed() {
        let plans = get("/api/v1/plans").await;
        assert_eq!(plans.status(), StatusCode::OK);
        assert_eq!(
            body(plans).await,
            r#"{"plans":[{"id":"home","repository":"demo"}]}"#
        );

        let repositories = get("/api/v1/repositories").await;
        assert_eq!(repositories.status(), StatusCode::OK);
        assert_eq!(body(repositories).await, r#"{"repositories":["demo"]}"#);
    }

    #[tokio::test]
    async fn a_repository_snapshots_are_listed_newest_first() {
        let response = get("/api/v1/repositories/demo/snapshots").await;
        assert_eq!(response.status(), StatusCode::OK);
        let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(
            wire["snapshots"][0]["id"],
            "ab".repeat(32),
            "the snapshot the fake catalog holds"
        );
    }

    #[tokio::test]
    async fn a_repository_the_configuration_does_not_have_is_a_typed_refusal() {
        let response = get("/api/v1/repositories/missing/snapshots").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(response).await,
            r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
        );
    }
}
