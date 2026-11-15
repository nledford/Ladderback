//! The MCP server's wiring (technical design §4.6): `ladderback mcp
//! --connect` drives the daemon at the address it was given through the
//! connect backend, and names this program's build.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use api::MsgArg;
use async_trait::async_trait;
use l10n::msg;

use super::{Refused, text};

/// The server for a session that drives the daemon at `daemon`, sending the
/// API token of `token_file`, or else of `mcp-token` in the configuration
/// folder `config_dir`, when either holds one.
pub(crate) fn connect(
    daemon: ::mcp::DaemonUrl,
    token_file: Option<&Path>,
    config_dir: Option<&Path>,
) -> Result<::mcp::Server, Refused> {
    let _ = (token_file, config_dir);
    let backend = ::mcp::Connect::new(daemon).map_err(|error| Refused {
        message: msg::CLI_MCP_CANNOT_START,
        args: vec![("detail", text(error))],
    })?;
    Ok(::mcp::Server::new(
        Arc::new(backend),
        crate::build_info::running(),
    ))
}

/// The API token connect mode sends: the one in `token_file`, which must be
/// there, or else in `mcp-token` in `config_dir`, which need not be: without
/// it, no token is sent (§4.6).
///
/// # Errors
///
/// The file `--token-file` names isn't there; or the file read can't be
/// read, others can read it, or it holds no token.
fn api_token(
    token_file: Option<&Path>,
    config_dir: Option<&Path>,
) -> Result<Option<::mcp::ApiToken>, Refused> {
    let _ = (token_file, config_dir);
    todo!()
}

/// The in-process [`mcp::Backend`] of headless and scratch modes: the
/// handles the router of `serve` reads, called directly.
pub(crate) struct InProcess {
    catalog: Arc<dyn http_adapter::Catalog>,
    operations: Arc<dyn http_adapter::Operations>,
}

impl InProcess {
    /// A backend over the handles of `state`.
    pub(crate) fn over(state: &http_adapter::AppState) -> Self {
        Self {
            catalog: Arc::clone(&state.catalog),
            operations: Arc::clone(&state.operations),
        }
    }
}

#[async_trait]
impl ::mcp::Backend for InProcess {
    async fn status(&self) -> Result<::mcp::BackendStatus, api::ErrorDto> {
        let _ = (&self.catalog, &self.operations);
        todo!()
    }
}

#[cfg(test)]
mod tests;
