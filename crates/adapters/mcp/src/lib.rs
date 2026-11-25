//! The MCP server (technical design §4.6): `ladderback mcp` lets AI agents
//! and scripts drive Ladderback through the Model Context Protocol,
//! newline-delimited JSON-RPC 2.0 on standard input and output. An inbound
//! adapter beside `http`, with no rule of its own: each tool calls the one
//! [`Backend`] trait, which connect mode implements as a client of a running
//! daemon's API ([`Connect`]).
//!
//! The protocol is the `rmcp` SDK's (decision 95). It serves the revisions
//! `2025-06-18` and `2025-11-25`, which start with `initialize`, and
//! `2026-07-28`, whose requests each name their revision and whose clients
//! may ask `server/discover` first; anything older is refused, because
//! results rely on `structuredContent`. Standard output carries protocol
//! messages only.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use std::borrow::Cow;
use std::sync::Arc;

use api::{
    BuildDto, ErrorCode, ErrorDto, ExposureDto, FindDto, FolderDto, LocationsTestDto,
    MaintenancePreviewDto, OperationDetailDto, OperationSummaryDto, OperationsPageDto, PairingDto,
    PermissionLevelDto, PermissionsDto, PlanDryRunDto, PlansDto, ProfileExportDto,
    ProfileImportDto, ProposalDto, RepositoriesDto, SnapshotsDto, StatusDto,
};
use async_trait::async_trait;
use domain::access::permissions::Permission;
use l10n::Localizer;
use rmcp::handler::server::tool::{schema_for_input, schema_for_output};
use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, ConstString,
    ContentBlock, CustomRequest, CustomResult, Implementation, JsonObject, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ProgressNotificationParam, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::schemars::transform::transform_subschemas;
use rmcp::schemars::{JsonSchema, Schema};
use rmcp::service::{RequestContext, ServerInitializeError};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

mod actions;
mod arguments;
mod checks;
mod connect;
mod contents;
mod log;
mod one_shot;
mod pages;
mod pairing;
mod proposals;

pub use actions::{CANCELLABLE_KIND, DEFAULT_WAIT, MAX_WAIT, WAIT_STEP, not_cancellable};
pub use connect::{
    ApiToken, ClientUnavailable, Connect, DEFAULT_DAEMON_URL, DaemonUrl, InvalidDaemonUrl,
    TokenOverPlainHttp,
};
pub use one_shot::{Answer, Call, CallRefused, Outcome};
pub use pages::{DEFAULT_PAGE, MAX_OPERATION_WARNINGS, MAX_PAGE, SnapshotFilter};
pub use proposals::{PlanTables, Proposals};

use actions::{
    CancelArguments, Cancelling, RunArguments, Started, WaitArguments, Waited, wait_length,
};
use arguments::{NoArguments, read};
use checks::{ExportArguments, ForgetPreview, ImportArguments, RepositoryArguments};
use contents::{FindArguments, FolderArguments, Search, folder_of};
use pages::{
    ListArguments, Operation, OperationArguments, OperationsArguments, PlansPage, RepositoriesPage,
    SnapshotsArguments, SnapshotsPage, page, page_length,
};
use proposals::{DryRunArguments, ProposalArguments, ProposeArguments, Proposing};

/// The revisions served, oldest first: the first with `structuredContent`
/// and `outputSchema`, the one after it, and the current one, which has no
/// `initialize` (§4.6).
const REVISIONS: &[ProtocolVersion] = &[
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
    ProtocolVersion::V_2026_07_28,
];

/// The longest message read from the client, in bytes. A longer line is
/// dropped unread and the session goes on. Every request a tool takes is a
/// few hundred bytes but `preview_import`'s, which carries a profile's text:
/// one near the import's own 1 MiB cap doesn't fit (§4.6). The SDK's own
/// default is 16 MiB.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// The tools' names.
const STATUS: &str = "status";
const LIST_PLANS: &str = "list_plans";
const LIST_REPOSITORIES: &str = "list_repositories";
const LIST_SNAPSHOTS: &str = "list_snapshots";
const LIST_SNAPSHOT_FOLDER: &str = "list_snapshot_folder";
const FIND_IN_SNAPSHOTS: &str = "find_in_snapshots";
const LIST_OPERATIONS: &str = "list_operations";
const GET_OPERATION: &str = "get_operation";
const READ_LOG: &str = "read_log";
const PAIRING_STATE: &str = "pairing_state";
const RUN_PLAN: &str = "run_plan";
const WAIT_OPERATION: &str = "wait_operation";
const CANCEL_OPERATION: &str = "cancel_operation";
const TEST_REPOSITORY: &str = "test_repository";
const PREVIEW_FORGET: &str = "preview_forget";
const PREVIEW_IMPORT: &str = "preview_import";
const EXPORT_PROFILE: &str = "export_profile";
const PROPOSE_SETUP: &str = "propose_setup";
const GET_PROPOSAL: &str = "get_proposal";
const WITHDRAW_PROPOSAL: &str = "withdraw_proposal";
const DRY_RUN_PLAN: &str = "dry_run_plan";

/// A tool, as a call names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolName {
    Status,
    ListPlans,
    ListRepositories,
    ListSnapshots,
    ListSnapshotFolder,
    FindInSnapshots,
    ListOperations,
    GetOperation,
    ReadLog,
    PairingState,
    RunPlan,
    WaitOperation,
    CancelOperation,
    TestRepository,
    PreviewForget,
    PreviewImport,
    ExportProfile,
    ProposeSetup,
    GetProposal,
    WithdrawProposal,
    DryRunPlan,
}

impl ToolName {
    /// Every tool.
    const ALL: [Self; 21] = [
        Self::Status,
        Self::ListPlans,
        Self::ListRepositories,
        Self::ListSnapshots,
        Self::ListSnapshotFolder,
        Self::FindInSnapshots,
        Self::ListOperations,
        Self::GetOperation,
        Self::ReadLog,
        Self::PairingState,
        Self::RunPlan,
        Self::WaitOperation,
        Self::CancelOperation,
        Self::TestRepository,
        Self::PreviewForget,
        Self::PreviewImport,
        Self::ExportProfile,
        Self::ProposeSetup,
        Self::GetProposal,
        Self::WithdrawProposal,
        Self::DryRunPlan,
    ];

    /// The tool named `name`, if this build has one.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|tool| tool.built() && tool.name() == name)
    }

    /// Whether this build has the tool: `pairing_state` reads a pairing
    /// only a build with the hub can make (§16, decision 122).
    const fn built(self) -> bool {
        !matches!(self, Self::PairingState) || cfg!(feature = "hub")
    }

    /// The tool's name, as `tools/list` gives it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Status => STATUS,
            Self::ListPlans => LIST_PLANS,
            Self::ListRepositories => LIST_REPOSITORIES,
            Self::ListSnapshots => LIST_SNAPSHOTS,
            Self::ListSnapshotFolder => LIST_SNAPSHOT_FOLDER,
            Self::FindInSnapshots => FIND_IN_SNAPSHOTS,
            Self::ListOperations => LIST_OPERATIONS,
            Self::GetOperation => GET_OPERATION,
            Self::ReadLog => READ_LOG,
            Self::PairingState => PAIRING_STATE,
            Self::RunPlan => RUN_PLAN,
            Self::WaitOperation => WAIT_OPERATION,
            Self::CancelOperation => CANCEL_OPERATION,
            Self::TestRepository => TEST_REPOSITORY,
            Self::PreviewForget => PREVIEW_FORGET,
            Self::PreviewImport => PREVIEW_IMPORT,
            Self::ExportProfile => EXPORT_PROFILE,
            Self::ProposeSetup => PROPOSE_SETUP,
            Self::GetProposal => GET_PROPOSAL,
            Self::WithdrawProposal => WITHDRAW_PROPOSAL,
            Self::DryRunPlan => DRY_RUN_PLAN,
        }
    }

    /// Whether a server of `offer` offers the tool.
    pub(crate) fn offered_in(self, offer: Offer) -> bool {
        offer == Offer::Connect
            || !matches!(
                self,
                Self::ProposeSetup | Self::GetProposal | Self::WithdrawProposal | Self::DryRunPlan
            )
    }

    /// The permissions the tool needs (decision 127), as the daemon's routes
    /// it calls need them: cancelling reads the operation it cancels first,
    /// so it needs Backups beside Backup runs.
    pub(crate) const fn needs(self) -> &'static [Permission] {
        match self {
            Self::Status
            | Self::ListPlans
            | Self::ListRepositories
            | Self::ListSnapshots
            | Self::ListOperations
            | Self::GetOperation
            | Self::WaitOperation
            | Self::ExportProfile
            // The hub's address and key's fingerprint, both public (§4.6).
            | Self::PairingState => &[Permission::Backups],
            Self::RunPlan | Self::TestRepository => &[Permission::BackupRuns],
            Self::CancelOperation => &[Permission::Backups, Permission::BackupRuns],
            Self::PreviewForget | Self::PreviewImport => &[Permission::Previews],
            // A folder's page and a search name files (decision 132).
            Self::ListSnapshotFolder | Self::FindInSnapshots => &[Permission::SnapshotContents],
            // The recent log can name files and folders (decision 134).
            Self::ReadLog => &[Permission::DaemonLog],
            Self::ProposeSetup | Self::GetProposal | Self::WithdrawProposal | Self::DryRunPlan => {
                &[Permission::Configuration]
            }
        }
    }

    /// Whether `permissions` grant every permission the tool needs.
    pub(crate) fn opened_by(self, permissions: &PermissionsDto) -> bool {
        self.needs()
            .iter()
            .all(|&permission| grants(permissions, permission))
    }
}

/// Whether `permissions` hold `permission` at its level: a row on the wire
/// is its level or `none`.
fn grants(permissions: &PermissionsDto, permission: Permission) -> bool {
    let level = match permission {
        Permission::Backups => permissions.backups,
        Permission::BackupRuns => permissions.backup_runs,
        Permission::Previews => permissions.previews,
        Permission::Configuration => permissions.configuration,
        Permission::SnapshotContents => permissions.snapshot_contents,
        Permission::DaemonLog => permissions.daemon_log,
        Permission::RepositoryCheck => permissions.repository_check,
    };
    level != PermissionLevelDto::None
}

/// Which tools a server offers (§4.6). Connect mode offers every tool: the
/// daemon it drives serves the "Pending changes" page a person approves a
/// proposal on. A session of its own, headless or scratch, has no web UI to
/// approve in, so it offers no proposal tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    Connect,
    InProcess,
}

/// The URI of the status summary as an MCP resource (§4.6): what the
/// `status` tool answers, for an agent that reads context before it acts.
pub const STATUS_RESOURCE: &str = "ladderback://status";

/// What `resources/list` says the status summary is.
pub const STATUS_RESOURCE_DESCRIPTION: &str = "What the status tool answers: each plan's last \
    operation, last successful backup and how long ago, whether it is overdue and its next run; \
    the operations queued or running; the repositories that couldn't be read at their last \
    attempt; and the Full Disk Access state.";

/// Whether `operation` is queued or running: not finished yet.
fn is_unfinished(operation: &OperationSummaryDto) -> bool {
    matches!(operation.status.as_str(), "queued" | "running")
}

/// What the tools reach Ladderback through: a running daemon in connect
/// mode, or a session of this process's own in headless and scratch modes,
/// wired by the composition root. Each method is one tool's reading or
/// action; a failure is the `{code, args}` the API returns (§8.4), which
/// the tool hands on as a result with `isError`.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Ladderback's state as `status` shows it, without this program's build.
    async fn status(&self) -> Result<BackendStatus, ErrorDto>;

    /// The configured plans, in the order they were written, each with its
    /// configuration warnings.
    async fn plans(&self) -> Result<PlansDto, ErrorDto>;

    /// The configured repositories, in the order of their aliases, each
    /// with its configuration warnings and never a secret (I4).
    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto>;

    /// The snapshots of the repository whose alias is `repository`, newest
    /// first, read live.
    async fn snapshots(&self, repository: String) -> Result<SnapshotsDto, ErrorDto>;

    /// The page of the folder `folder` of the snapshot `snapshot`, in the
    /// repository whose alias is `repository`, that starts at position
    /// `from` and holds at most `most` entries, read live.
    async fn folder(
        &self,
        repository: String,
        snapshot: domain::SnapshotId,
        folder: domain::browse::FolderPath,
        from: u64,
        most: u64,
    ) -> Result<FolderDto, ErrorDto>;

    /// The entries whose names match `pattern` in the newest snapshots of
    /// the repository whose alias is `repository`, of `plan` alone when it
    /// names one, within `caps`, read live.
    async fn find(
        &self,
        repository: String,
        pattern: domain::find::NamePattern,
        plan: Option<domain::PlanId>,
        caps: domain::find::SearchCaps,
    ) -> Result<FindDto, ErrorDto>;

    /// One page of the history, newest first, as the API pages it: of
    /// `plan` alone when it names one, after the operation `after`, at most
    /// `limit` long.
    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: u64,
    ) -> Result<OperationsPageDto, ErrorDto>;

    /// The operation whose ID is `id`: its summary and every warning it
    /// recorded.
    async fn operation(&self, id: String) -> Result<OperationDetailDto, ErrorDto>;

    /// The summary alone of the operation whose ID is `id`: what a cancel
    /// and each step of a wait read, the same size however many warnings
    /// the operation recorded.
    async fn summary(&self, id: String) -> Result<OperationSummaryDto, ErrorDto>;

    /// The recent log (§3.1), newest first, every line of it, as
    /// `GET /api/v1/log` answers it.
    async fn recent_log(&self) -> Result<api::RecentLogDto, ErrorDto>;

    /// Queues a backup of the plan whose ID is `plan` now, under the plan's
    /// overlap policy, and returns its operation's ID. The trigger recorded
    /// is the backend's: `mcp` in this process, and through a daemon an API
    /// token's, or `manual` with sign-in off (decision 93).
    async fn run(&self, plan: String) -> Result<String, ErrorDto>;

    /// Asks the queued or running operation whose ID is `id` to stop, as an
    /// API token may: a backup only, whoever started it. Anything else is
    /// [`not_cancellable`], since maintenance must be able to finish
    /// (decision 93).
    async fn cancel(&self, id: String) -> Result<(), ErrorDto>;

    /// Tries every location of the repository whose alias is `repository`,
    /// as a job would choose among them, and tells each one's outcome.
    /// Records nothing.
    async fn test_repository(&self, repository: String) -> Result<LocationsTestDto, ErrorDto>;

    /// What a forget of the repository whose alias is `repository` would
    /// let go, read from the repository: refused as `maintenance-not-owner`
    /// unless this agent maintains it (I1). Takes no maintenance slot and
    /// writes nothing.
    async fn preview_forget(&self, repository: String) -> Result<MaintenancePreviewDto, ErrorDto>;

    /// What importing the rustic profile whose TOML is `content` under the
    /// alias `repository` would add, checked as an import checks it. Saves
    /// nothing and reads no file: the profiles it uses are a notice.
    async fn preview_import(
        &self,
        content: String,
        repository: String,
    ) -> Result<ProfileImportDto, ErrorDto>;

    /// The plan whose ID is `plan` as a profile the stock `rustic` reads,
    /// and what was left out of it. A password stored inline is refused as
    /// `inline-secret`, never written out (I4).
    async fn export_profile(&self, plan: String) -> Result<ProfileExportDto, ErrorDto>;

    /// What this backend's AI agent may do (decision 127): in connect mode
    /// what the daemon says this program's requests may do, its token's
    /// permissions or, with sign-in off, what AI agents may do without a
    /// token; in a session of its own the `[without-token]` setting. The
    /// tools listed are those these open.
    async fn permissions(&self) -> Result<PermissionsDto, ErrorDto>;

    /// Whether this machine is paired with a hub, and with which, as its
    /// pin says (`pairing_state`): the hub's address and the fingerprint of
    /// its key, never a token or a secret (I4). It never connects to the
    /// hub.
    async fn pairing(&self) -> Result<api::PairingDto, ErrorDto>;
}

/// What a [`Backend`] tells `status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendStatus {
    /// How the tools reach Ladderback.
    pub access: Access,
    /// The daemon the tools drive, in connect mode.
    pub daemon: Option<DaemonStatus>,
    /// The status summary, as `GET /api/v1/status` answers it (§4.6).
    pub summary: StatusDto,
}

/// How the tools reach Ladderback, as `status` tells it: decided by how
/// this program connected, never by the session route's answer to a token,
/// which is `signed-out` (decision 93).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    /// Connect mode, with an API token the daemon took.
    ApiToken,
    /// Connect mode, to a daemon with sign-in off, where every route is
    /// open and a token is ignored.
    SignInOff,
    /// Headless and scratch modes: a session of this process's own.
    InProcess,
}

/// The daemon connect mode drives: its build, when it tells it (with
/// sign-in off or to a `read` token, decisions 90 and 93), and who can
/// reach it (decision 87).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct DaemonStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildDto>,
    pub exposure: ExposureDto,
}

/// The `status` tool's result: this program's build (`version`, `commit`,
/// `dirty`, `release` as fields of their own, decision 90), then what the
/// backend told.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
struct Status {
    /// This program's build.
    build: BuildDto,
    /// How the tools reach Ladderback: `api-token`, `sign-in-off` (connect
    /// mode) or `in-process` (headless and scratch modes).
    access: Access,
    /// The daemon the tools drive, in connect mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    daemon: Option<DaemonStatus>,
    /// The status summary: each plan with its last operation, last
    /// successful backup and whether it is overdue; the operations queued
    /// or running, newest first; the repositories that couldn't be read at
    /// their last attempt; and the Full Disk Access state.
    #[serde(flatten)]
    summary: StatusDto,
}

/// The MCP server: the tools over a [`Backend`], for one session on one pair
/// of streams, saying each refusal in the catalog's words of its locale.
pub struct Server {
    backend: Arc<dyn Backend>,
    proposing: Option<Proposing>,
    build: BuildDto,
    localizer: Localizer,
}

/// Why a session ended other than by the client closing its input.
#[derive(Debug)]
pub struct SessionFailed(String);

impl std::fmt::Display for SessionFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SessionFailed {}

impl Server {
    /// A server whose tools reach Ladderback through `backend`, naming
    /// `build` as this program's, and saying refusals through `localizer`:
    /// in the locale of the environment it runs in, as the command line's
    /// own messages are (§4.6, §8.4).
    pub fn new(backend: Arc<dyn Backend>, build: BuildDto, localizer: Localizer) -> Self {
        Self {
            backend,
            proposing: None,
            build,
            localizer,
        }
    }

    /// This server, offering the proposal tools over `proposals`, reading
    /// a proposed plan's table with `tables`: connect mode's, driving a
    /// daemon a person approves proposals on (§4.6).
    #[must_use]
    pub fn proposing(self, proposals: Arc<dyn Proposals>, tables: Arc<dyn PlanTables>) -> Self {
        Self {
            proposing: Some(Proposing { proposals, tables }),
            ..self
        }
    }

    /// Which tools the server offers.
    pub fn offer(&self) -> Offer {
        if self.proposing.is_some() {
            Offer::Connect
        } else {
            Offer::InProcess
        }
    }

    /// The sentence the catalog says `error` in, in the server's locale,
    /// with the code's own arguments and detail: what an AI agent, or the
    /// person it relays the refusal to, reads to know what to do next.
    pub fn sentence(&self, error: &ErrorDto) -> String {
        let args: Vec<(&str, api::MsgArg)> = error
            .args
            .iter()
            .map(|(name, arg)| (name.as_str(), arg.clone()))
            .collect();
        self.localizer
            .error(error.code, &args, error.detail.as_deref())
            .into_text()
    }

    /// What `tools/call` answers: a result as `structuredContent`, mirrored
    /// as JSON text. A refusal is marked as an error and has two text
    /// contents: its `{code, args}` as JSON, for a client to branch on, then
    /// its [`Server::sentence`]. It has no `structuredContent`: clients check
    /// `structuredContent` against the tool's output schema, error or not,
    /// and a refusal isn't the shape it declares (§4.6).
    fn result(&self, answer: Answer) -> CallToolResult {
        let content = match answer {
            Answer::Done(value) => return CallToolResult::structured(value),
            Answer::Refused(error) => vec![
                ContentBlock::text(
                    serde_json::to_string(&error).unwrap_or_else(|error| error.to_string()),
                ),
                ContentBlock::text(self.sentence(&error)),
            ],
            Answer::Unwritten(why) => vec![ContentBlock::text(why)],
        };
        CallToolResult::error(content)
    }

    /// Serves one session: reads requests from `input` and writes answers to
    /// `output`, until the client closes `input`.
    ///
    /// # Errors
    ///
    /// The session ended for another reason: the first message wasn't a
    /// request, or the streams failed.
    pub async fn serve<R, W>(self, input: R, output: W) -> Result<(), SessionFailed>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let transport =
            AsyncRwTransport::new_server(input, output).with_max_line_length(MAX_MESSAGE_BYTES);
        match ServiceExt::serve(self, transport).await {
            Ok(running) => running
                .waiting()
                .await
                .map(drop)
                .map_err(|error| SessionFailed(error.to_string())),
            // Input closed before the first request: nothing to serve.
            Err(ServerInitializeError::ConnectionClosed(_)) => Ok(()),
            Err(error) => Err(SessionFailed(error.to_string())),
        }
    }

    /// Calls `call`'s tool as `tools/call` does, with no client to tell its
    /// progress or to cancel it: a one-shot call (§4.6).
    pub async fn call(&self, call: Call) -> Answer {
        self.dispatch(call.tool, Some(call.arguments), None).await
    }

    /// Runs `tool` with the arguments `given`: the one dispatch of
    /// `tools/call` and of a one-shot call. `context` is the request's, over
    /// MCP, which a wait watches for its cancellation and tells its
    /// progress to.
    async fn dispatch(
        &self,
        tool: ToolName,
        given: Option<JsonObject>,
        context: Option<&RequestContext<RoleServer>>,
    ) -> Answer {
        match tool {
            ToolName::Status => match read::<NoArguments>(given) {
                Ok(NoArguments {}) => self.status().await,
                Err(error) => Answer::Refused(error),
            },
            ToolName::ListPlans => answered(self.list_plans(given).await),
            ToolName::ListRepositories => answered(self.list_repositories(given).await),
            ToolName::ListSnapshots => answered(self.list_snapshots(given).await),
            tool @ (ToolName::ListSnapshotFolder | ToolName::FindInSnapshots) => {
                self.snapshot_contents(tool, given).await
            }
            ToolName::ListOperations => answered(self.list_operations(given).await),
            ToolName::GetOperation => answered(self.get_operation(given).await),
            ToolName::ReadLog => answered(self.read_log(given).await),
            ToolName::PairingState => match read::<NoArguments>(given) {
                Ok(NoArguments {}) => answered(self.backend.pairing().await),
                Err(error) => Answer::Refused(error),
            },
            ToolName::RunPlan => answered(self.run_plan(given).await),
            ToolName::WaitOperation => answered(self.wait_operation(given, context).await),
            ToolName::CancelOperation => answered(self.cancel_operation(given).await),
            ToolName::TestRepository => answered(self.test_repository(given).await),
            ToolName::PreviewForget => answered(self.preview_forget(given).await),
            ToolName::PreviewImport => answered(self.preview_import(given).await),
            ToolName::ExportProfile => answered(self.export_profile(given).await),
            ToolName::ProposeSetup | ToolName::GetProposal | ToolName::WithdrawProposal => {
                let Some(proposing) = &self.proposing else {
                    // A one-shot call of a tool this server doesn't offer:
                    // refused before it asks anything, as `tools/call` is.
                    return Answer::Unwritten(format!("no tool is named {:?}", tool.name()));
                };
                answered(propose(proposing, tool, given).await)
            }
            ToolName::DryRunPlan => {
                let Some(proposing) = &self.proposing else {
                    return Answer::Unwritten(format!("no tool is named {:?}", tool.name()));
                };
                answered(dry_run(proposing, given).await)
            }
        }
    }

    async fn status(&self) -> Answer {
        match self.backend.status().await {
            Ok(BackendStatus {
                access,
                daemon,
                summary,
            }) => structured(&Status {
                build: self.build.clone(),
                access,
                daemon,
                summary,
            }),
            Err(error) => Answer::Refused(error),
        }
    }

    async fn list_plans(&self, given: Option<JsonObject>) -> Result<PlansPage, ErrorDto> {
        let arguments: ListArguments = read(given)?;
        let listed = self.backend.plans().await?;
        let (plans, next) = page(
            listed.plans,
            |plan| &plan.id,
            arguments.cursor,
            arguments.limit,
        )?;
        Ok(PlansPage { plans, next })
    }

    async fn list_repositories(
        &self,
        given: Option<JsonObject>,
    ) -> Result<RepositoriesPage, ErrorDto> {
        let arguments: ListArguments = read(given)?;
        let listed = self.backend.repositories().await?;
        let (repositories, next) = page(
            listed.repositories,
            |repository| &repository.alias,
            arguments.cursor,
            arguments.limit,
        )?;
        Ok(RepositoriesPage { repositories, next })
    }

    async fn list_snapshots(&self, given: Option<JsonObject>) -> Result<SnapshotsPage, ErrorDto> {
        let arguments: SnapshotsArguments = read(given)?;
        let filter = SnapshotFilter {
            plan: arguments.plan,
            host: arguments.host,
        };
        let mut listed = self
            .backend
            .snapshots(arguments.repository)
            .await?
            .snapshots;
        listed.retain(|snapshot| filter.keeps(snapshot));
        let (snapshots, next) = page(
            listed,
            |snapshot| &snapshot.id,
            arguments.cursor,
            arguments.limit,
        )?;
        Ok(SnapshotsPage { snapshots, next })
    }

    /// Runs one of the tools that show the file and folder names inside
    /// snapshots: the one place such a call passes through, where a setting
    /// that lets a person withhold those names from agents would refuse it.
    async fn snapshot_contents(&self, tool: ToolName, given: Option<JsonObject>) -> Answer {
        match tool {
            ToolName::ListSnapshotFolder => answered(self.list_snapshot_folder(given).await),
            ToolName::FindInSnapshots => answered(self.find_in_snapshots(given).await),
            other => Answer::Unwritten(format!("no tool is named {:?}", other.name())),
        }
    }

    async fn list_snapshot_folder(&self, given: Option<JsonObject>) -> Result<FolderDto, ErrorDto> {
        let arguments: FolderArguments = read(given)?;
        let (snapshot, folder) = folder_of(&arguments)?;
        self.backend
            .folder(
                arguments.repository,
                snapshot,
                folder,
                arguments.from.unwrap_or(0),
                page_length(arguments.limit),
            )
            .await
    }

    async fn find_in_snapshots(&self, given: Option<JsonObject>) -> Result<FindDto, ErrorDto> {
        let arguments: FindArguments = read(given)?;
        let search = Search::try_from(arguments)?;
        self.backend
            .find(search.repository, search.pattern, search.plan, search.caps)
            .await
    }

    async fn list_operations(
        &self,
        given: Option<JsonObject>,
    ) -> Result<OperationsPageDto, ErrorDto> {
        let arguments: OperationsArguments = read(given)?;
        self.backend
            .history(
                arguments.plan,
                arguments.cursor,
                page_length(arguments.limit),
            )
            .await
    }

    async fn get_operation(&self, given: Option<JsonObject>) -> Result<Operation, ErrorDto> {
        let arguments: OperationArguments = read(given)?;
        let OperationDetailDto {
            operation,
            warnings,
        } = self.backend.operation(arguments.operation).await?;
        Ok(Operation::listing(operation, warnings))
    }

    async fn read_log(&self, given: Option<JsonObject>) -> Result<log::LogPage, ErrorDto> {
        let arguments: log::LogArguments = read(given)?;
        log::read_page(self.backend.recent_log().await?, arguments)
    }

    async fn run_plan(&self, given: Option<JsonObject>) -> Result<Started, ErrorDto> {
        let arguments: RunArguments = read(given)?;
        let operation = self.backend.run(arguments.plan).await?;
        Ok(Started { operation })
    }

    /// Reads the operation's summary every [`WAIT_STEP`] until it has
    /// finished or the wait's length has passed, then its detail once,
    /// sending a progress notification after each read that finds it
    /// unfinished when the call carried a progress token. A detail too large
    /// to read is the summary just read, with no warnings listed or counted. A
    /// cancelled call stops the wait, never the operation: the answer is
    /// dropped then, as the protocol asks, so the detail isn't read for it.
    async fn wait_operation(
        &self,
        given: Option<JsonObject>,
        context: Option<&RequestContext<RoleServer>>,
    ) -> Result<Waited, ErrorDto> {
        let arguments: WaitArguments = read(given)?;
        let length = wait_length(arguments.timeout);
        let began = tokio::time::Instant::now();
        let progress = context.and_then(|context| context.meta.get_progress_token());
        loop {
            let operation = self.backend.summary(arguments.operation.clone()).await?;
            if context.is_some_and(|context| context.ct.is_cancelled()) {
                return Ok(Waited {
                    finished: !is_unfinished(&operation),
                    operation: Operation::unread(operation),
                });
            }
            let left = length.saturating_sub(began.elapsed());
            if !is_unfinished(&operation) || left.is_zero() {
                let operation = match self.backend.operation(arguments.operation).await {
                    Ok(OperationDetailDto {
                        operation,
                        warnings,
                    }) => Operation::listing(operation, warnings),
                    // How it ended still reaches the agent, its warnings'
                    // codes in the summary.
                    Err(error) if error.code == ErrorCode::DaemonAnswerTooLarge => {
                        Operation::unread(operation)
                    }
                    Err(error) => return Err(error),
                };
                return Ok(Waited {
                    finished: !is_unfinished(&operation.operation),
                    operation,
                });
            }
            if let (Some(token), Some(context)) = (&progress, context) {
                // The seconds waited, of the wait's length: they grow with
                // each, as the protocol asks. A client gone is the
                // session's end, which the wait sees through its token.
                let notification =
                    ProgressNotificationParam::new(token.clone(), began.elapsed().as_secs_f64())
                        .with_total(length.as_secs_f64())
                        .with_message(operation.status);
                let _ = context.peer.notify_progress(notification).await;
            }
            // A one-shot call has no client to cancel it: its wait ends
            // only when the operation does or its length has passed.
            let cancelled = async {
                match context {
                    Some(context) => context.ct.cancelled().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                () = cancelled => {}
                () = tokio::time::sleep(WAIT_STEP.min(left)) => {}
            }
        }
    }

    async fn cancel_operation(&self, given: Option<JsonObject>) -> Result<Cancelling, ErrorDto> {
        let arguments: CancelArguments = read(given)?;
        self.backend.cancel(arguments.operation.clone()).await?;
        Ok(Cancelling {
            operation: arguments.operation,
        })
    }

    async fn test_repository(
        &self,
        given: Option<JsonObject>,
    ) -> Result<LocationsTestDto, ErrorDto> {
        let RepositoryArguments { repository } = read(given)?;
        self.backend.test_repository(repository).await
    }

    async fn preview_forget(&self, given: Option<JsonObject>) -> Result<ForgetPreview, ErrorDto> {
        let RepositoryArguments { repository } = read(given)?;
        ForgetPreview::of(self.backend.preview_forget(repository).await?)
    }

    async fn preview_import(
        &self,
        given: Option<JsonObject>,
    ) -> Result<ProfileImportDto, ErrorDto> {
        let ImportArguments {
            content,
            repository,
        } = read(given)?;
        self.backend.preview_import(content, repository).await
    }

    async fn export_profile(
        &self,
        given: Option<JsonObject>,
    ) -> Result<ProfileExportDto, ErrorDto> {
        let ExportArguments { plan } = read(given)?;
        self.backend.export_profile(plan).await
    }
}

/// Runs the proposal tool `tool` over the daemon's proposal routes.
async fn propose(
    proposing: &Proposing,
    tool: ToolName,
    given: Option<JsonObject>,
) -> Result<ProposalDto, ErrorDto> {
    let proposals = proposing.proposals.as_ref();
    match tool {
        ToolName::ProposeSetup => {
            let arguments: ProposeArguments = read(given)?;
            let proposal = arguments.proposal(proposing.tables.as_ref())?;
            proposals.propose(proposal).await
        }
        ToolName::WithdrawProposal => {
            let ProposalArguments { proposal } = read(given)?;
            proposals.withdraw(proposal).await
        }
        _ => {
            let ProposalArguments { proposal } = read(given)?;
            proposals.proposal(proposal).await
        }
    }
}

/// Runs `dry_run_plan` over the daemon's dry-run route: the plan's table
/// read from its text, as `propose_setup` reads one.
async fn dry_run(
    proposing: &Proposing,
    given: Option<JsonObject>,
) -> Result<PlanDryRunDto, ErrorDto> {
    let arguments: DryRunArguments = read(given)?;
    let plan = arguments.plan(proposing.tables.as_ref())?;
    proposing.proposals.dry_run(plan).await
}

/// A tool's result: what it read or did, or its refusal.
fn answered(result: Result<impl Serialize, ErrorDto>) -> Answer {
    match result {
        Ok(value) => structured(&value),
        Err(error) => Answer::Refused(error),
    }
}

/// What a tool read or did, as JSON.
fn structured(value: &impl Serialize) -> Answer {
    match serde_json::to_value(value) {
        Ok(value) => Answer::Done(value),
        // Only a map with keys that aren't text fails to serialize, and no
        // result holds one: said as a failure all the same, never a panic.
        Err(error) => Answer::Unwritten(error.to_string()),
    }
}

/// The input schema of a tool that takes no arguments.
fn no_arguments() -> Arc<JsonObject> {
    let mut schema = JsonObject::new();
    schema.insert("type".to_owned(), "object".into());
    schema.insert(
        "properties".to_owned(),
        serde_json::Value::Object(JsonObject::new()),
    );
    schema.insert("additionalProperties".to_owned(), false.into());
    Arc::new(schema)
}

/// A tool that reads and changes nothing.
fn read_only() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// A tool that changes what Ladderback does, never what it keeps: starting
/// or stopping a backup, or making or withdrawing a proposal, which changes
/// no configuration until a person approves it.
fn acting() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(false)
        .idempotent(false)
        .open_world(false)
}

/// A tool that keeps nothing, yet opens a repository at each of its
/// locations as a backup would: testing a repository, which the daemon
/// grants only to the permission Backup runs, so it isn't offered as a
/// read.
fn trying() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// A tool's output schema without its fields' descriptions: the field names
/// say enough, and `tools/list` stays small (§4.6). Input schemas keep theirs.
fn output_schema<T: JsonSchema + 'static>() -> Arc<JsonObject> {
    let mut schema = Schema::from(schema_for_output::<T>().as_ref().clone());
    drop_descriptions(&mut schema);
    // A schema made from a map stays one.
    Arc::new(schema.as_object().cloned().unwrap_or_default())
}

fn drop_descriptions(schema: &mut Schema) {
    schema.remove("description");
    transform_subschemas(&mut drop_descriptions, schema);
}

/// A tool's input schema, in a form clients that turn it into a function
/// declaration of a narrower schema language take (§4.6): an optional
/// argument has its one type, left out of `required`, never a type list
/// with null; and no argument has a `format` or a `minimum`, its
/// description saying its bounds. Each argument type is a struct, whose
/// schema is an object, the one input schema the SDK takes; were it not,
/// the tool would say it takes no argument rather than give a schema that
/// isn't an object.
fn input_schema<T: JsonSchema + 'static>() -> Arc<JsonObject> {
    let Ok(made) = schema_for_input::<T>() else {
        return no_arguments();
    };
    let mut schema = Schema::from(made.as_ref().clone());
    narrow(&mut schema);
    schema
        .as_object()
        .map_or_else(no_arguments, |object| Arc::new(object.clone()))
}

fn narrow(schema: &mut Schema) {
    let named: Vec<Value> = match schema.get("type") {
        Some(Value::Array(types)) => types
            .iter()
            .filter(|kind| kind.as_str() != Some("null"))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    if let [kind] = &named[..] {
        schema.insert("type".to_owned(), kind.clone());
    }
    schema.remove("format");
    schema.remove("minimum");
    transform_subschemas(&mut narrow, schema);
}

/// Every tool, in the order `tools/list` gives them.
fn tools() -> Vec<Tool> {
    vec![
        Tool::new(
            STATUS,
            "Ladderback's state: each plan with its last operation, last successful backup \
             and how long ago, whether it is overdue and its next run; the operations queued \
             or running; the repositories that couldn't be read at their last attempt; the \
             Full Disk Access state; this program's build and, in connect mode, the daemon's \
             build, sign-in state and exposure.",
            no_arguments(),
        )
        .with_title("Status")
        .with_raw_output_schema(output_schema::<Status>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_PLANS,
            "The configured plans, a page at a time: each with its repository, whether it \
             has a schedule, its next run and its configuration warnings.",
            input_schema::<ListArguments>(),
        )
        .with_title("List plans")
        .with_raw_output_schema(output_schema::<PlansPage>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_REPOSITORIES,
            "The configured repositories, a page at a time: each with its location, \
             credentials masked, where its password comes from, who its entry asks to \
             maintain it, whether it is maintained here and its configuration warnings. \
             Never a secret.",
            input_schema::<ListArguments>(),
        )
        .with_title("List repositories")
        .with_raw_output_schema(output_schema::<RepositoriesPage>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_SNAPSHOTS,
            "One repository's snapshots, newest first, a page at a time, read from the \
             repository; only those of a plan, or taken on a host, when either is named.",
            input_schema::<SnapshotsArguments>(),
        )
        .with_title("List snapshots")
        .with_raw_output_schema(output_schema::<SnapshotsPage>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_SNAPSHOT_FOLDER,
            "One folder of a snapshot, a page at a time, read from the repository: each \
             entry's name, whether it is a folder, a file, a link or something else, a file's \
             size and when it was last modified, with how many entries the folder holds. \
             Restoring and downloading are done by the person, in the web UI.",
            input_schema::<FolderArguments>(),
        )
        .with_title("List snapshot folder")
        .with_raw_output_schema(output_schema::<FolderDto>())
        .with_annotations(read_only()),
        Tool::new(
            FIND_IN_SNAPSHOTS,
            "Finds files and folders by name across a repository's newest snapshots, or one \
             plan's: each match's path, and the snapshots that hold it with its size and when \
             it was last modified there. Searches at most 100 snapshots for at most 15 \
             seconds, returns at most 500 matches, and says which cap cut it short. \
             Restoring and downloading are done by the person, in the web UI.",
            input_schema::<FindArguments>(),
        )
        .with_title("Find in snapshots")
        .with_raw_output_schema(output_schema::<FindDto>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_OPERATIONS,
            "The history of operations, newest first, a page at a time, running ones \
             included: each with its status, times, warning codes and, when it failed, why.",
            input_schema::<OperationsArguments>(),
        )
        .with_title("List operations")
        .with_raw_output_schema(output_schema::<OperationsPageDto>())
        .with_annotations(read_only()),
        Tool::new(
            GET_OPERATION,
            "One operation: its summary, why it failed when it did, and the first 100 \
             warnings it recorded.",
            input_schema::<OperationArguments>(),
        )
        .with_title("Get operation")
        .with_raw_output_schema(output_schema::<Operation>())
        .with_annotations(read_only()),
        Tool::new(
            READ_LOG,
            "The daemon's recent log, newest first, as the web UI's Log page shows it: of a level \
             or worse, and of an operation, a plan or a time range when asked. At most 200 lines \
             and 32 KiB of text a call; stopped says which cap cut the page short, and next \
             reads on from it.",
            input_schema::<log::LogArguments>(),
        )
        .with_title("Read log")
        .with_raw_output_schema(output_schema::<log::LogPage>())
        .with_annotations(read_only()),
        Tool::new(
            PAIRING_STATE,
            "Whether this machine is paired with a hub, as its pin says: the hub's address, the \
             fingerprint of the hub's key and how its certificate is checked. A plain http:// \
             address names no certificate: it isn't encrypted and shows none, and the hub key \
             still proves the hub. Never a token or a secret. It doesn't connect to the hub, so \
             it can't say whether the hub has approved \
             the machine: `ladderback hub-status` on the machine says that. The resource \
             ladderback://pairing holds the steps of pairing.",
            no_arguments(),
        )
        .with_title("Pairing state")
        .with_raw_output_schema(output_schema::<PairingDto>())
        .with_annotations(read_only()),
        Tool::new(
            RUN_PLAN,
            "Queues a backup of a plan now, under the plan's overlap policy, and returns its \
             operation's ID. Its hooks and secret sources run as for any other run.",
            input_schema::<RunArguments>(),
        )
        .with_title("Run plan")
        .with_raw_output_schema(output_schema::<Started>())
        .with_annotations(acting()),
        Tool::new(
            WAIT_OPERATION,
            "Waits until an operation finishes or the timeout passes, then returns whether it \
             finished and the operation as get_operation does; when its warnings are too many \
             to read, none are listed, unlisted-warnings is absent and its warning-codes still \
             name them. Sends progress \
             notifications when the call asks for them. Cancelling the call stops the wait, not \
             the operation.",
            input_schema::<WaitArguments>(),
        )
        .with_title("Wait for operation")
        .with_raw_output_schema(output_schema::<Waited>())
        .with_annotations(read_only()),
        Tool::new(
            CANCEL_OPERATION,
            "Asks a queued or running backup to stop, however it started; it then ends as \
             cancelled. Maintenance can't be cancelled from here.",
            input_schema::<CancelArguments>(),
        )
        .with_title("Cancel operation")
        .with_raw_output_schema(output_schema::<Cancelling>())
        .with_annotations(acting()),
        Tool::new(
            TEST_REPOSITORY,
            "Tries each location of a configured repository, as a backup would choose among \
             them, and tells each one's outcome. Records nothing.",
            input_schema::<RepositoryArguments>(),
        )
        .with_title("Test repository")
        .with_raw_output_schema(output_schema::<LocationsTestDto>())
        .with_annotations(trying()),
        Tool::new(
            PREVIEW_FORGET,
            "What a forget of a repository maintained here would let go, by each plan's \
             retention, and what it leaves alone. Reads the repository; deletes nothing.",
            input_schema::<RepositoryArguments>(),
        )
        .with_title("Preview forget")
        .with_raw_output_schema(output_schema::<ForgetPreview>())
        .with_annotations(read_only()),
        Tool::new(
            PREVIEW_IMPORT,
            "What importing a rustic profile, given as its TOML text, would add: the \
             repository's name, the plans and what the import couldn't carry over. Saves \
             nothing and reads no file; profiles it uses are named in a notice.",
            input_schema::<ImportArguments>(),
        )
        .with_title("Preview import")
        .with_raw_output_schema(output_schema::<ProfileImportDto>())
        .with_annotations(read_only()),
        Tool::new(
            EXPORT_PROFILE,
            "A plan as a profile the stock rustic reads, and what the export left out. A \
             password stored in the configuration is refused, never written out; the \
             password command, every option rustic runs as a command and every hook are \
             left out, each named in a notice.",
            input_schema::<ExportArguments>(),
        )
        .with_title("Export profile")
        .with_raw_output_schema(output_schema::<ProfileExportDto>())
        .with_annotations(read_only()),
        Tool::new(
            PROPOSE_SETUP,
            "Proposes configuration changes for a person to approve in Ladderback's web UI: \
             plans to add, change or remove, with the reason why. Changes nothing until a \
             person approves; no tool approves. Returns the proposal kept, its ID and what \
             the submission checks warn of. A plan names a repository by its alias, never a \
             secret.",
            input_schema::<ProposeArguments>(),
        )
        .with_title("Propose setup")
        .with_raw_output_schema(output_schema::<ProposalDto>())
        .with_annotations(acting()),
        Tool::new(
            GET_PROPOSAL,
            "One proposal, by its ID: its status, its changes, its rationale and the reasons a \
             person gave for rejecting any of them. With an API token, only a proposal that \
             token made.",
            input_schema::<ProposalArguments>(),
        )
        .with_title("Get proposal")
        .with_raw_output_schema(output_schema::<ProposalDto>())
        .with_annotations(read_only()),
        Tool::new(
            WITHDRAW_PROPOSAL,
            "Withdraws a pending proposal, so no person approves it, and returns it. With an \
             API token, only a proposal that token made.",
            input_schema::<ProposalArguments>(),
        )
        .with_title("Withdraw proposal")
        .with_raw_output_schema(output_schema::<ProposalDto>())
        .with_annotations(acting()),
        Tool::new(
            DRY_RUN_PLAN,
            "Counts what a draft plan would back up, without backing anything up: the files \
             and bytes its sources hold through its source filter, and what each exclude \
             removes on its own. Counts and sizes only, never a path. Reads only within the \
             folders a person allowed on Ladderback's Settings page; runs no hook and opens no \
             repository. One runs at a time, none while a backup runs or waits, each stops \
             after a short time or a number of entries and says so, and each client may start \
             a few an hour.",
            input_schema::<DryRunArguments>(),
        )
        .with_title("Dry-run plan")
        .with_raw_output_schema(output_schema::<PlanDryRunDto>())
        .with_annotations(read_only()),
    ]
}

impl Server {
    /// The tools `tools/list` gives: those this server's offer holds and its
    /// backend's permissions open (decision 127). Permissions that can't be
    /// read hide no tool: the daemon still refuses what isn't allowed.
    async fn offered(&self) -> Vec<Tool> {
        let offer = self.offer();
        let permissions = self.backend.permissions().await.ok();
        tools()
            .into_iter()
            .filter(|tool| {
                ToolName::named(&tool.name).is_some_and(|named| {
                    named.offered_in(offer)
                        && permissions
                            .as_ref()
                            .is_none_or(|permissions| named.opened_by(permissions))
                })
            })
            .collect()
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        );
        info.server_info = Implementation::new("ladderback", self.build.display());
        info
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(REVISIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.offered().await))
    }

    /// The resources: the status summary and, in a build with the hub, the
    /// pairing guide (§4.6; §16, decision 122).
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let mut resources = vec![
            Resource::new(STATUS_RESOURCE, "status")
                .with_title("Status summary")
                .with_description(STATUS_RESOURCE_DESCRIPTION)
                .with_mime_type("application/json"),
        ];
        if cfg!(feature = "hub") {
            resources.push(
                Resource::new(pairing::PAIRING_RESOURCE, "pairing")
                    .with_title("Pairing a machine with its hub")
                    .with_description(pairing::PAIRING_RESOURCE_DESCRIPTION)
                    .with_mime_type("application/json"),
            );
        }
        Ok(ListResourcesResult::with_all_items(resources))
    }

    /// Reads the status summary as the `status` tool answers it, as JSON
    /// text. A refusal is an internal error whose data is its `{code, args}`
    /// and whose message is its [`Server::sentence`]. The pairing guide is
    /// said in the server's locale and asks the backend nothing. Any other
    /// URI names no resource.
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if cfg!(feature = "hub") && request.uri == pairing::PAIRING_RESOURCE {
            let guide = pairing::guide(&self.localizer);
            return Ok(ReadResourceResult::new(vec![
                ResourceContents::text(guide.to_string(), request.uri)
                    .with_mime_type("application/json"),
            ])
            .into());
        }
        if request.uri != STATUS_RESOURCE {
            return Err(McpError::resource_not_found(
                format!("no resource is named {:?}", request.uri),
                None,
            ));
        }
        match self.status().await {
            Answer::Done(value) => Ok(ReadResourceResult::new(vec![
                ResourceContents::text(value.to_string(), request.uri)
                    .with_mime_type("application/json"),
            ])
            .into()),
            Answer::Refused(error) => Err(McpError::internal_error(
                self.sentence(&error),
                serde_json::to_value(&error).ok(),
            )),
            Answer::Unwritten(why) => Err(McpError::internal_error(why, None)),
        }
    }

    /// Calls the tool `request` names. An argument it can't take is its
    /// `invalid-arguments` refusal, which the agent reads (§4.6); only a tool
    /// that doesn't exist is a protocol error, as are arguments that aren't
    /// an object, which the SDK refuses before this reads them.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let offer = self.offer();
        let Some(tool) = ToolName::named(&request.name).filter(|tool| tool.offered_in(offer))
        else {
            return Err(McpError::invalid_params(
                format!("no tool is named {:?}", request.name.as_ref()),
                None,
            ));
        };
        let answer = self.dispatch(tool, request.arguments, Some(&context)).await;
        Ok(self.result(answer).into())
    }

    /// A request the SDK couldn't read as any it knows. A `tools/call` among
    /// them has params of another shape, such as arguments that aren't an
    /// object: invalid params, as JSON-RPC names them, not a method this
    /// server lacks. Any other is a method it lacks, as the SDK answers.
    async fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, McpError> {
        if request.method == CallToolRequestMethod::VALUE {
            return Err(McpError::invalid_params(
                "a tool call names its tool and gives its arguments as an object",
                None,
            ));
        }
        Err(McpError::new(
            rmcp::model::ErrorCode::METHOD_NOT_FOUND,
            request.method,
            None,
        ))
    }
}

#[cfg(test)]
mod tests;
