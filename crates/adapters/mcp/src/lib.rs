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
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
    Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, ServerInitializeError};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};

mod actions;
mod connect;

pub use connect::{
    ApiToken, ClientUnavailable, Connect, DEFAULT_DAEMON_URL, DaemonUrl, InvalidDaemonUrl,
    TokenOverPlainHttp,
};
pub use actions::not_cancellable;

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

/// The `status` tool's name.
const STATUS: &str = "status";

/// What the tools reach Ladderback through: a running daemon in connect
/// mode. Each method is one tool's reading or action; a failure is the
/// `{code, args}` the API returns (§8.4), which the tool hands on as a
/// result with `isError`.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Ladderback's state as `status` shows it, without this program's build.
    async fn status(&self) -> Result<BackendStatus, ErrorDto>;

    /// The configured plans.
    async fn plans(&self) -> Result<PlansDto, ErrorDto> {
        todo!()
    }

    /// The configured repositories.
    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto> {
        todo!()
    }

    /// One repository's snapshots.
    async fn snapshots(&self, _repository: String) -> Result<SnapshotsDto, ErrorDto> {
        todo!()
    }

    /// One page of the history.
    async fn history(
        &self,
        _plan: Option<String>,
        _after: Option<String>,
        _limit: u64,
    ) -> Result<OperationsPageDto, ErrorDto> {
        todo!()
    }

    /// One operation with its warnings.
    async fn operation(&self, _id: String) -> Result<OperationDetailDto, ErrorDto> {
        todo!()
    }

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Status {
    build: BuildDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    daemon: Option<DaemonStatus>,
    plans: Vec<PlanStatus>,
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
        todo!()
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
}

/// A tool's result: `value` as `structuredContent`, mirrored as JSON text.
fn structured(value: &impl Serialize, is_error: bool) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) if is_error => CallToolResult::structured_error(value),
        Ok(value) => CallToolResult::structured(value),
        // Only a map with keys that aren't text fails to serialize, and no
        // result holds one: said as a failure all the same, never a panic.
        Err(error) => CallToolResult::error(vec![rmcp::model::ContentBlock::text(
            error.to_string(),
        )]),
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
        .with_annotations(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        ),
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
        let given = request.arguments.as_ref().is_some_and(|args| !args.is_empty());
        match request.name.as_ref() {
            STATUS if given => Err(McpError::invalid_params(
                "the tool status takes no arguments",
                None,
            )),
            STATUS => Ok(self.status().await.into()),
            name => Err(McpError::invalid_params(
                format!("no tool is named {name:?}"),
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests;
