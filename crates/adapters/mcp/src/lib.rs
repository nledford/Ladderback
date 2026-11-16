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
    BuildDto, ErrorCode, ErrorDto, ExposureDto, LocationsTestDto, MaintenancePreviewDto,
    OperationDetailDto, OperationSummaryDto, OperationsPageDto, PlanSummaryDto, PlansDto,
    ProfileExportDto, ProfileImportDto, RepositoriesDto, SnapshotsDto,
};
use async_trait::async_trait;
use l10n::Localizer;
use rmcp::handler::server::tool::{schema_for_input, schema_for_output};
use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, ConstString,
    ContentBlock, CustomRequest, CustomResult, Implementation, JsonObject, ListToolsResult,
    PaginatedRequestParams, ProgressNotificationParam, ProtocolVersion, ServerCapabilities,
    ServerConfig, Tool, ToolAnnotations,
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
mod one_shot;
mod pages;

pub use actions::{CANCELLABLE_KIND, DEFAULT_WAIT, MAX_WAIT, WAIT_STEP, not_cancellable};
pub use connect::{
    ApiToken, ClientUnavailable, Connect, DEFAULT_DAEMON_URL, DaemonUrl, InvalidDaemonUrl,
    TokenOverPlainHttp,
};
pub use one_shot::{Answer, Call, CallRefused, Outcome};
pub use pages::{DEFAULT_PAGE, MAX_OPERATION_WARNINGS, MAX_PAGE, SnapshotFilter};

use actions::{
    CancelArguments, Cancelling, RunArguments, Started, WaitArguments, Waited, wait_length,
};
use arguments::{NoArguments, read};
use checks::{ExportArguments, ForgetPreview, ImportArguments, RepositoryArguments};
use pages::{
    ListArguments, Operation, OperationArguments, OperationsArguments, PlansPage, RepositoriesPage,
    SnapshotsArguments, SnapshotsPage, page, page_length,
};

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
const LIST_OPERATIONS: &str = "list_operations";
const GET_OPERATION: &str = "get_operation";
const RUN_PLAN: &str = "run_plan";
const WAIT_OPERATION: &str = "wait_operation";
const CANCEL_OPERATION: &str = "cancel_operation";
const TEST_REPOSITORY: &str = "test_repository";
const PREVIEW_FORGET: &str = "preview_forget";
const PREVIEW_IMPORT: &str = "preview_import";
const EXPORT_PROFILE: &str = "export_profile";

/// A tool, as a call names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolName {
    Status,
    ListPlans,
    ListRepositories,
    ListSnapshots,
    ListOperations,
    GetOperation,
    RunPlan,
    WaitOperation,
    CancelOperation,
    TestRepository,
    PreviewForget,
    PreviewImport,
    ExportProfile,
}

impl ToolName {
    /// Every tool.
    const ALL: [Self; 13] = [
        Self::Status,
        Self::ListPlans,
        Self::ListRepositories,
        Self::ListSnapshots,
        Self::ListOperations,
        Self::GetOperation,
        Self::RunPlan,
        Self::WaitOperation,
        Self::CancelOperation,
        Self::TestRepository,
        Self::PreviewForget,
        Self::PreviewImport,
        Self::ExportProfile,
    ];

    /// The tool named `name`, if there is one.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// The tool's name, as `tools/list` gives it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Status => STATUS,
            Self::ListPlans => LIST_PLANS,
            Self::ListRepositories => LIST_REPOSITORIES,
            Self::ListSnapshots => LIST_SNAPSHOTS,
            Self::ListOperations => LIST_OPERATIONS,
            Self::GetOperation => GET_OPERATION,
            Self::RunPlan => RUN_PLAN,
            Self::WaitOperation => WAIT_OPERATION,
            Self::CancelOperation => CANCEL_OPERATION,
            Self::TestRepository => TEST_REPOSITORY,
            Self::PreviewForget => PREVIEW_FORGET,
            Self::PreviewImport => PREVIEW_IMPORT,
            Self::ExportProfile => EXPORT_PROFILE,
        }
    }
}

/// How many of the newest operations `status` looks through for those
/// queued or running: the history's largest page.
pub const RECENT_OPERATIONS: u64 = 200;

/// Those of `operations` that are queued or running, in their order: what
/// `status` lists of the newest [`RECENT_OPERATIONS`], in either mode.
pub fn unfinished(operations: Vec<OperationSummaryDto>) -> Vec<OperationSummaryDto> {
    operations.into_iter().filter(is_unfinished).collect()
}

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
}

/// What a [`Backend`] tells `status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendStatus {
    /// How the tools reach Ladderback.
    pub access: Access,
    /// The daemon the tools drive, in connect mode.
    pub daemon: Option<DaemonStatus>,
    pub plans: Vec<PlanStatus>,
    /// The operations queued or running, newest first.
    pub running: Vec<OperationSummaryDto>,
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

/// One plan as `status` shows it: the plans list's entry, with its next run,
/// and its last operation, left out when it has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct PlanStatus {
    #[serde(flatten)]
    pub plan: PlanSummaryDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<OperationSummaryDto>,
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
    plans: Vec<PlanStatus>,
    /// The operations queued or running, newest first.
    running: Vec<OperationSummaryDto>,
}

/// The MCP server: the tools over a [`Backend`], for one session on one pair
/// of streams, saying each refusal in the catalog's words of its locale.
pub struct Server {
    backend: Arc<dyn Backend>,
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
            build,
            localizer,
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
            ToolName::ListOperations => answered(self.list_operations(given).await),
            ToolName::GetOperation => answered(self.get_operation(given).await),
            ToolName::RunPlan => answered(self.run_plan(given).await),
            ToolName::WaitOperation => answered(self.wait_operation(given, context).await),
            ToolName::CancelOperation => answered(self.cancel_operation(given).await),
            ToolName::TestRepository => answered(self.test_repository(given).await),
            ToolName::PreviewForget => answered(self.preview_forget(given).await),
            ToolName::PreviewImport => answered(self.preview_import(given).await),
            ToolName::ExportProfile => answered(self.export_profile(given).await),
        }
    }

    async fn status(&self) -> Answer {
        match self.backend.status().await {
            Ok(BackendStatus {
                access,
                daemon,
                plans,
                running,
            }) => structured(&Status {
                build: self.build.clone(),
                access,
                daemon,
                plans,
                running,
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
/// or stopping a backup.
fn acting() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(false)
        .idempotent(false)
        .open_world(false)
}

/// A tool that keeps nothing, yet opens a repository at each of its
/// locations as a backup would: testing a repository, which the daemon
/// grants only to the `run` scope, so it isn't offered as a read.
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
            "Ladderback's state: each plan with its next run and last operation, the \
             operations queued or running, this program's build and, in connect mode, \
             the daemon's build, sign-in state and exposure.",
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
             credentials masked, where its password comes from, whether it is maintained \
             here and its configuration warnings. Never a secret.",
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
             password stored in the configuration is refused, never written out.",
            input_schema::<ExportArguments>(),
        )
        .with_title("Export profile")
        .with_raw_output_schema(output_schema::<ProfileExportDto>())
        .with_annotations(read_only()),
    ]
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
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
        Ok(ListToolsResult::with_all_items(tools()))
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
        let Some(tool) = ToolName::named(&request.name) else {
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
