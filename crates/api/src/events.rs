//! The events the daemon sends a page over its events stream.

use serde::{Deserialize, Serialize};

use crate::{ErrorCode, MsgArg, OperationStep, ProgressUnit, ProposalStatusDto, WarningCode};

/// One live update the daemon pushes to a connected page over the events
/// WebSocket (technical design §4.4): what an operation reported, or that an
/// agent came or went. Identifiers are text, counts are numbers, and times
/// are RFC 3339 strings. The daemon forwards an operation's progress and
/// warning events unchanged: coalescing is the worker's, once (§4.1). How an
/// operation started and how it ended are announced by the run that recorded
/// them, once the history has each (§4.4): a page that reads the history on
/// hearing either finds it there. A read that is no operation tells nothing:
/// no start, end, progress or warning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UiEvent {
    /// An operation left the queue and is running, and the history says so.
    /// `started_at` is the history's start time.
    OperationStarted {
        operation: String,
        repository: Option<String>,
        started_at: String,
    },
    /// A running operation moved forward: in `step`, `current` of `total`,
    /// when it says, counted in `unit`. What a page is shown is one progress
    /// bar, the one the engine began last and still has open (§4.4), so its
    /// numbers are that step's. `step` is `null` when the engine names no
    /// step this build knows, and is left out by a daemon older than the
    /// field; `unit` is left out by a daemon older than its field, and then
    /// reads as a count. Progress is transient: it is never stored, and a
    /// subscriber whose buffer fills loses the oldest of these first.
    OperationProgress {
        operation: String,
        #[serde(default)]
        step: Option<OperationStep>,
        #[serde(default)]
        unit: ProgressUnit,
        current: u64,
        total: Option<u64>,
    },
    /// A problem with one item that doesn't fail the operation.
    OperationWarning {
        operation: String,
        code: WarningCode,
        args: Vec<MsgArg>,
    },
    /// The operation ended well. The run that recorded it announces this once,
    /// after its ending hooks and its last write to the operation log, so a
    /// page that reads the history on hearing it finds the end there, unless
    /// the log lost that write. A cancelled operation has no such event.
    OperationSucceeded {
        operation: String,
        finished_at: String,
    },
    /// The operation ended badly: announced like [`Self::OperationSucceeded`],
    /// for a failure the run found out itself as well as the engine's own.
    /// `args` are the failure's arguments by position, then the engine's own
    /// words ([`ErrorDto::positional`](crate::ErrorDto::positional)).
    OperationFailed {
        operation: String,
        code: ErrorCode,
        args: Vec<MsgArg>,
        finished_at: String,
    },
    /// A plan's backup failed, and the agent retries it (`backup.retrying`,
    /// §5, §11): `operation` is the run that failed, `attempt` which of the
    /// plan's `attempts` retries this is, and `at` when it runs. Told once,
    /// when the retry is planned.
    BackupRetrying {
        operation: String,
        plan: String,
        attempt: u32,
        attempts: u32,
        at: String,
    },
    /// An agent joined the fleet.
    AgentOnline { agent: String, at: String },
    /// An agent left the fleet.
    AgentOffline { agent: String, at: String },
    /// A proposal's status changed (decision 89, point 14): facts only,
    /// never its rationale, its changes or its proposer's label. A page
    /// that shows proposals reads them again on hearing it.
    ProposalChanged {
        proposal: String,
        status: ProposalStatusDto,
    },
}

#[cfg(test)]
mod tests;
