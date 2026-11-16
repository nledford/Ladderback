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

mod api_tokens;
mod body_limit;
mod full_disk_access;
mod listener;
mod maintenance;
mod plans;
mod profiles;
mod proposals;
mod repositories;
mod request_checks;
mod response_headers;
mod restart;
mod restores;
mod sign_in;

use std::borrow::Cow;
#[cfg(debug_assertions)]
use std::path::{Component, Path};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;
// The release build embeds `crates/web/dist` through rust-embed (`ReleaseUi`);
// a debug build reads it from disk. Cargo can't make a dependency depend on
// `debug_assertions`, so a debug build references the crate only to quiet
// `cargo::unused_dependencies`, which would otherwise warn on every debug
// build. The lint stays on for every other dependency.
#[cfg(debug_assertions)]
use rust_embed as _;
use serde::{Deserialize, Serialize};

pub use full_disk_access::{FullDiskAccessRoute, SettingsOpener, SettingsUnopened};
pub use listener::{GatedListener, PeerAddr};
pub use maintenance::{MaintenanceRefusal, MaintenanceRequest};
pub use plans::{Plans, PlansError};
pub use profiles::{Profiles, ProfilesError};
pub use proposals::{Proposals, ProposalsError};
pub use repositories::{Repositories, RepositoriesError, SecretRefusal};
pub use request_checks::{
    Exposure, InvalidHostName, RequestChecks, StartupRefusal, proxied_hosts, startup_rule,
    validate_host_name,
};
pub use restart::{Restart, RestartAsker, Work, asked_by};
pub use restores::{RestoreRefusal, RestoreStart, Restores};
pub use sign_in::{COOKIE_NAME, CSRF_HEADER, SignIn};

use api::UiEvent;
use app::ports::EventBus;
use async_trait::async_trait;
use domain::execution::Trigger;
use futures::StreamExt;
use futures::stream::BoxStream;

/// What the `/api/v1` routes run against (technical design §4.2): the
/// daemon's event bus, operations, catalog and repositories, wired by the
/// composition root.
#[derive(Clone)]
pub struct AppState {
    /// Where the daemon's live updates come from.
    pub events: Arc<dyn EventBus>,
    /// How a request starts an operation.
    pub operations: Arc<dyn Operations>,
    /// How a request reads the configuration's plans, repositories and
    /// snapshots.
    pub catalog: Arc<dyn Catalog>,
    /// How a request tests, connects and initialises a repository.
    pub repositories: Arc<dyn Repositories>,
    /// How a request reads, creates, changes and deletes a plan.
    pub plans: Arc<dyn Plans>,
    /// How a request previews a profile's import and exports a plan as one.
    pub profiles: Arc<dyn Profiles>,
    /// How a request previews and starts a restore (§10.4).
    pub restores: Arc<dyn Restores>,
    /// How a request proposes configuration changes, and reads and
    /// withdraws proposals (decision 89).
    pub proposals: Arc<dyn Proposals>,
    /// Whether sign-in is on, and the sessions it has started (§10.1).
    pub sign_in: SignIn,
    /// Who can reach the daemon at the address it listens on, which the
    /// session route tells the page (§10.1).
    pub exposure: Exposure,
    /// The build the daemon runs, which the session route tells a page that
    /// may see the pages (decision 90).
    pub build: api::BuildDto,
    /// Whether a restart is offered, and the one asked for (decision 94).
    pub restart: Restart,
    /// The available update the release check found, which the session
    /// route tells with the build (decision 107).
    pub updates: Arc<app::updates::UpdateNotice>,
    /// The newest lines of the daemon's own log, which the Log page reads
    /// (#618).
    pub recent_log: Arc<dyn app::ports::RecentLogStore>,
    /// What the Full Disk Access probe found, and what opens System
    /// Settings' pane for it: neither off macOS (§16, decision NN).
    pub full_disk_access: FullDiskAccessRoute,
}

impl axum::extract::FromRef<AppState> for SignIn {
    fn from_ref(state: &AppState) -> Self {
        state.sign_in.clone()
    }
}

impl axum::extract::FromRef<AppState> for Exposure {
    fn from_ref(state: &AppState) -> Self {
        state.exposure
    }
}

/// What a page that may see the pages is told of the daemon (#502): the
/// build it runs, and the update its release check found, when there is
/// one (decision 107). The address of the update's notes is built from its
/// version, never taken from the release feed's answer.
#[derive(Clone, Debug)]
pub(crate) struct Running {
    pub(crate) build: api::BuildDto,
    pub(crate) update: Option<api::AvailableUpdateDto>,
}

impl axum::extract::FromRef<AppState> for Running {
    fn from_ref(state: &AppState) -> Self {
        Self {
            build: state.build.clone(),
            update: state
                .updates
                .available()
                .map(|version| api::AvailableUpdateDto {
                    version: version.to_string(),
                    release_notes: version.release_notes(),
                }),
        }
    }
}

/// The operations the API asks of the daemon (technical design §4.2): the
/// M1b start-by-plan route, maintenance (its history groups, its previews
/// and its start), and E2's cancel and history.
///
/// A call is dropped when its client hangs up before the answer, so a start
/// writes the operation log, and takes a repository's slot (I7), on a task of
/// its own: an operation it began goes on without the client, and one it did
/// not begin has stored nothing.
#[async_trait]
pub trait Operations: Send + Sync {
    /// Starts a backup of the plan named in the request, started by
    /// `trigger` (by hand, or by an API token), returning the operation's
    /// ID.
    async fn start_backup(&self, plan: String, trigger: Trigger) -> Result<String, StartError>;
    /// Starts maintenance (forget, prune, check or adopt) on the repository
    /// named in the request, returning the operation's ID. A forget or an
    /// adoption that carries the `selection` its preview named runs only if
    /// it would act on that selection still. Refusals are typed: an unknown
    /// repository or kind, a busy repository (I7), a policy that would keep
    /// nothing, or a selection the repository no longer matches.
    async fn start_maintenance(
        &self,
        repository: String,
        request: MaintenanceRequest,
        selection: Option<String>,
    ) -> Result<String, MaintenanceRefusal>;

    /// What `request` would do to the repository, told before it starts: a
    /// read of the repository that takes no maintenance slot and writes
    /// nothing. A forget's and an adoption's preview names the selection a
    /// start can carry.
    async fn preview(
        &self,
        repository: String,
        request: MaintenanceRequest,
    ) -> Result<api::MaintenancePreviewDto, MaintenanceRefusal>;

    /// The repository's snapshots grouped by who owns them, read live like
    /// a snapshot listing: the history the maintenance runs act on.
    async fn groups(
        &self,
        repository: String,
    ) -> Result<api::SnapshotGroupsDto, MaintenanceRefusal>;

    /// The repository's stats as last read and kept (§16, decision 104): a
    /// read of the history, never of the repository.
    async fn stats(
        &self,
        repository: String,
    ) -> Result<api::RepositoryStatsDto, MaintenanceRefusal>;

    /// Reads the repository's stats now and keeps the reading: a read of the
    /// repository like the groups, which takes no maintenance slot, is
    /// refused for nothing that runs on it and writes nothing to the
    /// operation log.
    async fn read_stats(
        &self,
        repository: String,
    ) -> Result<api::StatsReadingDto, MaintenanceRefusal>;

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

    /// Cancels a running operation. Finished operations, unknown IDs and an
    /// operation `by` may not stop are typed refusals, not errors.
    async fn cancel(&self, id: String, by: Canceller) -> Result<(), CancelError>;
}

/// Who asks for a cancel: a request with a session (or any request, with
/// sign-in off), or an API token, which cancels backups only (§10.1,
/// decision 93).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canceller {
    Session,
    ApiToken,
}

/// Why a request couldn't start an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// No plan by that name is configured.
    UnknownPlan,
    /// The plan's repository has a secret that cannot be resolved, or a
    /// setting with a reference that is not valid: the refusal says which,
    /// as the repository routes and the snapshot listing tell it.
    SecretRefused(SecretRefusal),
    /// A plan may be there but cannot be started just now, because its
    /// configuration cannot be read or used: `code` is the `config-*` code
    /// a save of it is refused with, `args` the arguments it declares and
    /// `detail` the store's own words, as [`CatalogError::Unavailable`]
    /// carries them.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// The catalog the API asks of the daemon (§4.2): the plans the operations
/// page can start, the repositories the snapshots page can show, and one
/// repository's snapshots, fetched live — nothing stores them.
#[async_trait]
pub trait Catalog: Send + Sync {
    /// The configured plans, in the order they were written.
    async fn plans(&self) -> Result<api::PlansDto, CatalogError>;

    /// The configured repositories, in the order of their aliases.
    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError>;

    /// One repository's snapshots, newest first.
    async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError>;

    /// The page of the folder `folder` of the snapshot `snapshot`, in the
    /// repository `alias`, that starts at position `from` (§16, decision
    /// 102), read live.
    async fn folder(
        &self,
        alias: String,
        snapshot: domain::SnapshotId,
        folder: domain::browse::FolderPath,
        from: u64,
    ) -> Result<api::FolderDto, FolderError>;
}

/// Why a catalog request couldn't be answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// The configuration cannot be read or used, so nothing is known: `code`
    /// is the `config-*` code a save of it is refused with, `args` the
    /// arguments it declares (a mistake's file and line), and `detail` the
    /// store's own words (the setting and what is wrong with it), which
    /// never hold a value of the file (I4).
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// Why a snapshot listing couldn't be answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotsError {
    /// The configuration has no repository by that name.
    Unknown,
    /// The repository's secrets cannot be resolved, or a setting with a
    /// reference is not valid: the refusal says which.
    SecretRefused(SecretRefusal),
    /// No repository is at the location: it is a folder on this machine
    /// holding nothing of a repository (decision 53).
    RepositoryNotFound,
    /// No repository's config file was found at the location, and nothing
    /// tells whether it is empty or out of reach (#525). `detail` is the
    /// engine's own words, masked.
    RepositoryConfigNotFound { detail: String },
    /// The snapshots cannot be read just now: the configuration cannot be
    /// read or used (its `config-*` code, arguments and the store's words,
    /// as [`CatalogError::Unavailable`] carries them), or the engine cannot
    /// be started (`engine-unavailable`, its words only in the log).
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// Why a folder of a snapshot couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderError {
    /// Refused as a listing of the repository's snapshots would be: no
    /// repository by that name, a secret, or nothing to read it with.
    Repository(SnapshotsError),
    /// The repository holds no snapshot by that ID.
    SnapshotUnknown,
    /// The snapshot holds no folder at that path.
    FolderUnknown,
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
    /// An API token asked, and the operation is not a backup: only a
    /// session may stop it.
    SessionOnly,
}

/// The body `POST /api/v1/operations` takes.
#[derive(Deserialize)]
struct RunPlanBody {
    plan: String,
}

/// What the same route answers: the started operation's ID, unenveloped.
#[derive(Serialize)]
pub(crate) struct StartedOperation {
    pub(crate) operation: String,
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
/// history, one operation, cancel — the catalog routes the pages read: the
/// plans, the repositories, one repository's snapshots — the routes that
/// maintain a repository: its history by owner, a preview of each run and
/// the start of one — the routes that test, connect and initialise a
/// repository, the routes that read, create, change and delete a plan, the
/// session route that signs in and out, the route that signs out everywhere,
/// the sign-in settings' route, and the route that sets the admin password. A
/// request that fails `checks` never reaches a
/// route; with sign-in on, neither does an API request without a live
/// session, or a changed one without its CSRF token; and neither does one
/// whose body is over the limit of its class of route (§10.1). Every
/// response, a refusal included, carries the security headers. Serve it
/// with `into_make_service_with_connect_info::<PeerAddr>()`
/// through a [`GatedListener`]: with sign-in on, a sign-in without its
/// connection's address is refused.
pub fn router(assets: Arc<dyn AssetSource>, checks: RequestChecks, state: AppState) -> Router {
    let sign_in = state.sign_in.clone();
    Router::new()
        .route(
            sign_in::SESSION_PATH,
            axum::routing::get(sign_in::current)
                .post(sign_in::sign_in)
                .delete(sign_in::sign_out),
        )
        .route(
            sign_in::SESSIONS_PATH,
            axum::routing::delete(sign_in::sign_out_everywhere),
        )
        .route(
            sign_in::ADMIN_PASSWORD_PATH,
            axum::routing::post(sign_in::set_password),
        )
        .route(
            api_tokens::TOKENS_PATH,
            axum::routing::get(api_tokens::list).post(api_tokens::create),
        )
        .route(
            "/api/v1/tokens/{id}",
            axum::routing::delete(api_tokens::revoke),
        )
        .route(
            sign_in::settings::SETTINGS_PATH,
            axum::routing::get(sign_in::settings::read).put(sign_in::settings::change),
        )
        .route(sign_in::EVENTS_PATH, axum::routing::get(events))
        .route(
            "/api/v1/operations",
            axum::routing::get(list_operations).post(run_plan),
        )
        .route("/api/v1/log", axum::routing::get(recent_log))
        .route(
            full_disk_access::FULL_DISK_ACCESS_PATH,
            axum::routing::get(full_disk_access::read),
        )
        .route(
            full_disk_access::CHECK_PATH,
            axum::routing::post(full_disk_access::check),
        )
        .route(
            full_disk_access::SETTINGS_PATH,
            axum::routing::post(full_disk_access::open_settings),
        )
        .route("/api/v1/operations/{id}", axum::routing::get(get_operation))
        .route(
            "/api/v1/operations/{id}/cancel",
            axum::routing::post(cancel_operation),
        )
        .route(
            "/api/v1/plans",
            axum::routing::get(list_plans).post(plans::create),
        )
        .route(
            "/api/v1/plans/{id}",
            axum::routing::get(plans::read)
                .put(plans::change)
                .delete(plans::delete),
        )
        .route(
            "/api/v1/plans/{id}/profile",
            axum::routing::get(profiles::export),
        )
        .route(
            "/api/v1/proposals",
            axum::routing::get(proposals::list).post(proposals::propose),
        )
        .route(
            "/api/v1/proposals/{id}",
            axum::routing::get(proposals::read),
        )
        .route(
            "/api/v1/proposals/{id}/withdraw",
            axum::routing::post(proposals::withdraw),
        )
        // `body_limit` bounds this body by its own class, over the 2 MiB
        // axum's JSON extractor would otherwise allow.
        .route(
            body_limit::PROFILE_PREVIEW_PATH,
            axum::routing::post(profiles::preview).layer(axum::extract::DefaultBodyLimit::max(
                usize::try_from(body_limit::PROFILE_BODY_LIMIT).unwrap_or(usize::MAX),
            )),
        )
        .route(
            "/api/v1/repositories",
            axum::routing::get(list_repositories).post(repositories::connect_repository),
        )
        // Underscore-prefixed, so no alias (a slug) can ever be one of them.
        .route(
            "/api/v1/repositories/_test",
            axum::routing::post(repositories::test_repository),
        )
        .route(
            "/api/v1/repositories/_init",
            axum::routing::post(repositories::init_repository),
        )
        .route(
            "/api/v1/repositories/{alias}/init",
            axum::routing::post(repositories::create_saved_repository),
        )
        .route(
            "/api/v1/repositories/{alias}/test",
            axum::routing::post(repositories::test_saved_repository),
        )
        .route(
            "/api/v1/repositories/{alias}/snapshots",
            axum::routing::get(list_snapshots),
        )
        .route(
            "/api/v1/repositories/{alias}/snapshots/{snapshot}/folder",
            axum::routing::get(read_folder),
        )
        // For a session only: no API token scope opens either (§10.5).
        .route(
            "/api/v1/repositories/{alias}/snapshots/{snapshot}/restore",
            axum::routing::post(restores::start),
        )
        .route(
            "/api/v1/repositories/{alias}/snapshots/{snapshot}/restore/preview",
            axum::routing::post(restores::preview),
        )
        .route(
            "/api/v1/repositories/{alias}/groups",
            axum::routing::get(maintenance::groups),
        )
        .route(
            "/api/v1/repositories/{alias}/stats",
            axum::routing::get(maintenance::stats).post(maintenance::read_stats),
        )
        .route(
            "/api/v1/repositories/{alias}/maintenance",
            axum::routing::post(maintenance::start),
        )
        .route(
            "/api/v1/repositories/{alias}/maintenance/preview",
            axum::routing::post(maintenance::preview),
        )
        .route(
            "/api/v1/repositories/{alias}/maintainer",
            axum::routing::put(repositories::change_maintainer),
        )
        .route(
            restart::RESTART_PATH,
            axum::routing::get(restart::read)
                .post(restart::ask)
                .delete(restart::cancel),
        )
        .with_state(state)
        .fallback(move |method: Method, uri: Uri| {
            let assets = Arc::clone(&assets);
            async move { serve(assets.as_ref(), &method, &uri) }
        })
        .layer(from_fn(body_limit::enforce))
        .layer(from_fn_with_state(sign_in, sign_in::enforce))
        .layer(from_fn_with_state(checks, request_checks::enforce))
        .layer(from_fn(response_headers::add))
}

/// The most one message from an events client may hold. The UI sends pings,
/// which a control frame keeps to 125 bytes, and the library's default of
/// 64 MiB would let a client make the daemon buffer that much.
const EVENTS_CLIENT_MAX_MESSAGE: usize = 1024;

/// How often an events stream opened with a session or an API token asks
/// whether it is still live: a session that has ended (signed out, unused
/// too long, or the admin password set anew) or a token revoked stops its
/// stream within this long.
const SESSION_RECHECK: std::time::Duration = std::time::Duration::from_secs(5);

/// The live-updates WebSocket (technical design §4.4): one per tab, carrying
/// [`UiEvent`]s. It sends nothing until an event arrives — the first state
/// comes over REST — and the client is dropped when a send fails. The
/// request checks have already allowed this handshake: a foreign `Origin`
/// never reaches here. With sign-in on, the stream ends when the session or
/// the API token that opened it does.
async fn events(
    upgrade: WebSocketUpgrade,
    State(state): State<AppState>,
    session: Option<axum::Extension<sign_in::SessionOf>>,
    token: Option<axum::Extension<api_tokens::TokenOf>>,
) -> Response {
    let stream = state.events.subscribe().await;
    let opener = match (session, token) {
        (Some(axum::Extension(session)), _) => Some(Opener::Session(session)),
        (None, Some(axum::Extension(token))) => Some(Opener::Token(token)),
        (None, None) => None,
    };
    upgrade
        .max_message_size(EVENTS_CLIENT_MAX_MESSAGE)
        .max_frame_size(EVENTS_CLIENT_MAX_MESSAGE)
        .on_upgrade(move |socket| forward(socket, stream, opener))
}

/// What opened an events stream with sign-in on: a session, or an API token.
enum Opener {
    Session(sign_in::SessionOf),
    Token(api_tokens::TokenOf),
}

impl Opener {
    /// Whether the session is live still, or the token kept still (not
    /// revoked, and of the admin password kept now).
    async fn live(&self) -> bool {
        match self {
            Self::Session(session) => session.access.peek(&session.token).is_some(),
            Self::Token(token) => token.access.api_token_live(&token.grant.id).await,
        }
    }
}

/// Reads the bus and writes each event to the client, until the client goes,
/// the daemon does, or the session or token that opened the stream ends. A
/// client's `Ping` is answered with a `Pong`: the events stream never waits
/// on the client, so the reply races fairly with the next event.
async fn forward(
    mut socket: WebSocket,
    mut stream: BoxStream<'static, UiEvent>,
    opener: Option<Opener>,
) {
    let mut recheck = tokio::time::interval(SESSION_RECHECK);
    loop {
        tokio::select! {
            _ = recheck.tick(), if opener.is_some() => {
                if let Some(opener) = &opener
                    && !opener.live().await
                {
                    break;
                }
            }
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
/// reports arrives over the events WebSocket. A run an API token asked for
/// is recorded with that token's label and ID (decision 93); any other is a
/// run by hand.
async fn run_plan(
    State(state): State<AppState>,
    token: Option<axum::Extension<api_tokens::TokenOf>>,
    body: Result<axum::Json<RunPlanBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let plan = match body {
        Ok(axum::Json(RunPlanBody { plan })) => plan,
        Err(_) => return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody),
    };
    let trigger = match token {
        Some(axum::Extension(token)) => Trigger::ApiToken {
            id: token.grant.id,
            label: token.grant.label,
        },
        None => Trigger::Manual,
    };
    match state.operations.start_backup(plan.clone(), trigger).await {
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
        Err(StartError::SecretRefused(secret)) => repositories::secret_refusal(&secret),
        Err(StartError::Unavailable { code, args, detail }) => unavailable(code, args, detail),
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
                .map_or_default(|cursor| vec![("cursor", api::MsgArg::Text(cursor))]),
        ),
    }
}

/// The recent log: the daemon log's newest lines, newest first (#618).
async fn recent_log(State(state): State<AppState>) -> Response {
    let answer = app::recent_log::answer(state.recent_log.as_ref());
    (StatusCode::OK, axum::Json(answer)).into_response()
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
/// stops, the finally-hooks run, the operation ends cancelled. An API token
/// cancels backups only; anything else it is refused as needing a session.
async fn cancel_operation(
    State(state): State<AppState>,
    token: Option<axum::Extension<api_tokens::TokenOf>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Result<axum::Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if body.is_err() {
        return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody);
    }
    let by = match token {
        Some(_) => Canceller::ApiToken,
        None => Canceller::Session,
    };
    match state.operations.cancel(id.clone(), by).await {
        Err(CancelError::SessionOnly) => api_tokens::scope_refused(api_tokens::SESSION_SCOPE),
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
        Err(CatalogError::Unavailable { code, args, detail }) => unavailable(code, args, detail),
    }
}

/// The configured repositories: what the Repositories page lists and the
/// snapshots page can pick from.
async fn list_repositories(State(state): State<AppState>) -> Response {
    match state.catalog.repositories().await {
        Ok(repositories) => (StatusCode::OK, axum::Json(repositories)).into_response(),
        Err(CatalogError::Unavailable { code, args, detail }) => unavailable(code, args, detail),
    }
}

/// What the daemon can't answer just now, as the configuration or the engine
/// stands: 503 with the code, its arguments and the store's words. A
/// configuration is refused as a save of it is, wherever it is read.
fn unavailable(
    code: api::ErrorCode,
    args: Vec<(&'static str, api::MsgArg)>,
    detail: Option<String>,
) -> Response {
    refused_detailed(StatusCode::SERVICE_UNAVAILABLE, code, args, detail)
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
        Err(SnapshotsError::SecretRefused(secret)) => repositories::secret_refusal(&secret),
        Err(SnapshotsError::RepositoryNotFound) => no_repository(None),
        Err(SnapshotsError::RepositoryConfigNotFound { detail }) => no_repository(Some(detail)),
        Err(SnapshotsError::Unavailable { code, args, detail }) => unavailable(code, args, detail),
    }
}

/// The query the folder route takes: the folder's path in the snapshot, its
/// names joined by `/` (none for the top), and the position the page starts
/// at (none for the first). Both optional.
#[derive(Deserialize, Default)]
struct FolderParams {
    path: Option<String>,
    from: Option<String>,
}

/// One page of one folder of a snapshot, read live like the listing (§9).
/// The snapshot's ID and the folder's path are parsed here, where they
/// enter: one no snapshot or folder could have is as unknown as one the
/// snapshot happens not to hold.
async fn read_folder(
    State(state): State<AppState>,
    axum::extract::Path((alias, snapshot)): axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<FolderParams>,
) -> Response {
    let from = match params.from {
        None => 0,
        Some(cursor) => match cursor.parse::<u64>() {
            Ok(from) => from,
            Err(_) => {
                return refused_with(
                    StatusCode::BAD_REQUEST,
                    api::ErrorCode::InvalidCursor,
                    vec![("cursor", api::MsgArg::Text(cursor))],
                );
            }
        },
    };
    let Ok(id) = domain::SnapshotId::parse(&snapshot) else {
        return snapshot_unknown(snapshot);
    };
    let path = params.path.unwrap_or_default();
    let Ok(folder) = domain::browse::FolderPath::parse(&path) else {
        return folder_unknown(path);
    };
    match state.catalog.folder(alias.clone(), id, folder, from).await {
        Ok(page) => (StatusCode::OK, axum::Json(page)).into_response(),
        Err(FolderError::SnapshotUnknown) => snapshot_unknown(snapshot),
        Err(FolderError::FolderUnknown) => folder_unknown(path),
        Err(FolderError::Repository(SnapshotsError::Unknown)) => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::RepositoryUnknown,
            vec![("alias", api::MsgArg::Text(alias))],
        ),
        Err(FolderError::Repository(SnapshotsError::SecretRefused(secret))) => {
            repositories::secret_refusal(&secret)
        }
        Err(FolderError::Repository(SnapshotsError::RepositoryNotFound)) => no_repository(None),
        Err(FolderError::Repository(SnapshotsError::RepositoryConfigNotFound { detail })) => {
            no_repository(Some(detail))
        }
        Err(FolderError::Repository(SnapshotsError::Unavailable { code, args, detail })) => {
            unavailable(code, args, detail)
        }
    }
}

/// No repository at the repository's location, a configuration to correct
/// rather than a daemon that is unavailable: 404, as `repository-not-found`
/// for an empty folder, or as `repository-config-not-found` with the
/// engine's masked words, `detail`, when no config file was found (#525).
fn no_repository(detail: Option<String>) -> Response {
    let code = match detail {
        None => api::ErrorCode::RepositoryNotFound,
        Some(_) => api::ErrorCode::RepositoryConfigNotFound,
    };
    refused_detailed(StatusCode::NOT_FOUND, code, Vec::new(), detail)
}

/// The refusal of a snapshot the repository doesn't hold, naming it as the
/// request did.
fn snapshot_unknown(snapshot: String) -> Response {
    refused_with(
        StatusCode::NOT_FOUND,
        api::ErrorCode::SnapshotUnknown,
        vec![("snapshot", api::MsgArg::Text(snapshot))],
    )
}

/// The refusal of a folder the snapshot doesn't hold, naming it as the
/// request did.
fn folder_unknown(path: String) -> Response {
    refused_with(
        StatusCode::NOT_FOUND,
        api::ErrorCode::FolderUnknown,
        vec![("path", api::MsgArg::Text(path))],
    )
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
    refused_detailed(status, code, args, None)
}

/// A typed refusal that also carries the sender's own words, when it has
/// some: the engine's message, which the UI shows as it came.
fn refused_detailed(
    status: StatusCode,
    code: api::ErrorCode,
    args: Vec<(&'static str, api::MsgArg)>,
    detail: Option<String>,
) -> Response {
    (status, axum::Json(api::ErrorDto::new(code, args, detail))).into_response()
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
    // A file whose name carries its content hash gets a new name when it
    // changes, so it can be cached for good; any other, index.html included,
    // must be revalidated, or a new release hides behind the cached copy.
    let cache_control = HeaderValue::from_static(if carries_content_hash(path) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    });
    // The entry page starts the application with a script of its own, so its
    // policy names that script by its hash (§10.1).
    let policy = (path == INDEX).then(|| response_headers::page_policy(&asset.bytes));
    let mut response = (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(asset.bytes),
    )
        .into_response();
    if let Some(policy) = policy {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, policy);
    }
    response
}

/// Whether `path` names a file by its content, as `trunk build` names each
/// file it hashes: `<stem>-<hash>.<extension>`, or `<stem>-<hash>_bg.wasm`
/// for the module. The hash is 64 bits in lowercase hex: 16 digits, or fewer
/// for the script and the module, whose hash Trunk doesn't pad. Asking for at
/// least 8 keeps a name such as `icon-192.png` out; a hash under 8 digits
/// comes once in 2^36 builds and costs only revalidation.
fn carries_content_hash(path: &str) -> bool {
    let Some((_, tail)) = path.rsplit_once('-') else {
        return false;
    };
    let digits = tail
        .bytes()
        .take_while(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        .count();
    (8..=16).contains(&digits) && matches!(tail.as_bytes().get(digits), Some(b'.' | b'_'))
}

#[cfg(test)]
mod tests {
    mod hardening;

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
                snapshot: Some("cd".repeat(32)),
                bytes_processed: Some(65_536),
            }),
            None,
        )
        .unwrap()
    }

    /// The fake's reading of the stats of "demo", taken at `taken_at`.
    fn demo_reading(taken_at: &str) -> api::StatsReadingDto {
        api::StatsReadingDto {
            taken_at: taken_at.to_owned(),
            snapshots: 3,
            total_size: 3000,
            stored_size: 1200,
            compression_saved_percent: Some(50),
        }
    }

    /// The refusal the fake stands for `repository` with: `None` for a
    /// repository it answers.
    fn refusal_of(repository: &str) -> Option<MaintenanceRefusal> {
        let text = |value: &str| value.to_owned();
        Some(match repository {
            "missing" => MaintenanceRefusal::UnknownRepository,
            "not-owner" => MaintenanceRefusal::NotOwner,
            "busy" => MaintenanceRefusal::Busy {
                running: text("018f0000-0000-7000-8000-000000000003"),
            },
            "keeps-nothing" => MaintenanceRefusal::KeepsNothing { plan: text("home") },
            "unknown-plan" => MaintenanceRefusal::UnknownPlan {
                plan: text("nowhere"),
            },
            "no-match" => MaintenanceRefusal::AdoptionNoMatch,
            "ambiguous" => MaintenanceRefusal::AdoptionAmbiguous {
                plans: vec![text("home"), text("mirror")],
            },
            "bad-token" => MaintenanceRefusal::InvalidBody,
            "stale" => MaintenanceRefusal::SelectionChanged,
            "no-secret" => MaintenanceRefusal::SecretRefused(SecretRefusal {
                code: api::ErrorCode::SecretMissing,
                args: vec![
                    ("field", api::MsgArg::Text(text("password"))),
                    ("source", api::MsgArg::Text(text("PW"))),
                ],
                detail: None,
            }),
            "not-found" => MaintenanceRefusal::RepositoryNotFound,
            "no-config" => MaintenanceRefusal::RepositoryConfigNotFound {
                detail: text("No repository config file found for `/media/usb/restic`."),
            },
            "failed" => MaintenanceRefusal::OperationFailed {
                detail: text("wrong password or no repository at the location"),
            },
            "slow" => MaintenanceRefusal::TimedOut,
            "unreachable" => MaintenanceRefusal::Unavailable {
                code: api::ErrorCode::ConfigUnavailable,
                args: Vec::new(),
                detail: None,
            },
            "engine-down" => MaintenanceRefusal::Unavailable {
                code: api::ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: Some(text("no worker program")),
            },
            _ => return None,
        })
    }

    #[async_trait]
    impl Operations for Plans {
        async fn start_backup(
            &self,
            plan: String,
            _trigger: domain::execution::Trigger,
        ) -> Result<String, StartError> {
            match plan.as_str() {
                "missing" => Err(StartError::UnknownPlan),
                "no-secret" => Err(StartError::SecretRefused(SecretRefusal {
                    code: api::ErrorCode::SecretCommandFailed,
                    args: vec![
                        ("field", api::MsgArg::Text("password".to_owned())),
                        ("source", api::MsgArg::Text("pass".to_owned())),
                    ],
                    detail: Some("no such entry".to_owned()),
                })),
                "mistaken" => {
                    let (code, args, detail) = mistake();
                    Err(StartError::Unavailable { code, args, detail })
                }
                _ => Ok("018f0000-0000-7000-8000-000000000001".to_owned()),
            }
        }

        async fn start_maintenance(
            &self,
            repository: String,
            request: MaintenanceRequest,
            selection: Option<String>,
        ) -> Result<String, MaintenanceRefusal> {
            if let Some(refusal) = refusal_of(&repository) {
                return Err(refusal);
            }
            if repository != "demo" {
                return Err(MaintenanceRefusal::UnknownRepository);
            }
            let adopt = matches!(request, MaintenanceRequest::Adopt { .. });
            match (adopt, selection.as_deref()) {
                (false, None) => Ok("018f0000-0000-7000-8000-000000000002".to_owned()),
                (true, None) => Ok("018f0000-0000-7000-8000-000000000004".to_owned()),
                // The selection a preview named: the run goes ahead.
                (false, Some(SELECTION)) => Ok("018f0000-0000-7000-8000-000000000005".to_owned()),
                (true, Some(SELECTION)) => Ok("018f0000-0000-7000-8000-000000000006".to_owned()),
                // Any other: the repository has changed since.
                (_, Some(_)) => Err(MaintenanceRefusal::SelectionChanged),
            }
        }

        async fn preview(
            &self,
            repository: String,
            request: MaintenanceRequest,
        ) -> Result<api::MaintenancePreviewDto, MaintenanceRefusal> {
            if let Some(refusal) = refusal_of(&repository) {
                return Err(refusal);
            }
            if repository != "demo" {
                return Err(MaintenanceRefusal::UnknownRepository);
            }
            let shown = |snapshot: &api::SnapshotDto| Some(snapshot.clone());
            let snapshots = [demo_snapshot()];
            Ok(match request {
                MaintenanceRequest::Forget => api::MaintenancePreviewDto::Forget {
                    plans: vec![api::ForgetPlanDto::new(
                        "home".to_owned(),
                        3,
                        &snapshots,
                        shown,
                    )],
                    left_alone: api::LeftAloneDto::default(),
                    selection: SELECTION.to_owned(),
                    warnings: Vec::new(),
                },
                MaintenanceRequest::Adopt { into, .. } => {
                    api::MaintenancePreviewDto::adopt(into, &snapshots, SELECTION.to_owned(), shown)
                }
                MaintenanceRequest::Prune => api::MaintenancePreviewDto::Prune {
                    packs_marked: 2,
                    packs_deleted: 1,
                    packs_recovered: 0,
                    bytes_deleted: 4096,
                    keep_delete_hours: 144,
                    warnings: Vec::new(),
                },
                MaintenanceRequest::Check => api::MaintenancePreviewDto::Check { read_data: false },
            })
        }

        async fn groups(
            &self,
            repository: String,
        ) -> Result<api::SnapshotGroupsDto, MaintenanceRefusal> {
            if let Some(refusal) = refusal_of(&repository) {
                return Err(refusal);
            }
            if repository != "demo" {
                return Err(MaintenanceRefusal::UnknownRepository);
            }
            Ok(api::SnapshotGroupsDto {
                plans: vec![api::PlanGroupDto::new(
                    "home".to_owned(),
                    &[demo_snapshot()],
                    |snapshot| Some(snapshot.clone()),
                )],
                orphaned: Vec::new(),
                unmanaged: Vec::new(),
                warnings: Vec::new(),
            })
        }

        async fn stats(
            &self,
            repository: String,
        ) -> Result<api::RepositoryStatsDto, MaintenanceRefusal> {
            match refusal_of(&repository) {
                Some(refusal) => Err(refusal),
                // A repository never read has no reading.
                None if repository == "never-read" => Ok(api::RepositoryStatsDto { latest: None }),
                None if repository == "demo" => Ok(api::RepositoryStatsDto {
                    latest: Some(demo_reading("2026-11-14T09:00:00Z")),
                }),
                None => Err(MaintenanceRefusal::UnknownRepository),
            }
        }

        async fn read_stats(
            &self,
            repository: String,
        ) -> Result<api::StatsReadingDto, MaintenanceRefusal> {
            match refusal_of(&repository) {
                Some(refusal) => Err(refusal),
                None if repository == "demo" => Ok(demo_reading("2026-11-15T09:00:00Z")),
                None => Err(MaintenanceRefusal::UnknownRepository),
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

        async fn cancel(&self, id: String, _by: Canceller) -> Result<(), CancelError> {
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

    /// What the fake catalog says of the repository `empty` (an empty
    /// folder) and of `no-config` (no config file found).
    fn no_repository(alias: &str) -> SnapshotsError {
        match alias {
            "empty" => SnapshotsError::RepositoryNotFound,
            _ => SnapshotsError::RepositoryConfigNotFound {
                detail: "No repository config file found for `/media/usb/restic`.".to_owned(),
            },
        }
    }

    #[async_trait]
    impl Catalog for DemoCatalog {
        async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
            Ok(api::PlansDto {
                plans: vec![api::PlanSummaryDto {
                    id: "home".to_owned(),
                    repository: "demo".to_owned(),
                    scheduled: false,
                    next_run: None,
                    warnings: Vec::new(),
                }],
            })
        }

        async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
            Ok(api::RepositoriesDto {
                repositories: vec![api::RepositoryDto {
                    alias: "demo".to_owned(),
                    repository: "/srv/demo/repo".to_owned(),
                    repo_hot: None,
                    password: api::PasswordSourceDto::Inline,
                    maintained_here: true,
                    create_offered: false,
                    warnings: Vec::new(),
                    locations: Vec::new(),
                }],
            })
        }

        async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
            match alias.as_str() {
                "missing" => Err(SnapshotsError::Unknown),
                "engine-down" => Err(SnapshotsError::Unavailable {
                    code: api::ErrorCode::EngineUnavailable,
                    args: Vec::new(),
                    detail: None,
                }),
                "empty" | "no-config" => Err(no_repository(&alias)),
                _ => Ok(api::SnapshotsDto {
                    snapshots: vec![demo_snapshot()],
                }),
            }
        }

        /// The folder `source/docs` of the snapshot `ab…` holds one file of
        /// a folder of 1,001 entries; the top holds `source`. The page
        /// echoes the path and continues from where it started.
        async fn folder(
            &self,
            alias: String,
            snapshot: domain::SnapshotId,
            folder: domain::browse::FolderPath,
            from: u64,
        ) -> Result<api::FolderDto, FolderError> {
            match alias.as_str() {
                "missing" => return Err(FolderError::Repository(SnapshotsError::Unknown)),
                "empty" | "no-config" => {
                    return Err(FolderError::Repository(no_repository(&alias)));
                }
                "engine-down" => {
                    return Err(FolderError::Repository(SnapshotsError::Unavailable {
                        code: api::ErrorCode::EngineUnavailable,
                        args: Vec::new(),
                        detail: None,
                    }));
                }
                "no-secret" => {
                    return Err(FolderError::Repository(SnapshotsError::SecretRefused(
                        SecretRefusal {
                            code: api::ErrorCode::SecretMissing,
                            args: vec![
                                ("field", api::MsgArg::Text("password".to_owned())),
                                ("source", api::MsgArg::Text("PW".to_owned())),
                            ],
                            detail: None,
                        },
                    )));
                }
                _ => {}
            }
            if snapshot.as_str() != "ab".repeat(32) {
                return Err(FolderError::SnapshotUnknown);
            }
            let entry = |name: &str, kind, size| api::EntryDto {
                name: name.to_owned(),
                display: name.to_owned(),
                kind,
                size,
                modified: Some("2026-11-01T12:00:00Z".to_owned()),
            };
            let (entries, total) = match folder.to_string().as_str() {
                "" => (vec![entry("source", api::EntryKindDto::Folder, None)], 1),
                "source/docs" => (
                    vec![entry("notes.md", api::EntryKindDto::File, Some(5))],
                    1_001,
                ),
                _ => return Err(FolderError::FolderUnknown),
            };
            let next = from.checked_add(1_000).filter(|next| *next < total);
            Ok(api::FolderDto {
                path: folder.to_string(),
                entries,
                total,
                next,
            })
        }
    }

    fn state() -> AppState {
        AppState {
            events: Arc::new(InMemoryEventBus::new()),
            operations: Arc::new(Plans),
            catalog: Arc::new(DemoCatalog),
            repositories: Arc::new(Repos::default()),
            plans: Arc::new(crate::request_checks::tests::NoPlans),
            profiles: Arc::new(crate::request_checks::tests::NoProfiles),
            restores: Arc::new(crate::request_checks::tests::NoRestores),
            proposals: Arc::new(crate::request_checks::tests::NoProposals),
            sign_in: crate::request_checks::tests::sign_in_off(),
            exposure: Exposure::Loopback,
            build: crate::request_checks::tests::test_build(),
            restart: Restart::default(),
            updates: Arc::default(),
            recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
            full_disk_access: FullDiskAccessRoute::default(),
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
        let loopback = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
        let listener = GatedListener::bind(loopback).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let events = Arc::new(InMemoryEventBus::new());
        let state = AppState {
            events: events.clone(),
            operations: Arc::new(Plans),
            catalog: Arc::new(DemoCatalog),
            repositories: Arc::new(Repos::default()),
            plans: Arc::new(crate::request_checks::tests::NoPlans),
            profiles: Arc::new(crate::request_checks::tests::NoProfiles),
            restores: Arc::new(crate::request_checks::tests::NoRestores),
            proposals: Arc::new(crate::request_checks::tests::NoProposals),
            sign_in: crate::request_checks::tests::sign_in_off(),
            exposure: Exposure::Loopback,
            build: crate::request_checks::tests::test_build(),
            restart: Restart::default(),
            updates: Arc::default(),
            recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
            full_disk_access: FullDiskAccessRoute::default(),
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

    /// A secret of the plan's repository that can't be resolved is refused
    /// with its own code, its arguments and what the source said, as the
    /// repository routes refuse it: not as a configuration that can't be
    /// read (#309).
    #[tokio::test]
    async fn a_plan_whose_secret_cannot_be_resolved_is_refused_with_the_secret_s_code() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/operations",
            r#"{"plan":"no-secret"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
            serde_json::json!({
                "code": "secret-command-failed",
                "args": {
                    "field": {"type": "text", "value": "password"},
                    "source": {"type": "text", "value": "pass"},
                },
                "detail": "no such entry",
            })
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
    async fn a_forget_that_names_a_plan_to_adopt_into_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"forget","into":"home"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await, r#"{"code":"invalid-body"}"#);
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
    async fn an_adoption_is_accepted_with_its_operation() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"adopt","into":"home","from":{"from":"unmanaged"}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            body(response).await,
            r#"{"operation":"018f0000-0000-7000-8000-000000000004"}"#
        );
    }

    #[tokio::test]
    async fn an_adoption_that_matches_no_plan_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/no-match/maintenance",
            r#"{"kind":"adopt","into":"home","from":{"from":"unmanaged"}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body(response).await, r#"{"code":"adoption-no-match"}"#);
    }

    #[tokio::test]
    async fn an_adoption_two_plans_match_is_refused_with_both() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/ambiguous/maintenance",
            r#"{"kind":"adopt","into":"home","from":{"from":"unmanaged"}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body(response).await,
            r#"{"code":"adoption-ambiguous","args":{"plans":{"type":"text","value":"home, mirror"}}}"#
        );
    }

    #[tokio::test]
    async fn an_adoption_without_its_group_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"adopt","into":"home"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await, r#"{"code":"invalid-body"}"#);
    }

    #[tokio::test]
    async fn a_repository_that_cannot_be_read_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/unreachable/maintenance",
            r#"{"kind":"forget"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body(response).await, r#"{"code":"config-unavailable"}"#);
    }

    /// The selection every preview of the fake names.
    const SELECTION: &str = "0123456789abcdef";

    #[tokio::test]
    async fn a_repositorys_groups_are_listed_by_owner() {
        let response = get("/api/v1/repositories/demo/groups").await;
        assert_eq!(response.status(), StatusCode::OK);
        let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(wire["plans"][0]["plan"], "home");
        assert_eq!(wire["plans"][0]["count"], 1);
        assert_eq!(wire["plans"][0]["snapshots"][0]["id"], "ab".repeat(32));
        assert_eq!(wire["orphaned"], serde_json::json!([]));
        assert_eq!(wire["unmanaged"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn a_repositorys_stats_are_the_reading_kept_last_or_none() {
        use serde_json::json;
        for (alias, expected) in [
            (
                "demo",
                json!({"latest": {
                    "taken-at": "2026-11-14T09:00:00Z",
                    "snapshots": 3,
                    "total-size": 3000,
                    "stored-size": 1200,
                    "compression-saved-percent": 50,
                }}),
            ),
            ("never-read", json!({})),
        ] {
            let response = get(&format!("/api/v1/repositories/{alias}/stats")).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::OK, "{alias}");
            assert_eq!(answer, expected, "{alias}");
        }
    }

    #[tokio::test]
    async fn reading_a_repositorys_stats_answers_the_new_reading() {
        let response = request(ui(), Method::POST, "/api/v1/repositories/demo/stats", "{}").await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer["taken-at"], "2026-11-15T09:00:00Z");
        assert_eq!(answer["snapshots"], 3);
    }

    #[tokio::test]
    async fn a_read_of_the_stats_with_a_body_that_is_not_json_is_refused() {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/stats",
            "not json",
        )
        .await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(answer, serde_json::json!({"code": "invalid-body"}));
    }

    #[tokio::test]
    async fn each_preview_names_its_kind_and_what_the_run_would_do() {
        use serde_json::json;
        let snapshot = serde_json::to_value(demo_snapshot()).unwrap();
        for (request_body, expected) in [
            (
                r#"{"kind":"forget"}"#,
                json!({
                    "kind": "forget",
                    "plans": [{"plan": "home", "total": 3, "remove": 1, "listed": [snapshot]}],
                    "left-alone": {
                        "orphaned": {"groups": 0, "snapshots": 0},
                        "unmanaged": {"groups": 0, "snapshots": 0},
                    },
                    "selection": SELECTION,
                }),
            ),
            (
                r#"{"kind":"adopt","into":"home","from":{"from":"orphaned","agent":"a","plan":"p"}}"#,
                json!({
                    "kind": "adopt",
                    "into": "home",
                    "count": 1,
                    "listed": [snapshot],
                    "selection": SELECTION,
                }),
            ),
            (
                r#"{"kind":"prune"}"#,
                json!({
                    "kind": "prune",
                    "packs-marked": 2,
                    "packs-deleted": 1,
                    "packs-recovered": 0,
                    "bytes-deleted": 4096,
                    "keep-delete-hours": 144,
                }),
            ),
            (
                r#"{"kind":"check"}"#,
                json!({"kind": "check", "read-data": false}),
            ),
        ] {
            let response = request(
                ui(),
                Method::POST,
                "/api/v1/repositories/demo/maintenance/preview",
                request_body,
            )
            .await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::OK, "{request_body}");
            assert_eq!(answer, expected, "{request_body}");
        }
    }

    #[tokio::test]
    async fn a_start_hands_the_selection_it_carries_to_the_daemon() {
        use serde_json::json;
        for (request_body, status, expected) in [
            (
                r#"{"kind":"forget"}"#.to_owned(),
                202,
                json!({"operation": "018f0000-0000-7000-8000-000000000002"}),
            ),
            (
                format!(r#"{{"kind":"forget","selection":"{SELECTION}"}}"#),
                202,
                json!({"operation": "018f0000-0000-7000-8000-000000000005"}),
            ),
            (
                format!(
                    r#"{{"kind":"adopt","into":"home","from":{{"from":"unmanaged"}},"selection":"{SELECTION}"}}"#
                ),
                202,
                json!({"operation": "018f0000-0000-7000-8000-000000000006"}),
            ),
            (
                r#"{"kind":"forget","selection":"ffffffffffffffff"}"#.to_owned(),
                409,
                json!({"code": "maintenance-selection-changed"}),
            ),
        ] {
            let response = request(
                ui(),
                Method::POST,
                "/api/v1/repositories/demo/maintenance",
                &request_body,
            )
            .await;
            let (actual, answer) = json_of(response).await;
            assert_eq!(actual.as_u16(), status, "{request_body}");
            assert_eq!(answer, expected, "{request_body}");
        }
    }

    #[tokio::test]
    async fn every_maintenance_refusal_is_typed_with_the_status_of_its_kind() {
        use serde_json::json;
        let text = |value: &str| json!({"type": "text", "value": value});
        for (alias, status, expected) in [
            (
                "missing",
                404,
                json!({"code": "repository-unknown", "args": {"alias": text("missing")}}),
            ),
            (
                "unknown-plan",
                404,
                json!({"code": "plan-unknown", "args": {"plan": text("nowhere")}}),
            ),
            (
                "busy",
                409,
                json!({
                    "code": "maintenance-busy",
                    "args": {"operation": text("018f0000-0000-7000-8000-000000000003")},
                }),
            ),
            (
                "stale",
                409,
                json!({"code": "maintenance-selection-changed"}),
            ),
            (
                "keeps-nothing",
                422,
                json!({"code": "retention-keeps-nothing", "args": {"plan": text("home")}}),
            ),
            ("no-match", 422, json!({"code": "adoption-no-match"})),
            (
                "ambiguous",
                422,
                json!({"code": "adoption-ambiguous", "args": {"plans": text("home, mirror")}}),
            ),
            (
                "no-secret",
                422,
                json!({"code": "secret-missing", "args": {"field": text("password"), "source": text("PW")}}),
            ),
            (
                "failed",
                422,
                json!({
                    "code": "operation-failed",
                    "detail": "wrong password or no repository at the location",
                }),
            ),
            ("not-found", 404, json!({"code": "repository-not-found"})),
            (
                "no-config",
                404,
                json!({
                    "code": "repository-config-not-found",
                    "detail": "No repository config file found for `/media/usb/restic`.",
                }),
            ),
            ("bad-token", 400, json!({"code": "invalid-body"})),
            ("slow", 504, json!({"code": "repository-read-timed-out"})),
            ("unreachable", 503, json!({"code": "config-unavailable"})),
            (
                "engine-down",
                503,
                json!({"code": "engine-unavailable", "detail": "no worker program"}),
            ),
        ] {
            // The groups, the stats, a preview and a start refuse alike.
            for (method, route, request_body) in [
                (
                    Method::GET,
                    format!("/api/v1/repositories/{alias}/groups"),
                    "",
                ),
                (
                    Method::GET,
                    format!("/api/v1/repositories/{alias}/stats"),
                    "",
                ),
                (
                    Method::POST,
                    format!("/api/v1/repositories/{alias}/stats"),
                    "{}",
                ),
                (
                    Method::POST,
                    format!("/api/v1/repositories/{alias}/maintenance/preview"),
                    r#"{"kind":"forget"}"#,
                ),
                (
                    Method::POST,
                    format!("/api/v1/repositories/{alias}/maintenance"),
                    r#"{"kind":"forget"}"#,
                ),
            ] {
                let response = request(ui(), method, &route, request_body).await;
                let (actual, answer) = json_of(response).await;
                assert_eq!(actual.as_u16(), status, "{alias} on {route}");
                assert_eq!(answer, expected, "{alias} on {route}");
            }
        }
    }

    #[tokio::test]
    async fn maintenance_of_a_repository_this_agent_does_not_maintain_is_a_conflict_naming_it() {
        use serde_json::json;
        let expected = json!({
            "code": "maintenance-not-owner",
            "args": {"alias": {"type": "text", "value": "not-owner"}},
        });
        for route in [
            "/api/v1/repositories/not-owner/maintenance/preview",
            "/api/v1/repositories/not-owner/maintenance",
        ] {
            for kind in ["forget", "prune", "check"] {
                let body = format!(r#"{{"kind":"{kind}"}}"#);
                let response = request(ui(), Method::POST, route, &body).await;
                let (status, answer) = json_of(response).await;
                assert_eq!(status.as_u16(), 409, "{kind} on {route}");
                assert_eq!(answer, expected, "{kind} on {route}");
            }
        }
    }

    #[tokio::test]
    async fn a_maintenance_request_that_takes_other_fields_is_refused_before_the_daemon_sees_it() {
        // The `demo` repository answers every request it is given, so each
        // refusal here is the route's own.
        const PREVIEW: &str = "/api/v1/repositories/demo/maintenance/preview";
        const START: &str = "/api/v1/repositories/demo/maintenance";
        let adopt = format!(
            r#"{{"kind":"adopt","into":"home","from":{{"from":"unmanaged"}},"selection":"{SELECTION}"}}"#
        );
        let selected = |kind: &str| format!(r#"{{"kind":"{kind}","selection":"{SELECTION}"}}"#);
        for (what, route, request_body) in [
            ("a selection on a prune", START, selected("prune")),
            ("a selection on a check", START, selected("check")),
            (
                "a selection on a forget preview",
                PREVIEW,
                selected("forget"),
            ),
            ("a selection on an adoption preview", PREVIEW, adopt),
            (
                "a key it does not know",
                START,
                r#"{"kind":"forget","selectoin":"0123456789abcdef"}"#.to_owned(),
            ),
            (
                "a key a preview does not know",
                PREVIEW,
                r#"{"kind":"forget","extra":1}"#.to_owned(),
            ),
            (
                "a forget that names a plan",
                PREVIEW,
                r#"{"kind":"forget","into":"home"}"#.to_owned(),
            ),
            (
                "a prune that names a group",
                PREVIEW,
                r#"{"kind":"prune","from":{"from":"unmanaged"}}"#.to_owned(),
            ),
            (
                "an adoption without its group",
                PREVIEW,
                r#"{"kind":"adopt","into":"home"}"#.to_owned(),
            ),
            (
                "an adoption without its plan",
                START,
                r#"{"kind":"adopt","from":{"from":"unmanaged"}}"#.to_owned(),
            ),
            (
                "a group of no kind",
                PREVIEW,
                r#"{"kind":"adopt","into":"home","from":{"from":"elsewhere"}}"#.to_owned(),
            ),
            ("a body that is not JSON", PREVIEW, "not json".to_owned()),
            ("an empty body", START, String::new()),
        ] {
            let response = request(ui(), Method::POST, route, &request_body).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{what} to {route}");
            assert_eq!(
                answer,
                serde_json::json!({"code": "invalid-body"}),
                "{what} to {route}"
            );
        }
        // A kind this version doesn't run is told as such, whatever else the
        // body holds.
        for (route, request_body) in [
            (PREVIEW, r#"{"kind":"wipe"}"#.to_owned()),
            (START, selected("wipe")),
        ] {
            let response = request(ui(), Method::POST, route, &request_body).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{request_body} to {route}");
            assert_eq!(
                answer,
                serde_json::json!({
                    "code": "maintenance-kind-unknown",
                    "args": {"kind": {"type": "text", "value": "wipe"}},
                }),
                "{request_body} to {route}"
            );
        }
    }

    #[tokio::test]
    async fn the_maintenance_routes_want_json_and_bound_their_bodies() {
        for route in [
            "/api/v1/repositories/demo/maintenance",
            "/api/v1/repositories/demo/maintenance/preview",
        ] {
            let post = |content_type: &'static str, request_body: String| {
                router(ui(), RequestChecks::default(), state()).oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri(route)
                        .header(header::HOST, "localhost:9898")
                        .header(header::CONTENT_TYPE, content_type)
                        .body(Body::from(request_body))
                        .unwrap(),
                )
            };
            let (status, answer) = json_of(
                post("text/plain", r#"{"kind":"forget"}"#.to_owned())
                    .await
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{route}");
            assert_eq!(answer["code"], "json-required", "{route}");
            let oversize = format!(r#"{{"kind":"{}"}}"#, "a".repeat(3 * 1024 * 1024));
            let (status, answer) = json_of(post("application/json", oversize).await.unwrap()).await;
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{route}");
            assert_eq!(answer["code"], "body-too-large", "{route}");
        }
    }

    #[tokio::test]
    async fn the_maintenance_routes_take_nothing_but_their_own_method() {
        for (method, route) in [
            (Method::GET, "/api/v1/repositories/demo/maintenance"),
            (Method::GET, "/api/v1/repositories/demo/maintenance/preview"),
            (Method::POST, "/api/v1/repositories/demo/groups"),
        ] {
            let response = request(ui(), method.clone(), route, "{}").await;
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {route}"
            );
        }
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
    async fn only_files_named_by_their_content_are_cached_for_good() {
        // Names as `trunk build` writes them: a content hash of 16 hex digits,
        // or fewer for the script and the module when the hash starts with
        // zeros, since Trunk doesn't pad theirs.
        let hashed = [
            "web-3fb4055f36b2f8e4.js",
            "web-10133c38bd221a27_bg.wasm",
            "web-fb4055f36b2f8e4.js",
            "web-fb405512.js",
            "style-d60c77a651c609fd.css",
            "favicon-31b6d255c3714cba.svg",
            "favicon-32-73b3bd9fb011307a.png",
        ];
        let unhashed = [
            "favicon.svg",
            "favicon-32.png",
            "icon-maskable-512.png",
            "web-fb40551.js",
            "web-03fb4055f36b2f8e4.js",
            "icon-deadbeefxl.png",
            "snippets/web-a1b2c3d4e5f60718/inline0.js",
        ];
        let assets: Arc<dyn AssetSource> = Arc::new(Files(
            hashed
                .iter()
                .chain(&unhashed)
                .map(|name| (*name, "x"))
                .collect(),
        ));
        for name in hashed {
            let response = request(Arc::clone(&assets), Method::GET, &format!("/{name}"), "").await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable",
                "{name}: a new build gives a changed file a new name"
            );
        }
        for name in unhashed {
            let response = request(Arc::clone(&assets), Method::GET, &format!("/{name}"), "").await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "no-cache",
                "{name}: a file that keeps its name across builds must be revalidated"
            );
        }
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
            r#"{"plans":[{"id":"home","repository":"demo","scheduled":false,"next-run":null}]}"#
        );

        let repositories = get("/api/v1/repositories").await;
        assert_eq!(repositories.status(), StatusCode::OK);
        assert_eq!(
            body(repositories).await,
            r#"{"repositories":[{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintained-here":true}]}"#
        );
    }

    /// A catalog whose configuration has a mistake on line 6: both lists and
    /// every repository's snapshots are refused as a save of it is.
    struct MistakenCatalog;

    /// The code, the arguments and the detail a mistake on line 6 of the
    /// configuration is refused with.
    fn mistake() -> (
        api::ErrorCode,
        Vec<(&'static str, api::MsgArg)>,
        Option<String>,
    ) {
        (
            api::ErrorCode::ConfigInvalid,
            vec![
                (
                    "file",
                    api::MsgArg::Text("/etc/ladderback/config.toml".to_owned()),
                ),
                ("line", api::MsgArg::Text("6".to_owned())),
            ],
            Some("repositories.demo.colour".to_owned()),
        )
    }

    #[async_trait]
    impl Catalog for MistakenCatalog {
        async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
            let (code, args, detail) = mistake();
            Err(CatalogError::Unavailable { code, args, detail })
        }

        async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
            let (code, args, detail) = mistake();
            Err(CatalogError::Unavailable { code, args, detail })
        }

        async fn snapshots(&self, _alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
            let (code, args, detail) = mistake();
            Err(SnapshotsError::Unavailable { code, args, detail })
        }

        async fn folder(
            &self,
            _alias: String,
            _snapshot: domain::SnapshotId,
            _folder: domain::browse::FolderPath,
            _from: u64,
        ) -> Result<api::FolderDto, FolderError> {
            let (code, args, detail) = mistake();
            Err(FolderError::Repository(SnapshotsError::Unavailable {
                code,
                args,
                detail,
            }))
        }
    }

    /// Every route that reads the configuration refuses a mistake in it with
    /// its code, its file, its line and its setting (#347). The maintenance
    /// routes carry theirs through `MaintenanceRefusal::Unavailable`, tested
    /// with the other refusals of maintenance.
    #[tokio::test]
    async fn a_configuration_that_cannot_be_used_is_refused_with_its_code_and_where_it_is() {
        for (method, path, sent) in [
            (Method::GET, "/api/v1/plans", ""),
            (Method::GET, "/api/v1/repositories", ""),
            (Method::GET, "/api/v1/repositories/demo/snapshots", ""),
            (
                Method::GET,
                "/api/v1/repositories/demo/snapshots/abababababababababababababababababababababababababababababababab/folder",
                "",
            ),
            (Method::POST, "/api/v1/operations", r#"{"plan":"mistaken"}"#),
        ] {
            let state = AppState {
                catalog: Arc::new(MistakenCatalog),
                ..state()
            };
            let response = router(ui(), RequestChecks::default(), state)
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::HOST, "localhost:9898")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(sent))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
            let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
            assert_eq!(
                wire,
                serde_json::json!({
                    "code": "config-invalid",
                    "args": {
                        "file": {"type": "text", "value": "/etc/ladderback/config.toml"},
                        "line": {"type": "text", "value": "6"},
                    },
                    "detail": "repositories.demo.colour",
                }),
                "{path}"
            );
        }
    }

    /// An engine that can't be started is told by its code alone: its words
    /// may hold a location, and go to the log.
    #[tokio::test]
    async fn a_listing_the_engine_cannot_start_is_refused_with_its_code_alone() {
        let response = get("/api/v1/repositories/engine-down/snapshots").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body(response).await, r#"{"code":"engine-unavailable"}"#);
    }

    /// No repository at the location is a configuration to correct, not a
    /// daemon that is unavailable: a listing and a folder read are refused
    /// as 404, an empty folder as `repository-not-found` and a location with
    /// no config file as `repository-config-not-found` with the engine's
    /// masked words (#525).
    #[tokio::test]
    async fn a_listing_or_folder_read_of_no_repository_is_refused_as_not_found() {
        for (alias, expected) in [
            ("empty", r#"{"code":"repository-not-found"}"#),
            (
                "no-config",
                r#"{"code":"repository-config-not-found","detail":"No repository config file found for `/media/usb/restic`."}"#,
            ),
        ] {
            for route in [
                format!("/api/v1/repositories/{alias}/snapshots"),
                folder_route(alias, ""),
            ] {
                let response = get(&route).await;
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{route}");
                assert_eq!(body(response).await, expected, "{route}");
            }
        }
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

    /// The snapshot every folder test opens.
    fn folder_route(alias: &str, query: &str) -> String {
        format!(
            "/api/v1/repositories/{alias}/snapshots/{}/folder{query}",
            "ab".repeat(32)
        )
    }

    /// A page of a folder carries its path, its entries with their kind,
    /// size and modification time, the folder's total and where the next
    /// page starts, from the position the request named.
    #[tokio::test]
    async fn a_page_of_a_snapshot_s_folder_starts_where_the_request_says() {
        let response = get(&folder_route("demo", "?path=source/docs&from=0")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "path": "source/docs",
                "entries": [{
                    "name": "notes.md",
                    "display": "notes.md",
                    "kind": "file",
                    "size": 5,
                    "modified": "2026-11-01T12:00:00Z",
                }],
                "total": 1001,
                "next": 1000,
            })
        );
        let last = get(&folder_route("demo", "?path=source/docs&from=1000")).await;
        assert_eq!(last.status(), StatusCode::OK);
        let wire: serde_json::Value = serde_json::from_str(&body(last).await).unwrap();
        assert_eq!(wire.get("next"), None, "the last page names no next");
    }

    /// No path is the snapshot's top folder, and no position its first page.
    #[tokio::test]
    async fn a_folder_request_naming_nothing_opens_the_top_folder_s_first_page() {
        let response = get(&folder_route("demo", "")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(wire["path"], "");
        assert_eq!(wire["entries"][0]["name"], "source");
        assert_eq!(wire["entries"][0]["kind"], "folder");
        assert_eq!(wire["entries"][0].get("size"), None);
    }

    /// A position that is no count of entries is refused as an invalid
    /// cursor, naming it.
    #[tokio::test]
    async fn a_folder_position_that_is_no_count_is_an_invalid_cursor() {
        for from in ["abc", "-1", "1.5", "18446744073709551616"] {
            let response = get(&folder_route("demo", &format!("?from={from}"))).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{from}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
                serde_json::json!({
                    "code": "invalid-cursor",
                    "args": {"cursor": {"type": "text", "value": from}},
                }),
                "{from}"
            );
        }
    }

    /// An ID no snapshot could have is as unknown as one the repository
    /// doesn't hold; both are named.
    #[tokio::test]
    async fn a_snapshot_the_repository_does_not_hold_is_a_typed_refusal() {
        for snapshot in ["nope".to_owned(), "cd".repeat(32)] {
            let response = get(&format!(
                "/api/v1/repositories/demo/snapshots/{snapshot}/folder"
            ))
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{snapshot}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
                serde_json::json!({
                    "code": "snapshot-unknown",
                    "args": {"snapshot": {"type": "text", "value": snapshot}},
                }),
            );
        }
    }

    /// A path no folder could have (an empty name) is as unknown as one the
    /// snapshot doesn't hold; both are named as they were asked for.
    #[tokio::test]
    async fn a_folder_the_snapshot_does_not_hold_is_a_typed_refusal() {
        for (query, path) in [
            ("?path=source//docs", "source//docs"),
            ("?path=/source", "/source"),
            ("?path=source/gone", "source/gone"),
        ] {
            let response = get(&folder_route("demo", query)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
                serde_json::json!({
                    "code": "folder-unknown",
                    "args": {"path": {"type": "text", "value": path}},
                }),
            );
        }
    }

    /// The repository is refused as its listing would be: unknown, a secret
    /// that can't be resolved, or an engine that can't start.
    #[tokio::test]
    async fn a_folder_of_a_repository_that_cannot_be_read_is_refused_as_its_listing_is() {
        let missing = get(&folder_route("missing", "")).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(missing).await,
            r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
        );
        let secret = get(&folder_route("no-secret", "")).await;
        assert_eq!(secret.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let wire: serde_json::Value = serde_json::from_str(&body(secret).await).unwrap();
        assert_eq!(wire["code"], "secret-missing");
        let down = get(&folder_route("engine-down", "")).await;
        assert_eq!(down.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body(down).await, r#"{"code":"engine-unavailable"}"#);
    }

    /// Answers by the location a request names, and counts the requests
    /// that reach it.
    #[derive(Default)]
    struct Repos {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl Repos {
        /// The one answer a location stands for: `Ok` for any other.
        fn answer(&self, settings: &api::RepositorySettingsDto) -> Result<(), RepositoriesError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match settings.repository.as_str() {
                "/invalid" => Err(RepositoriesError::InvalidBody),
                "/alias-invalid" => Err(RepositoriesError::AliasInvalid),
                "/alias-taken" => Err(RepositoriesError::AliasTaken {
                    alias: "demo".to_owned(),
                }),
                "/already-configured" => Err(RepositoriesError::AlreadyConfigured {
                    alias: "demo".to_owned(),
                }),
                "/exists" => Err(RepositoriesError::Exists {
                    location: "/exists".to_owned(),
                }),
                "/not-found" => Err(RepositoriesError::NotFound),
                "/config-not-found" => Err(RepositoriesError::ConfigNotFound {
                    detail: "No repository config file found for `/media/usb/restic`.".to_owned(),
                }),
                "/relative" => Err(RepositoriesError::PathRelative {
                    field: "repo-hot".to_owned(),
                }),
                "/secret" => Err(RepositoriesError::SecretRefused(SecretRefusal {
                    code: api::ErrorCode::SecretMissing,
                    args: vec![
                        ("field", api::MsgArg::Text("password".to_owned())),
                        ("source", api::MsgArg::Text("PW".to_owned())),
                    ],
                    detail: None,
                })),
                "/failed" => Err(RepositoriesError::OperationFailed {
                    detail: "wrong password or no repository at the location".to_owned(),
                }),
                "/unavailable" => Err(RepositoriesError::Unavailable {
                    code: api::ErrorCode::EngineUnavailable,
                    args: Vec::new(),
                    detail: Some("no worker program".to_owned()),
                }),
                "/config" => Err(RepositoriesError::Unavailable {
                    code: api::ErrorCode::ConfigUnavailable,
                    args: vec![("file", api::MsgArg::Text("/etc/config.toml".to_owned()))],
                    detail: Some("permission denied".to_owned()),
                }),
                _ => Ok(()),
            }
        }
    }

    #[async_trait]
    impl Repositories for Repos {
        async fn test(
            &self,
            request: api::TestRepositoryDto,
        ) -> Result<api::RepositoryIdDto, RepositoriesError> {
            self.answer(&request.settings)?;
            Ok(api::RepositoryIdDto {
                id: format!("test:{}", request.settings.repository),
            })
        }

        async fn connect(
            &self,
            request: api::ConnectRepositoryDto,
        ) -> Result<api::RepositoryIdDto, RepositoriesError> {
            self.answer(&request.settings)?;
            Ok(api::RepositoryIdDto {
                id: format!(
                    "{}:{}:{}",
                    request.alias,
                    request.settings.repository,
                    request.settings.repo_hot.as_deref().unwrap_or("-")
                ),
            })
        }

        async fn init(
            &self,
            request: api::InitRepositoryDto,
        ) -> Result<api::RepositoryInitDto, RepositoriesError> {
            self.answer(&request.settings)?;
            Ok(api::RepositoryInitDto {
                id: format!("init:{}", request.settings.repository),
                saved: request.alias.is_some(),
            })
        }

        async fn create_saved(
            &self,
            alias: String,
        ) -> Result<api::RepositoryIdDto, RepositoriesError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match alias.as_str() {
                "missing" => Err(RepositoriesError::Unknown { alias }),
                "exists" => Err(RepositoriesError::Exists {
                    location: "/srv/exists".to_owned(),
                }),
                "not-offered" => Err(RepositoriesError::CreateNotOffered),
                _ => Ok(api::RepositoryIdDto {
                    id: format!("created:{alias}"),
                }),
            }
        }

        async fn test_saved(
            &self,
            alias: String,
        ) -> Result<api::LocationsTestDto, RepositoriesError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match alias.as_str() {
                "missing" => Err(RepositoriesError::Unknown { alias }),
                _ => Ok(api::LocationsTestDto {
                    locations: vec![api::LocationTriedDto {
                        position: 1,
                        address: format!("/srv/{alias}"),
                        outcome: "used".to_owned(),
                        source: None,
                    }],
                }),
            }
        }

        async fn change_maintainer(
            &self,
            alias: String,
            request: api::MaintainerDto,
        ) -> Result<(), RepositoriesError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match (alias.as_str(), request.maintain_here) {
                ("missing", _) => Err(RepositoriesError::Unknown { alias }),
                ("busy", _) => Err(RepositoriesError::MaintenanceBusy {
                    operation: BUSY_OPERATION.to_owned(),
                }),
                ("demo", true) | ("stopped", false) => Ok(()),
                _ => Err(RepositoriesError::InvalidBody),
            }
        }
    }

    const TEST: &str = "/api/v1/repositories/_test";
    const CONNECT: &str = "/api/v1/repositories";
    const INIT: &str = "/api/v1/repositories/_init";
    /// The maintenance operation the fake says runs on the repository `busy`.
    const BUSY_OPERATION: &str = "018f0000-0000-7000-8000-0000000000cc";

    /// A request body for `repository`, typed with a password.
    fn settings_json(repository: &str) -> serde_json::Value {
        serde_json::json!({
            "repository": repository,
            "password": {"source": "inline", "value": "correct-horse"},
        })
    }

    /// Posts `body` to `path` against a fake that counts what reaches it:
    /// the answer, and how many requests the fake took.
    async fn post_counted(path: &str, body: String) -> (Response, usize) {
        send_counted(Method::POST, path, body).await
    }

    /// As [`post_counted`], with the request's `method`.
    async fn send_counted(method: Method, path: &str, body: String) -> (Response, usize) {
        let repos = Arc::new(Repos::default());
        let state = AppState {
            events: Arc::new(InMemoryEventBus::new()),
            operations: Arc::new(Plans),
            catalog: Arc::new(DemoCatalog),
            repositories: repos.clone(),
            plans: Arc::new(crate::request_checks::tests::NoPlans),
            profiles: Arc::new(crate::request_checks::tests::NoProfiles),
            restores: Arc::new(crate::request_checks::tests::NoRestores),
            proposals: Arc::new(crate::request_checks::tests::NoProposals),
            sign_in: crate::request_checks::tests::sign_in_off(),
            exposure: Exposure::Loopback,
            build: crate::request_checks::tests::test_build(),
            restart: Restart::default(),
            updates: Arc::default(),
            recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
            full_disk_access: FullDiskAccessRoute::default(),
        };
        let response = router(ui(), RequestChecks::default(), state)
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, "localhost:9898")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        (
            response,
            repos.calls.load(std::sync::atomic::Ordering::SeqCst),
        )
    }

    #[tokio::test]
    async fn turning_maintenance_of_a_saved_repository_on_or_off_answers_204_or_a_typed_refusal() {
        use serde_json::json;
        let route = |alias: &str| format!("/api/v1/repositories/{alias}/maintainer");
        for (alias, on) in [("demo", true), ("stopped", false)] {
            let request = json!({"maintain-here": on}).to_string();
            let (response, calls) = send_counted(Method::PUT, &route(alias), request).await;
            assert_eq!(calls, 1, "{alias}");
            assert_eq!(response.status(), StatusCode::NO_CONTENT, "{alias}");
            assert_eq!(body(response).await, "", "{alias}");
        }
        let text = |value: &str| json!({"type": "text", "value": value});
        for (alias, status, expected) in [
            (
                "missing",
                404,
                json!({"code": "repository-unknown", "args": {"alias": text("missing")}}),
            ),
            (
                "busy",
                409,
                json!({"code": "maintenance-busy", "args": {"operation": text(BUSY_OPERATION)}}),
            ),
        ] {
            let request = json!({"maintain-here": false}).to_string();
            let (response, _) = send_counted(Method::PUT, &route(alias), request).await;
            let (actual, answer) = json_of(response).await;
            assert_eq!(actual.as_u16(), status, "{alias}");
            assert_eq!(answer, expected, "{alias}");
        }
        for request in ["not json", "{}", r#"{"maintain-here": true, "more": 1}"#] {
            let (response, calls) = send_counted(Method::PUT, &route("demo"), request.into()).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{request}");
            assert_eq!(
                calls, 0,
                "a body that is not the request reached the daemon"
            );
        }
    }

    async fn json_of(response: Response) -> (StatusCode, serde_json::Value) {
        let status = response.status();
        (status, serde_json::from_str(&body(response).await).unwrap())
    }

    #[tokio::test]
    async fn a_test_answers_with_the_repositorys_id() {
        let request = serde_json::json!({"settings": settings_json("/ok")});
        let (response, calls) = post_counted(TEST, request.to_string()).await;
        assert_eq!(calls, 1);
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(answer, serde_json::json!({"id": "test:/ok"}));
    }

    #[tokio::test]
    async fn a_connection_is_created_and_answers_with_the_repositorys_id() {
        let mut settings = settings_json("/ok");
        settings["repo-hot"] = serde_json::json!("/hot");
        let request = serde_json::json!({"alias": "demo", "settings": settings});
        let (response, _) = post_counted(CONNECT, request.to_string()).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(answer, serde_json::json!({"id": "demo:/ok:/hot"}));
    }

    #[tokio::test]
    async fn an_initialisation_says_whether_it_was_saved() {
        for (alias, saved) in [(None, false), (Some("fresh"), true)] {
            let mut request = serde_json::json!({"settings": settings_json("/new")});
            if let Some(alias) = alias {
                request["alias"] = serde_json::json!(alias);
            }
            let (response, _) = post_counted(INIT, request.to_string()).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::CREATED, "{alias:?}");
            assert_eq!(
                answer,
                serde_json::json!({"id": "init:/new", "saved": saved}),
                "{alias:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_saved_repository_is_created_and_answers_with_its_id() {
        let (response, calls) = post_counted("/api/v1/repositories/demo/init", "{}".into()).await;
        assert_eq!(calls, 1);
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(answer, serde_json::json!({"id": "created:demo"}));
    }

    #[tokio::test]
    async fn a_test_of_a_saved_repository_answers_each_location_or_a_typed_refusal() {
        let (response, calls) = post_counted("/api/v1/repositories/nas/test", "{}".into()).await;
        assert_eq!(calls, 1);
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            answer,
            serde_json::json!({"locations": [
                {"position": 1, "address": "/srv/nas", "outcome": "used"}
            ]})
        );
        let (response, _) = post_counted("/api/v1/repositories/missing/test", "{}".into()).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(answer["code"], "repository-unknown");
        let (response, calls) =
            post_counted("/api/v1/repositories/nas/test", "not json".into()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(calls, 0, "a body that is not JSON reached the daemon");
    }

    #[tokio::test]
    async fn creating_a_saved_repository_is_refused_by_the_kind_of_its_refusal() {
        use serde_json::json;
        let text = |value: &str| json!({"type": "text", "value": value});
        for (alias, status, expected) in [
            (
                "missing",
                404,
                json!({"code": "repository-unknown", "args": {"alias": text("missing")}}),
            ),
            (
                "exists",
                409,
                json!({"code": "repository-exists", "args": {"location": text("/srv/exists")}}),
            ),
            ("not-offered", 409, json!({"code": "create-not-offered"})),
        ] {
            let route = format!("/api/v1/repositories/{alias}/init");
            let (response, _) = post_counted(&route, "{}".into()).await;
            let (actual, answer) = json_of(response).await;
            assert_eq!(actual.as_u16(), status, "{alias}");
            assert_eq!(answer, expected, "{alias}");
        }
        let (response, calls) =
            post_counted("/api/v1/repositories/demo/init", "not json".into()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(calls, 0, "a body that is not JSON reached the daemon");
    }

    #[tokio::test]
    async fn every_refusal_is_typed_with_the_status_of_its_kind() {
        use serde_json::json;
        let text = |value: &str| json!({"type": "text", "value": value});
        for (location, status, expected) in [
            ("/invalid", 400, json!({"code": "invalid-body"})),
            (
                "/alias-invalid",
                422,
                json!({"code": "repository-alias-invalid"}),
            ),
            (
                "/alias-taken",
                409,
                json!({"code": "repository-alias-taken", "args": {"alias": text("demo")}}),
            ),
            (
                "/already-configured",
                409,
                json!({"code": "repository-already-configured", "args": {"alias": text("demo")}}),
            ),
            (
                "/exists",
                409,
                json!({"code": "repository-exists", "args": {"location": text("/exists")}}),
            ),
            ("/not-found", 404, json!({"code": "repository-not-found"})),
            (
                "/config-not-found",
                404,
                json!({
                    "code": "repository-config-not-found",
                    "detail": "No repository config file found for `/media/usb/restic`.",
                }),
            ),
            (
                "/relative",
                422,
                json!({"code": "repository-path-relative", "args": {"field": text("repo-hot")}}),
            ),
            (
                "/secret",
                422,
                json!({"code": "secret-missing", "args": {"field": text("password"), "source": text("PW")}}),
            ),
            (
                "/failed",
                422,
                json!({
                    "code": "operation-failed",
                    "detail": "wrong password or no repository at the location",
                }),
            ),
            (
                "/unavailable",
                503,
                json!({"code": "engine-unavailable", "detail": "no worker program"}),
            ),
            (
                "/config",
                503,
                json!({
                    "code": "config-unavailable",
                    "args": {"file": text("/etc/config.toml")},
                    "detail": "permission denied",
                }),
            ),
        ] {
            for (route, body) in [
                (TEST, json!({"settings": settings_json(location)})),
                (
                    CONNECT,
                    json!({"alias": "demo", "settings": settings_json(location)}),
                ),
                (INIT, json!({"settings": settings_json(location)})),
            ] {
                let (response, _) = post_counted(route, body.to_string()).await;
                let (actual, answer) = json_of(response).await;
                assert_eq!(actual.as_u16(), status, "{location} on {route}");
                assert_eq!(answer, expected, "{location} on {route}");
                assert!(
                    !answer.to_string().contains("correct-horse"),
                    "an answer holds the password: {answer}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_body_that_is_not_the_json_the_route_takes_is_refused_before_the_daemon_sees_it() {
        let valid = settings_json("/ok");
        let hot_underscore = {
            let mut settings = valid.clone();
            settings["repo_hot"] = serde_json::json!("/y");
            settings
        };
        let command = serde_json::json!({
            "repository": "/ok",
            "password": {"source": "command", "command": "pass show x"},
        });
        for route in [TEST, CONNECT, INIT] {
            let wrap = |settings: &serde_json::Value| {
                serde_json::json!({"alias": "demo", "settings": settings}).to_string()
            };
            for (what, body) in [
                ("not JSON", "not json".to_owned()),
                ("an empty body", String::new()),
                ("an array", "[]".to_owned()),
                ("no settings", r#"{"alias":"demo"}"#.to_owned()),
                (
                    "null settings",
                    r#"{"alias":"demo","settings":null}"#.to_owned(),
                ),
                ("an unknown field", wrap(&hot_underscore)),
                ("a source the API refuses", wrap(&command)),
                (
                    "an unknown field at the top",
                    serde_json::json!({"alias": "demo", "settings": valid, "x": 1}).to_string(),
                ),
            ] {
                let (response, calls) = post_counted(route, body).await;
                let (status, answer) = json_of(response).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{what} to {route}");
                assert_eq!(
                    answer,
                    serde_json::json!({"code": "invalid-body"}),
                    "{what} to {route}"
                );
                assert_eq!(calls, 0, "{what} to {route} reached the daemon");
            }
        }
        // A connection needs the name to save it under.
        let (response, calls) =
            post_counted(CONNECT, serde_json::json!({"settings": valid}).to_string()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(calls, 0);
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_before_the_daemon_sees_it() {
        let oversize = "a".repeat(3 * 1024 * 1024);
        for route in [TEST, CONNECT, INIT] {
            let request =
                serde_json::json!({"alias": "demo", "settings": settings_json(&oversize)});
            let (response, calls) = post_counted(route, request.to_string()).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{route}");
            assert_eq!(answer["code"], "body-too-large", "{route}");
            assert_eq!(calls, 0, "{route} reached the daemon");
        }
    }

    #[tokio::test]
    async fn the_routes_want_a_json_content_type() {
        for route in [TEST, CONNECT, INIT] {
            let response = router(ui(), RequestChecks::default(), state())
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri(route)
                        .header(header::HOST, "localhost:9898")
                        .header(header::CONTENT_TYPE, "text/plain")
                        .body(Body::from(r#"{"settings":{}}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{route}");
            assert_eq!(answer["code"], "json-required", "{route}");
        }
    }

    #[tokio::test]
    async fn the_test_and_init_routes_take_nothing_but_a_post() {
        for route in [TEST, INIT] {
            let response = request(ui(), Method::GET, route, "").await;
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{route}");
        }
    }
}
