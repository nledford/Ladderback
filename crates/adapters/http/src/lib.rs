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
mod downloads;
mod dry_runs;
mod full_disk_access;
#[cfg(feature = "hub")]
mod hub_pages;
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
mod sync;

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

pub use api_tokens::{status_shown, summary_shown, warning_shown};
pub use downloads::{DownloadRefusal, DownloadStart, Downloads, HandOff};
pub use dry_runs::{DryRuns, DryRunsError};
pub use full_disk_access::{FullDiskAccessRoute, SettingsOpener, SettingsUnopened};
pub use listener::{GatedListener, PeerAddr};
pub use maintenance::{MaintenanceRefusal, MaintenanceRequest};
pub use plans::{Plans, PlansError};
pub use profiles::{Profiles, ProfilesError};
pub use proposals::{Approval, Proposals, ProposalsError};
pub use repositories::{Repositories, RepositoriesError, SecretRefusal};
pub use request_checks::{
    Exposure, InvalidHostName, RequestChecks, StartupRefusal, proxied_hosts, startup_rule,
    validate_host_name,
};
pub use restart::{Restart, RestartAsker, Work, asked_by};
pub use restores::{RestoreRefusal, RestoreStart, Restores};
pub use sign_in::{COOKIE_NAME, CSRF_HEADER, SignIn};
pub use sync::SyncHub;

use api::UiEvent;
use app::ports::EventBus;
use async_trait::async_trait;
use domain::access::permissions::{Permission, Permissions};
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
    /// How a request dry-runs a draft plan, and reads and sets the dry-run
    /// allowlist (decision 89, point 13).
    pub dry_runs: Arc<dyn DryRuns>,
    /// How a request confirms a download and fetches it by its ticket
    /// (§10.4).
    pub downloads: Arc<dyn Downloads>,
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
    /// What AI agents may do without a token, which holds a request
    /// `ladderback mcp` marks when sign-in is off (§10.5).
    pub without_token: Arc<app::without_token::WithoutToken>,
    /// What the Full Disk Access probe found, and what opens System
    /// Settings' pane for it: neither off macOS (§16, decision 115).
    pub full_disk_access: FullDiskAccessRoute,
    /// What `/sync/` serves: the hub's handshake under `serve --hub`, and
    /// nothing otherwise (§16, decision 122).
    pub sync: SyncHub,
}

impl axum::extract::FromRef<AppState> for SignIn {
    fn from_ref(state: &AppState) -> Self {
        state.sign_in.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<app::without_token::WithoutToken> {
    fn from_ref(state: &AppState) -> Self {
        Arc::clone(&state.without_token)
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
    /// The plan's sources hold the data folder while it holds a secret key
    /// file that no glob can leave out, its path not being UTF-8 (§16,
    /// decision 120, "A copied data folder"): refused rather than back the
    /// keys up.
    KeyFilesNotLeftOut,
    /// A run of the plan is waiting for its turn already, and backs up the
    /// sources as they are when it starts: a plan never has two runs
    /// waiting (§16, decision 75, point 4), so nothing was queued.
    RunWaiting,
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
    /// repository `alias`, that starts at position `from` and holds at most
    /// `most` entries (§16, decision 102), read live.
    async fn folder(
        &self,
        alias: String,
        snapshot: domain::SnapshotId,
        folder: domain::browse::FolderPath,
        from: u64,
        most: u64,
    ) -> Result<api::FolderDto, FolderError>;

    /// The entries whose names match `pattern` in the newest snapshots of
    /// the repository `alias`, of `plan` alone when it names one, within
    /// `caps` (§16, decision 132), read live.
    async fn find(
        &self,
        alias: String,
        pattern: domain::find::NamePattern,
        plan: Option<domain::PlanId>,
        caps: domain::find::SearchCaps,
    ) -> Result<api::FindDto, SnapshotsError>;

    /// The status summary (§4.6), read now, with `full_disk_access`, what
    /// the Full Disk Access probe found, as its own.
    async fn status(
        &self,
        full_disk_access: api::FullDiskAccessStateDto,
    ) -> Result<api::StatusDto, CatalogError>;
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
    /// The engine ran the read and failed in its own words, with no more
    /// specific code (`operation-failed`), as the maintenance routes tell
    /// such a failure. `detail` is those words, which the engine masked of
    /// every secret of the repository (I4).
    OperationFailed { detail: String },
    /// The snapshots cannot be read just now: the configuration cannot be
    /// read or used (its `config-*` code, arguments and the store's words,
    /// as [`CatalogError::Unavailable`] carries them), or the engine cannot
    /// be started (`engine-unavailable`, its words only in the log).
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
    /// No turn came free, or the read didn't end, within the read timeout
    /// (`repository-read-timed-out`).
    TimedOut,
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
    let gate = sign_in::Gate {
        sign_in: state.sign_in.clone(),
        without_token: Arc::clone(&state.without_token),
    };
    #[cfg(feature = "hub")]
    let hub = state.sync.clone();
    let routes = Router::new()
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
            "/api/v1/tokens/{id}/permissions",
            axum::routing::put(api_tokens::change_permissions),
        )
        .route(
            api_tokens::PERMISSIONS_PATH,
            axum::routing::get(api_tokens::permissions),
        )
        // A session's only: the route table names no permission for it, so
        // neither a token nor an AI agent changes what AI agents may do
        // without a token (decision 127).
        .route(
            api_tokens::WITHOUT_TOKEN_PATH,
            axum::routing::get(api_tokens::without_token).put(api_tokens::change_without_token),
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
        .route("/api/v1/status", axum::routing::get(read_status))
        // Outside `/api/v1/plans/{id}`, so a plan whose ID is `dry-run`
        // stays reachable there.
        .route("/api/v1/dry-runs", axum::routing::post(dry_runs::dry_run))
        // A session's only: the scope table names no scope for it, so no
        // token, and so no proposal, sets the allowlist (decision 89).
        .route(
            "/api/v1/settings/dry-run-allowlist",
            axum::routing::get(dry_runs::allowlist).put(dry_runs::set_allowlist),
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
        // A session's only: the scope table names none of these, so a
        // token is refused before it reaches them (decision 89, point 7).
        .route(
            "/api/v1/proposals/{id}/review",
            axum::routing::get(proposals::review),
        )
        .route(
            "/api/v1/proposals/{id}/forget-preview",
            axum::routing::post(proposals::forget_preview),
        )
        .route(
            "/api/v1/proposals/{id}/approve",
            axum::routing::post(proposals::approve),
        )
        .route(
            "/api/v1/proposals/{id}/reject",
            axum::routing::post(proposals::reject),
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
            repositories::NOTES_PATH,
            axum::routing::get(repositories::notes),
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
        .route(
            "/api/v1/repositories/{alias}/find",
            axum::routing::get(find_in_snapshots),
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
            "/api/v1/repositories/{alias}/snapshots/{snapshot}/download",
            axum::routing::post(downloads::start),
        )
        .route(
            "/api/v1/downloads/{ticket}",
            axum::routing::get(downloads::fetch),
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
        );
    // Without the hub, the paths under `/sync/` answer 404 behind the
    // browser's checks, as every other path does (§16, decision 122).
    #[cfg(not(feature = "hub"))]
    let routes = routes.merge(sync::routes(&state.sync));
    // With it, the hub's own routes under `/api/v1/hub` are the page's,
    // behind every check the others are.
    #[cfg(feature = "hub")]
    let routes = hub_pages::routes(routes);
    let app = routes
        .with_state(state)
        .fallback(move |method: Method, uri: Uri| {
            let assets = Arc::clone(&assets);
            async move { serve(assets.as_ref(), &method, &uri) }
        })
        .layer(from_fn(body_limit::enforce))
        .layer(from_fn_with_state(gate, sign_in::enforce))
        .layer(from_fn_with_state(checks, request_checks::enforce));
    // With it, they are the agents', behind a check of their own.
    #[cfg(feature = "hub")]
    let app = sync::in_front_of(app, &hub);
    app.layer(from_fn(response_headers::add))
}

/// The most one message from an events client may hold. The UI sends pings,
/// which a control frame keeps to 125 bytes, and the library's default of
/// 64 MiB would let a client make the daemon buffer that much.
const EVENTS_CLIENT_MAX_MESSAGE: usize = 1024;

/// How often an events stream opened with a session, an API token or, with
/// sign-in off, as an AI agent's asks whether it may go on: a session that
/// has ended (signed out, unused too long, or the admin password set anew),
/// a token revoked or one that no longer has Backups, or AI agents without a
/// token no longer allowed Backups, stops its stream within this long.
const SESSION_RECHECK: std::time::Duration = std::time::Duration::from_secs(5);

/// The live-updates WebSocket (technical design §4.4): one per tab, carrying
/// [`UiEvent`]s. It sends nothing until an event arrives — the first state
/// comes over REST — and the client is dropped when a send fails. The
/// request checks have already allowed this handshake: a foreign `Origin`
/// never reaches here. The stream ends when the session that opened it
/// does, or when the program that opened it may no longer see backups, and
/// shows a program only what its permissions allow, read again as often.
async fn events(
    upgrade: WebSocketUpgrade,
    State(state): State<AppState>,
    session: Option<axum::Extension<sign_in::SessionOf>>,
    token: Option<axum::Extension<api_tokens::TokenOf>>,
    granted: Option<axum::Extension<api_tokens::Granted>>,
) -> Response {
    let stream = state.events.subscribe().await;
    let shown = granted
        .as_ref()
        .map_or_else(api_tokens::every, |axum::Extension(granted)| granted.0);
    let opener = match (session, token, granted) {
        (Some(axum::Extension(session)), _, _) => Some(Opener::Session(session)),
        (None, Some(axum::Extension(token)), _) => Some(Opener::Token(token)),
        (None, None, Some(_)) => Some(Opener::AiAgent(Arc::clone(&state.without_token))),
        (None, None, None) => None,
    };
    upgrade
        .max_message_size(EVENTS_CLIENT_MAX_MESSAGE)
        .max_frame_size(EVENTS_CLIENT_MAX_MESSAGE)
        .on_upgrade(move |socket| forward(socket, stream, opener, shown))
}

/// What opened an events stream: a session, an API token, or, with sign-in
/// off, a request marked as an AI agent's.
enum Opener {
    Session(sign_in::SessionOf),
    Token(api_tokens::TokenOf),
    AiAgent(Arc<app::without_token::WithoutToken>),
}

impl Opener {
    /// What the stream may show now: everything while the session is
    /// live; the permissions the token holds now, while it is kept (not
    /// revoked, and of the admin password kept now); what AI agents may do
    /// without a token now. `None` once there is none of these.
    async fn permissions(&self) -> Option<Permissions> {
        match self {
            Self::Session(session) => session
                .access
                .peek(&session.token)
                .map(|_| api_tokens::every()),
            Self::Token(token) => token.access.api_token_permissions(&token.grant.id).await,
            Self::AiAgent(without_token) => without_token.permissions().await.ok(),
        }
    }
}

/// Reads the bus and writes each event to the client, as `shown` lets it
/// see it, until the client goes, the daemon does, or what opened the
/// stream no longer has Backups, which the events route needs. A client's
/// `Ping` is answered with a `Pong`: the events stream never waits on the
/// client, so the reply races fairly with the next event.
async fn forward(
    mut socket: WebSocket,
    mut stream: BoxStream<'static, UiEvent>,
    opener: Option<Opener>,
    mut shown: Permissions,
) {
    let mut recheck = tokio::time::interval(SESSION_RECHECK);
    loop {
        tokio::select! {
            _ = recheck.tick(), if opener.is_some() => {
                if let Some(opener) = &opener {
                    match opener.permissions().await {
                        Some(now) if now.grants(Permission::Backups) => shown = now,
                        _ => break,
                    }
                }
            }
            event = stream.next() => {
                let Some(event) = event else { break };
                // An event that cannot be written is not retryable here: the
                // client is gone, so the connection ends rather than falling
                // behind.
                let Ok(text) = serde_json::to_string(&api_tokens::event_shown(shown, event)) else {
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
        Err(StartError::KeyFilesNotLeftOut) => refused_with(
            StatusCode::CONFLICT,
            api::ErrorCode::KeyFilesNotLeftOut,
            Vec::new(),
        ),
        Err(StartError::RunWaiting) => refused_with(
            StatusCode::CONFLICT,
            api::ErrorCode::PlanRunWaiting,
            vec![("plan", api::MsgArg::Text(plan))],
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
    granted: Option<axum::Extension<api_tokens::Granted>>,
    axum::extract::Query(params): axum::extract::Query<HistoryParams>,
) -> Response {
    match state
        .operations
        .history(params.plan, params.after.clone(), params.limit)
        .await
    {
        Ok(page) => {
            let page = match granted {
                Some(axum::Extension(api_tokens::Granted(held))) => api::OperationsPageDto {
                    operations: page
                        .operations
                        .into_iter()
                        .map(|summary| api_tokens::summary_shown(held, summary))
                        .collect(),
                    ..page
                },
                None => page,
            };
            (StatusCode::OK, axum::Json(page)).into_response()
        }
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

/// One operation's detail: what happened, with its warnings, each as the
/// program asking may see it: its path, and the engine's words of a failure,
/// only with Snapshot contents.
async fn get_operation(
    State(state): State<AppState>,
    granted: Option<axum::Extension<api_tokens::Granted>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match state.operations.operation(id.clone()).await {
        Ok(detail) => {
            let detail = match granted {
                Some(axum::Extension(api_tokens::Granted(held))) => api::OperationDetailDto {
                    operation: api_tokens::summary_shown(held, detail.operation),
                    warnings: detail
                        .warnings
                        .into_iter()
                        .map(|warning| api_tokens::warning_shown(held, warning))
                        .collect(),
                },
                None => detail,
            };
            (StatusCode::OK, axum::Json(detail)).into_response()
        }
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
        Err(CancelError::SessionOnly) => api_tokens::permission_refused(api_tokens::SESSION_ONLY),
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

/// `GET /api/v1/status`: the status summary (§4.6), with what the Full
/// Disk Access probe found last, each operation in it as the program asking
/// may see it.
async fn read_status(
    State(state): State<AppState>,
    granted: Option<axum::Extension<api_tokens::Granted>>,
) -> Response {
    match state.catalog.status(state.full_disk_access.state()).await {
        Ok(status) => {
            let status = match granted {
                Some(axum::Extension(api_tokens::Granted(held))) => {
                    api_tokens::status_shown(held, status)
                }
                None => status,
            };
            (StatusCode::OK, axum::Json(status)).into_response()
        }
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
        Err(error) => snapshots_refused(alias, error),
    }
}

/// The refusal of a read of the repository `alias` the listing, a folder's
/// page and a search all tell alike.
fn snapshots_refused(alias: String, error: SnapshotsError) -> Response {
    match error {
        SnapshotsError::Unknown => refused_with(
            StatusCode::NOT_FOUND,
            api::ErrorCode::RepositoryUnknown,
            vec![("alias", api::MsgArg::Text(alias))],
        ),
        SnapshotsError::SecretRefused(secret) => repositories::secret_refusal(&secret),
        SnapshotsError::RepositoryNotFound => no_repository(None),
        SnapshotsError::RepositoryConfigNotFound { detail } => no_repository(Some(detail)),
        SnapshotsError::OperationFailed { detail } => refused_detailed(
            StatusCode::UNPROCESSABLE_ENTITY,
            api::ErrorCode::OperationFailed,
            Vec::new(),
            Some(detail),
        ),
        SnapshotsError::Unavailable { code, args, detail } => unavailable(code, args, detail),
        SnapshotsError::TimedOut => read_timed_out(),
    }
}

/// The query the folder route takes: the folder's path in the snapshot, its
/// names joined by `/` (none for the top), the position the page starts at
/// (none for the first) and the most entries the page holds (none for a
/// full page). All optional.
#[derive(Deserialize, Default)]
struct FolderParams {
    path: Option<String>,
    from: Option<String>,
    limit: Option<String>,
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
    let Ok(most) = number(params.limit) else {
        return invalid_argument("limit", api::ArgumentProblem::Invalid);
    };
    let most = most.unwrap_or(u64::MAX);
    let Ok(id) = domain::SnapshotId::parse(&snapshot) else {
        return snapshot_unknown(snapshot);
    };
    let path = params.path.unwrap_or_default();
    let Ok(folder) = domain::browse::FolderPath::parse(&path) else {
        return folder_unknown(path);
    };
    match state
        .catalog
        .folder(alias.clone(), id, folder, from, most)
        .await
    {
        Ok(page) => (StatusCode::OK, axum::Json(page)).into_response(),
        Err(FolderError::SnapshotUnknown) => snapshot_unknown(snapshot),
        Err(FolderError::FolderUnknown) => folder_unknown(path),
        Err(FolderError::Repository(error)) => snapshots_refused(alias, error),
    }
}

/// The query the search route takes: the pattern a name must match, and
/// optionally the plan whose snapshots alone are searched and the caps on
/// the snapshots read and the matches returned.
#[derive(Deserialize, Default)]
struct FindParams {
    pattern: Option<String>,
    plan: Option<String>,
    snapshots: Option<String>,
    matches: Option<String>,
}

/// A search of a repository's snapshots for entries by name (§16, decision
/// 132), read live like the listing (§9). The pattern, the plan and the caps
/// are parsed here, where they enter; a cap past its most is lowered to it.
async fn find_in_snapshots(
    State(state): State<AppState>,
    axum::extract::Path(alias): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<FindParams>,
) -> Response {
    let Some(pattern) = params.pattern else {
        return invalid_argument("pattern", api::ArgumentProblem::Missing);
    };
    let Ok(pattern) = domain::find::NamePattern::parse(&pattern) else {
        return invalid_argument("pattern", api::ArgumentProblem::Invalid);
    };
    let plan = match params.plan.as_deref().map(domain::PlanId::parse) {
        None => None,
        Some(Ok(plan)) => Some(plan),
        Some(Err(_)) => return invalid_argument("plan", api::ArgumentProblem::Invalid),
    };
    let Ok(snapshots) = number(params.snapshots) else {
        return invalid_argument("snapshots", api::ArgumentProblem::Invalid);
    };
    let Ok(matches) = number(params.matches) else {
        return invalid_argument("matches", api::ArgumentProblem::Invalid);
    };
    let caps = domain::find::SearchCaps::clamped(snapshots, matches);
    match state.catalog.find(alias.clone(), pattern, plan, caps).await {
        Ok(found) => (StatusCode::OK, axum::Json(found)).into_response(),
        Err(error) => snapshots_refused(alias, error),
    }
}

/// A query parameter's value, a whole number when it is given; anything
/// else is an error, which the caller refuses as `invalid-arguments`.
fn number(given: Option<String>) -> Result<Option<u64>, std::num::ParseIntError> {
    given.map(|text| text.parse::<u64>()).transpose()
}

/// The refusal of a query parameter: 400 `invalid-arguments`, naming the
/// parameter and what is wrong with it, never the value given.
fn invalid_argument(field: &'static str, problem: api::ArgumentProblem) -> Response {
    refused_with(
        StatusCode::BAD_REQUEST,
        api::ErrorCode::InvalidArguments,
        vec![
            ("field", api::MsgArg::Text(field.to_owned())),
            ("problem", api::MsgArg::Text(problem.as_str().to_owned())),
        ],
    )
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

/// The refusal of a read of a repository that ran out of time: no turn came
/// free, or the worker didn't answer, within the read timeout.
fn read_timed_out() -> Response {
    refused_with(
        StatusCode::GATEWAY_TIMEOUT,
        api::ErrorCode::RepositoryReadTimedOut,
        Vec::new(),
    )
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

/// The router's fallback: the UI's files, and its page for every client-side
/// route. A path whose first segment is the API's is never the UI's, so one no
/// route serves is `route-unknown` (404) whatever its method: a program that
/// asks for a route this daemon lacks gets an error it can read, never the
/// page as a success (#1063).
fn serve(assets: &dyn AssetSource, method: &Method, uri: &Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let first = path.split_once('/').map_or(path, |(first, _)| first);
    if first == API_SEGMENT {
        // An answer of the API is never stored, as the response headers make
        // every one under `/api/`; this one also answers `/api` and `//api/…`.
        let mut response = refused(StatusCode::NOT_FOUND, api::ErrorCode::RouteUnknown);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    if method != Method::GET && method != Method::HEAD {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, HeaderValue::from_static("GET, HEAD"))],
        )
            .into_response();
    }
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

/// The first path segment the API keeps for itself.
const API_SEGMENT: &str = "api";

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
mod tests;
