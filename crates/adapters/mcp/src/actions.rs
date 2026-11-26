//! The tools that act (§4.6): `run_plan` starts a backup, `wait_operation`
//! waits for an operation to end, `cancel_operation` stops a backup. Their
//! arguments and results, and how long a wait may last.

use std::time::Duration;

use api::{ErrorCode, ErrorDto, MsgArg};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pages::Operation;

/// How long `wait_operation` waits when the call names no `timeout`.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(60);

/// The longest `wait_operation` waits: a longer `timeout` is clamped to it,
/// never refused, as a page's `limit` is.
pub const MAX_WAIT: Duration = Duration::from_secs(600);

/// How often a wait reads the operation again, and sends a progress
/// notification when the call asked for them.
pub const WAIT_STEP: Duration = Duration::from_millis(500);

/// The one kind of operation MCP may cancel, as an API token may (decision
/// 93): maintenance runs to its end, so a loop can't keep a repository from
/// being pruned.
pub const CANCELLABLE_KIND: &str = "backup";

/// The refusal of a cancel of anything but a backup: the one the daemon
/// gives an API token, `api-token-permission` naming `session`, so both
/// backends refuse alike (decision 93).
pub fn not_cancellable() -> ErrorDto {
    ErrorDto::new(
        ErrorCode::ApiTokenPermission,
        [("permission", MsgArg::Text("session".to_owned()))],
        None,
    )
}

/// `timeout`, in seconds, as how long a wait lasts: [`DEFAULT_WAIT`] when
/// not given, and at most [`MAX_WAIT`]. Zero reads the operation once.
pub fn wait_length(timeout: Option<u64>) -> Duration {
    timeout
        .map_or(DEFAULT_WAIT, Duration::from_secs)
        .min(MAX_WAIT)
}

/// The arguments of `run_plan`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RunArguments {
    /// The plan's ID, as `list_plans` names it.
    pub plan: String,
}

/// What `run_plan` returns: the ID of the backup it queued.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct Started {
    pub operation: String,
}

/// The arguments of `wait_operation`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct WaitArguments {
    /// The operation's ID, as `run_plan` or `list_operations` names it.
    pub operation: String,
    /// How many seconds to wait at most, a whole number from 0 to 600; 60
    /// when not given. A larger number waits 600; 0 reads the operation once.
    pub timeout: Option<u64>,
}

/// What `wait_operation` returns: whether the operation finished before the
/// wait ended, and the operation as it stood then.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct Waited {
    pub finished: bool,
    #[serde(flatten)]
    pub operation: Operation,
}

/// The arguments of `cancel_operation`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CancelArguments {
    /// The backup's ID, as `run_plan` or `list_operations` names it.
    pub operation: String,
}

/// What `cancel_operation` returns: the operation asked to stop. It ends as
/// cancelled once its worker has stopped and its ending hooks have run,
/// which `wait_operation` tells.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct Canceling {
    pub operation: String,
}
