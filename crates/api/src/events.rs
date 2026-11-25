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
    /// reads as a count. `at` is when the daemon heard the report, in its
    /// own clock, the one `started_at` is in, so a page can say when a run
    /// without a total last reported (§4.5); a daemon older than the field
    /// leaves it out. Progress is transient: it is never stored, and a
    /// subscriber whose buffer fills loses the oldest of these first.
    OperationProgress {
        operation: String,
        #[serde(default)]
        step: Option<OperationStep>,
        #[serde(default)]
        unit: ProgressUnit,
        current: u64,
        total: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
    /// A problem with one item that doesn't fail the operation. `args`, the
    /// item and the engine's own words, are empty for a program without the
    /// permission Snapshot contents: either can name a file.
    OperationWarning {
        operation: String,
        code: WarningCode,
        args: Vec<MsgArg>,
    },
    /// The operation ended well. The run that recorded it announces this once,
    /// after its ending hooks and its last write to the operation log, so a
    /// page that reads the history on hearing it finds the end there, unless
    /// the log lost that write.
    OperationSucceeded {
        operation: String,
        finished_at: String,
    },
    /// The operation ended badly: announced like [`Self::OperationSucceeded`],
    /// for a failure the run found out itself as well as the engine's own.
    /// `args` are the failure's arguments by position, then the engine's own
    /// words ([`ErrorDto::positional`](crate::ErrorDto::positional)): empty
    /// for a program without the permission Snapshot contents, as either can
    /// name a file.
    OperationFailed {
        operation: String,
        code: ErrorCode,
        args: Vec<MsgArg>,
        finished_at: String,
    },
    /// The operation was cancelled: announced like
    /// [`Self::OperationSucceeded`], once the run recorded the cancel. A
    /// paired machine reports it to its hub as the operation's end (§7.2).
    OperationCancelled {
        operation: String,
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
    /// The machine `agent` came online at `at` (RFC 3339): the hub holds
    /// its sync connection (§7.2; §16, decision 159). A page that shows the
    /// hub's machines reads them again on hearing it.
    AgentOnline { agent: String, at: String },
    /// The machine `agent` went offline at `at`: the hub let go of its sync
    /// connection, which closed or sent nothing for 90 seconds.
    AgentOffline { agent: String, at: String },
    /// A machine paired with this hub and waits for a person's approval
    /// (§16, decision 121, "Waiting for approval"). Told only to a browser
    /// that sees the hub's machines; a page that lists them reads them
    /// again on hearing it.
    AgentPending {
        agent: String,
        hostname: String,
        at: String,
    },
    /// A person approved the machine `agent`.
    AgentApproved {
        agent: String,
        hostname: String,
        at: String,
    },
    /// A person removed the machine `agent`, or rejected it while it
    /// waited (§16, decision 120, "Removing, re-keying and recovering an
    /// agent"). A page that lists the hub's machines reads them again on
    /// hearing it.
    AgentRemoved {
        agent: String,
        hostname: String,
        at: String,
    },
    /// The hub took a report of the machine `agent`'s operation `operation`
    /// (§7.2): that it started, or how it ended. A page that shows the
    /// machine's operations reads them again on hearing it.
    AgentReported {
        agent: String,
        operation: String,
        outcome: crate::MachineOutcomeDto,
    },
    /// The hub took a snapshot of the machine `agent`'s plans (§7.2). A
    /// page that shows the machine's plans reads them again on hearing it.
    AgentPlans { agent: String },
    /// The hub took the latest progress of the machine `agent`'s running
    /// operation (§7.2), which a page that shows it shows as it comes.
    AgentProgress {
        agent: String,
        progress: crate::MachineProgressDto,
    },
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
