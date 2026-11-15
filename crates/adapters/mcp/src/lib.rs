//! The MCP server (technical design §4.6): `ladderback mcp` lets AI agents
//! and scripts drive Ladderback through the Model Context Protocol,
//! newline-delimited JSON-RPC 2.0 on standard input and output. An inbound
//! adapter beside `http`, with no rule of its own: each tool calls the one
//! [`Backend`] trait, which connect mode implements as a client of a running
//! daemon's API ([`Connect`]).
//!
//! The protocol is the `rmcp` SDK's (decision NN). It serves the revisions
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
    BuildDto, ErrorDto, ExposureDto, LocationsTestDto, MaintenancePreviewDto, OperationDetailDto,
    OperationSummaryDto, OperationsPageDto, PlanSummaryDto, PlansDto, ProfileExportDto,
    ProfileImportDto, RepositoriesDto, SnapshotDto, SnapshotsDto,
};
use async_trait::async_trait;
use rmcp::handler::server::tool::schema_for_output;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
    Tool, ToolAnnotations,
};
use rmcp::schemars::transform::transform_subschemas;
use rmcp::schemars::{JsonSchema, Schema};
use rmcp::service::{RequestContext, ServerInitializeError};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite};

mod actions;
mod connect;
mod pages;

pub use connect::{
    ApiToken, ClientUnavailable, Connect, DEFAULT_DAEMON_URL, DaemonUrl, InvalidDaemonUrl,
    TokenOverPlainHttp,
};
pub use actions::not_cancellable;
pub use pages::{DEFAULT_PAGE, MAX_OPERATION_WARNINGS, MAX_PAGE};

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
/// few hundred bytes; the SDK's own default is 16 MiB.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// The tools' names.
const STATUS: &str = "status";
const LIST_PLANS: &str = "list_plans";
const LIST_REPOSITORIES: &str = "list_repositories";
const LIST_SNAPSHOTS: &str = "list_snapshots";
const LIST_OPERATIONS: &str = "list_operations";
const GET_OPERATION: &str = "get_operation";

/// How many of the newest operations `status` looks through for those
/// queued or running: the history's largest page.
pub const RECENT_OPERATIONS: u64 = 200;

/// Those of `operations` that are queued or running, in their order: what
/// `status` lists of the newest [`RECENT_OPERATIONS`], in either mode.
pub fn unfinished(operations: Vec<OperationSummaryDto>) -> Vec<OperationSummaryDto> {
    operations
        .into_iter()
        .filter(|operation| matches!(operation.status.as_str(), "queued" | "running"))
        .collect()
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

    /// Queues a backup of a plan now and returns its operation's ID.
    async fn run(&self, _plan: String) -> Result<String, ErrorDto> {
        todo!()
    }

    /// Asks a queued or running backup to stop.
    async fn cancel(&self, _id: String) -> Result<(), ErrorDto> {
        todo!()
    }

    /// Tries every location of a repository and tells each one's outcome.
    async fn test_repository(&self, _repository: String) -> Result<LocationsTestDto, ErrorDto> {
        todo!()
    }

    /// What a forget of a repository maintained here would let go.
    async fn preview_forget(
        &self,
        _repository: String,
    ) -> Result<MaintenancePreviewDto, ErrorDto> {
        todo!()
    }

    /// What importing a profile given as its TOML would add.
    async fn preview_import(
        &self,
        _content: String,
        _repository: String,
    ) -> Result<ProfileImportDto, ErrorDto> {
        todo!()
    }

    /// A plan as a profile the stock `rustic` reads.
    async fn export_profile(&self, _plan: String) -> Result<ProfileExportDto, ErrorDto> {
        todo!()
    }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

/// Which of a repository's snapshots `list_snapshots` lists: those of a
/// plan, those taken on a host, or both; every one when it names neither.
/// The server filters what either backend reads, so both modes keep the
/// same snapshots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotFilter {
    pub plan: Option<String>,
    pub host: Option<String>,
}

impl SnapshotFilter {
    /// Whether `snapshot` is one this filter lists: the plan its
    /// `ladderback-plan:` tag names, as maintenance reads it (§7.3), is the
    /// plan, and its host is the host.
    pub fn keeps(&self, snapshot: &SnapshotDto) -> bool {
        let _ = snapshot;
        todo!()
    }
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
    /// The daemon the tools drive, in connect mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    daemon: Option<DaemonStatus>,
    plans: Vec<PlanStatus>,
    /// The operations queued or running, newest first.
    running: Vec<OperationSummaryDto>,
}

/// The MCP server: the tools over a [`Backend`], for one session on one pair
/// of streams.
pub struct Server {
    backend: Arc<dyn Backend>,
    build: BuildDto,
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
    /// `build` as this program's.
    pub fn new(backend: Arc<dyn Backend>, build: BuildDto) -> Self {
        Self { backend, build }
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

    async fn status(&self) -> CallToolResult {
        match self.backend.status().await {
            Ok(BackendStatus {
                access: _,
                daemon,
                plans,
                running,
            }) => structured(
                &Status {
                    build: self.build.clone(),
                    daemon,
                    plans,
                    running,
                },
                false,
            ),
            Err(error) => structured(&error, true),
        }
    }

    async fn list_plans(&self, arguments: ListArguments) -> Result<PlansPage, ErrorDto> {
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
        arguments: ListArguments,
    ) -> Result<RepositoriesPage, ErrorDto> {
        let listed = self.backend.repositories().await?;
        let (repositories, next) = page(
            listed.repositories,
            |repository| &repository.alias,
            arguments.cursor,
            arguments.limit,
        )?;
        Ok(RepositoriesPage { repositories, next })
    }

    async fn list_snapshots(
        &self,
        arguments: SnapshotsArguments,
    ) -> Result<SnapshotsPage, ErrorDto> {
        let listed = self.backend.snapshots(arguments.repository).await?;
        let (snapshots, next) = page(
            listed.snapshots,
            |snapshot| &snapshot.id,
            arguments.cursor,
            arguments.limit,
        )?;
        Ok(SnapshotsPage { snapshots, next })
    }

    async fn list_operations(
        &self,
        arguments: OperationsArguments,
    ) -> Result<OperationsPageDto, ErrorDto> {
        self.backend
            .history(
                arguments.plan,
                arguments.cursor,
                page_length(arguments.limit),
            )
            .await
    }

    async fn get_operation(&self, arguments: OperationArguments) -> Result<Operation, ErrorDto> {
        let OperationDetailDto {
            operation,
            warnings,
        } = self.backend.operation(arguments.id).await?;
        Ok(Operation::listing(operation, warnings))
    }
}

/// A read tool's result: what it read, or the refusal, marked as an error.
fn answered(result: Result<impl Serialize, ErrorDto>) -> CallToolResponse {
    match result {
        Ok(value) => structured(&value, false),
        Err(error) => structured(&error, true),
    }
    .into()
}

/// The arguments `tool` was given, read as `T`.
///
/// # Errors
///
/// Invalid params: one is missing, unknown or of the wrong type.
fn arguments<T: DeserializeOwned>(tool: &str, given: Option<JsonObject>) -> Result<T, McpError> {
    serde_json::from_value(serde_json::Value::Object(given.unwrap_or_default())).map_err(|error| {
        McpError::invalid_params(
            format!("the tool {tool} can't take these arguments: {error}"),
            None,
        )
    })
}

/// A tool's result: `value` as `structuredContent`, mirrored as JSON text.
fn structured(value: &impl Serialize, is_error: bool) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) if is_error => CallToolResult::structured_error(value),
        Ok(value) => CallToolResult::structured(value),
        // Only a map with keys that aren't text fails to serialize, and no
        // result holds one: said as a failure all the same, never a panic.
        Err(error) => {
            CallToolResult::error(vec![rmcp::model::ContentBlock::text(error.to_string())])
        }
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

/// What every tool so far is: a reading that changes nothing.
fn read_only() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
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

/// Every tool, in the order `tools/list` gives them. Each argument type is
/// a struct, whose schema is an object, the one input schema the SDK takes.
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
            no_arguments(),
        )
        .with_title("List plans")
        .with_input_schema::<ListArguments>()
        .with_raw_output_schema(output_schema::<PlansPage>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_REPOSITORIES,
            "The configured repositories, a page at a time: each with its location, \
             credentials masked, where its password comes from, whether it is maintained \
             here and its configuration warnings. Never a secret.",
            no_arguments(),
        )
        .with_title("List repositories")
        .with_input_schema::<ListArguments>()
        .with_raw_output_schema(output_schema::<RepositoriesPage>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_SNAPSHOTS,
            "One repository's snapshots, newest first, a page at a time, read from the \
             repository.",
            no_arguments(),
        )
        .with_title("List snapshots")
        .with_input_schema::<SnapshotsArguments>()
        .with_raw_output_schema(output_schema::<SnapshotsPage>())
        .with_annotations(read_only()),
        Tool::new(
            LIST_OPERATIONS,
            "The history of operations, newest first, a page at a time, running ones \
             included: each with its status, times, warning codes and, when it failed, why.",
            no_arguments(),
        )
        .with_title("List operations")
        .with_input_schema::<OperationsArguments>()
        .with_raw_output_schema(output_schema::<OperationsPageDto>())
        .with_annotations(read_only()),
        Tool::new(
            GET_OPERATION,
            "One operation: its summary, why it failed when it did, and the first 100 \
             warnings it recorded.",
            no_arguments(),
        )
        .with_title("Get operation")
        .with_input_schema::<OperationArguments>()
        .with_raw_output_schema(output_schema::<Operation>())
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

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let given = request.arguments;
        match request.name.as_ref() {
            STATUS if given.as_ref().is_some_and(|args| !args.is_empty()) => Err(
                McpError::invalid_params("the tool status takes no arguments", None),
            ),
            STATUS => Ok(self.status().await.into()),
            LIST_PLANS => Ok(answered(
                self.list_plans(arguments(LIST_PLANS, given)?).await,
            )),
            LIST_REPOSITORIES => Ok(answered(
                self.list_repositories(arguments(LIST_REPOSITORIES, given)?)
                    .await,
            )),
            LIST_SNAPSHOTS => Ok(answered(
                self.list_snapshots(arguments(LIST_SNAPSHOTS, given)?).await,
            )),
            LIST_OPERATIONS => Ok(answered(
                self.list_operations(arguments(LIST_OPERATIONS, given)?)
                    .await,
            )),
            GET_OPERATION => Ok(answered(
                self.get_operation(arguments(GET_OPERATION, given)?).await,
            )),
            name => Err(McpError::invalid_params(
                format!("no tool is named {name:?}"),
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests;
