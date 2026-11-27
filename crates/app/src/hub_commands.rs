//! A machine's answers to its hub's commands (technical design §7.2, §10.3;
//! §16, decisions 120 and 176): run a plan's backup now, through the same
//! use case as Back up now, or cancel a backup it runs. The machine acts
//! on a command only while its configuration lets the hub command it, only
//! when the hub asked it within [`COMMAND_LIFE`] of the hub's time, and
//! only once: it remembers each command's ID in its [`CommandLedger`], so a
//! command replayed, after a restart too, is refused. The hub starts at
//! most one backup of a plan every [`HUB_RUN_SPACING`] and
//! [`HUB_RUNS_PER_HOUR`] in an hour. Each command is answered with what the
//! machine did, or the code of why it did nothing.

use std::sync::Arc;

use api::ErrorCode;
use async_trait::async_trait;
use domain::access::SessionTag;
use domain::fleet::{
    Accepts, COMMAND_LIFE, CommandRequest, EpochSeconds, HUB_RUN_SPACING, HUB_RUNS_PER_HOUR,
};
use domain::{OperationId, PlanId, SignedDuration, Timestamp};

use crate::pairing::{AgentChannel, ChannelError};
use crate::ports::{Clock, CommandLedger, ConfigStore};

/// What starts and cancels the backups the hub asks for: the composition
/// root's plan runner, the same use case as Back up now. Not a port: it is
/// the use cases the root wires, as [`crate::schedule::PlanRuns`] is.
#[async_trait]
pub trait HubRuns: Send + Sync {
    /// The backup of `plan` running or waiting for its turn, if one is.
    async fn going(&self, plan: &PlanId) -> Option<OperationId>;

    /// Starts a backup of `plan` the hub's session `asked_by` asked for,
    /// as a new operation, and returns once the operation log has it.
    async fn start(
        &self,
        plan: &PlanId,
        asked_by: Option<SessionTag>,
    ) -> Result<OperationId, HubRunRefused>;

    /// Cancels the backup `operation`.
    async fn cancel(&self, operation: &OperationId) -> Result<(), HubCancelRefused>;
}

/// Why a backup the hub asked for didn't start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubRunRefused {
    /// The machine has no plan of that name.
    UnknownPlan,
    /// A run of the plan is waiting for its turn already.
    RunWaiting,
    /// The plan's sources hold the data folder's secret key files, which
    /// no glob can leave out.
    KeyFilesNotLeftOut,
    /// Anything else: the machine's own log and page say why.
    Refused,
}

/// Why an operation the hub asked to cancel wasn't.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubCancelRefused {
    /// The machine's log has no operation of that ID.
    Unknown,
    /// The operation has ended.
    NotRunning,
    /// The operation isn't a backup: the hub cancels only backups.
    NotABackup,
}

/// A command the machine took from its hub on a connection whose `Welcome`
/// the hub signed at `welcomed_at`, `since` before it took it on the
/// machine's monotonic clock: what the hub's time was then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedCommand {
    pub request: CommandRequest,
    pub welcomed_at: EpochSeconds,
    pub since: SignedDuration,
}

/// The machine's answer to a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandAnswer {
    /// The backup runs as `operation`: one it started, or the plan's run
    /// already going.
    Started(OperationId),
    /// The backup `operation` was canceled.
    Canceled(OperationId),
    /// Nothing was done, for the reason `code` says; with
    /// [`ErrorCode::CommandRateLimited`], `retry_at` says when the hub may
    /// ask again.
    Refused {
        code: ErrorCode,
        retry_at: Option<Timestamp>,
    },
}

/// The machine's side of its hub's commands.
pub struct AnswerHubCommands {
    config: Arc<dyn ConfigStore>,
    ledger: Arc<dyn CommandLedger>,
    runs: Arc<dyn HubRuns>,
    clock: Arc<dyn Clock>,
}

impl AnswerHubCommands {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        ledger: Arc<dyn CommandLedger>,
        runs: Arc<dyn HubRuns>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            ledger,
            runs,
            clock,
        }
    }

    /// The commands the machine's `Hello` says it takes: every one, unless
    /// its configuration turns its hub's commands off or can't be read.
    pub async fn accepts(&self) -> Accepts {
        let _ = (&self.config, Accepts::all());
        todo!()
    }

    /// The answer to `command`; none when the machine can't tell whether it
    /// took it before, as when its database can't be read: the hub then
    /// says it had no answer, and nothing ran.
    pub async fn answer(&self, command: &ReceivedCommand) -> Option<CommandAnswer> {
        let _ = (
            command,
            &self.ledger,
            &self.runs,
            &self.clock,
            COMMAND_LIFE,
            HUB_RUN_SPACING,
            HUB_RUNS_PER_HOUR,
        );
        todo!()
    }

    /// Answers each command `channel` holds, then waits for the hub's next
    /// and answers it.
    pub async fn answer_next(&self, channel: &mut AgentChannel) -> Result<(), ChannelError> {
        let _ = channel;
        todo!()
    }
}

#[cfg(test)]
mod tests;
