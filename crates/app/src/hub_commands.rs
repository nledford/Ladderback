//! A machine's answers to its hub's commands (technical design §7.2, §10.3;
//! §16, decisions 120 and 176): run a plan's backup now, through the same
//! use case as Back up now, or cancel a backup it runs. The machine acts
//! on a command only while its configuration lets the hub command it, only
//! when the hub asked it within [`COMMAND_LIFE`](domain::fleet::COMMAND_LIFE)
//! of the hub's time, and only once: it remembers each command's ID in its
//! [`CommandLedger`], so a command replayed, after a restart too, is
//! refused. The hub starts at most one backup of a plan every
//! [`HUB_RUN_SPACING`](domain::fleet::HUB_RUN_SPACING) and
//! [`HUB_RUNS_PER_HOUR`](domain::fleet::HUB_RUNS_PER_HOUR) in an hour. Each
//! command is answered with what the
//! machine did, or the code of why it did nothing.

use std::sync::Arc;

use api::ErrorCode;
use async_trait::async_trait;
use domain::access::SessionTag;
use domain::fleet::{
    Accepts, COMMANDS_REMEMBERED_FOR, CommandRequest, EpochSeconds, HubCommand, HubRun,
    command_fresh, hub_run_admitted,
};
use domain::planning::HubCommands;
use domain::{OperationId, PlanId, SignedDuration, Timestamp};

use crate::pairing::{AgentChannel, ChannelError};
use crate::ports::{Clock, CommandLedger, ConfigStore, Remembered};

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

    /// Admits a backup of `plan` the hub's session `asked_by` asked for,
    /// as a new operation, and returns once the operation log has it
    /// queued: on a task of its own, the backup then reads its
    /// repository's secrets and runs, or ends failed with why it can't
    /// (decision 176).
    async fn queue(
        &self,
        plan: &PlanId,
        asked_by: Option<SessionTag>,
    ) -> Result<OperationId, HubRunRefused> {
        let _ = (plan, asked_by);
        todo!()
    }

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
/// the hub signed at `welcomed_at`, `since` after it sent its `Hello` on
/// the machine's monotonic clock: what the hub's time was then. It sent
/// its `Hello` at `hello_sent` on that clock, by which it judges the hub's
/// time again when it acts on the command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedCommand {
    pub request: CommandRequest,
    pub welcomed_at: EpochSeconds,
    pub since: SignedDuration,
    pub hello_sent: SignedDuration,
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
        if self.allowed().await {
            Accepts::all()
        } else {
            Accepts::none()
        }
    }

    /// The answer to `command`; none when the machine can't tell whether it
    /// took it before, as when its database can't be read: the hub then
    /// says it had no answer, and nothing ran.
    pub async fn answer(&self, command: &ReceivedCommand) -> Option<CommandAnswer> {
        if !self.allowed().await {
            return Some(refused(ErrorCode::MachineCommandsOff));
        }
        let request = &command.request;
        // Fresh when it came, and still when the machine acts on it, after
        // however long it was held.
        let acting = self.clock.monotonic().saturating_sub(command.hello_sent);
        if !command_fresh(command.welcomed_at, command.since, request.requested_at)
            || !command_fresh(command.welcomed_at, acting, request.requested_at)
        {
            return Some(refused(ErrorCode::CommandExpired));
        }
        let now = self.clock.now();
        let remembered = self
            .ledger
            .remember(request.id, now)
            .await
            .inspect_err(|error| log::error!("the hub's command couldn't be remembered: {error}"))
            .ok()?;
        match remembered {
            Remembered::New => {}
            Remembered::Seen => return Some(refused(ErrorCode::CommandReplayed)),
            Remembered::Full { oldest } => {
                return Some(rate_limited(
                    oldest.checked_add(COMMANDS_REMEMBERED_FOR).ok(),
                ));
            }
        }
        match &request.command {
            HubCommand::RunPlan { plan } => self.run(plan, request.asked_by.clone(), now).await,
            HubCommand::CancelOp { op } => Some(match self.runs.cancel(op).await {
                Ok(()) => CommandAnswer::Canceled(*op),
                Err(HubCancelRefused::Unknown) => refused(ErrorCode::OperationUnknown),
                Err(HubCancelRefused::NotRunning) => refused(ErrorCode::OperationNotRunning),
                Err(HubCancelRefused::NotABackup) => refused(ErrorCode::CommandBackupsOnly),
            }),
        }
    }

    /// Answers each command `channel` holds, then waits for the hub's next
    /// and answers it.
    pub async fn answer_next(&self, channel: &mut AgentChannel) -> Result<(), ChannelError> {
        self.answer_held(channel).await?;
        let command = channel.listen_command().await?;
        self.reply(channel, &command).await
    }

    /// Answers each command `channel` took while the agent waited for
    /// another message, and each it dropped to make room.
    pub async fn answer_held(&self, channel: &mut AgentChannel) -> Result<(), ChannelError> {
        channel.answer_dropped().await?;
        while let Some(command) = channel.buffered_command() {
            self.reply(channel, &command).await?;
        }
        Ok(())
    }

    /// Answers `command` on `channel`, when the machine has an answer,
    /// after each command the channel dropped to make room for it.
    pub async fn reply(
        &self,
        channel: &mut AgentChannel,
        command: &ReceivedCommand,
    ) -> Result<(), ChannelError> {
        channel.answer_dropped().await?;
        match self.answer(command).await {
            Some(answer) => {
                channel
                    .send_command_result(command.request.id, &answer)
                    .await
            }
            None => Ok(()),
        }
    }

    /// Whether the machine's configuration lets the hub command it: not
    /// when it turns the commands off, nor when it can't be read.
    async fn allowed(&self) -> bool {
        match self.config.load().await {
            Ok(config) => config.agent().hub_commands == HubCommands::On,
            Err(error) => {
                log::warn!("the configuration couldn't be read for the hub's commands: {error}");
                false
            }
        }
    }

    /// A backup of `plan` the hub's session `asked_by` asked for at `now`:
    /// the plan's run already going, or a new one within the hub's limits.
    async fn run(
        &self,
        plan: &PlanId,
        asked_by: Option<SessionTag>,
        now: Timestamp,
    ) -> Option<CommandAnswer> {
        if let Some(going) = self.runs.going(plan).await {
            return Some(CommandAnswer::Started(going));
        }
        let runs = self
            .ledger
            .runs()
            .await
            .inspect_err(|error| log::error!("the hub's runs couldn't be read: {error}"))
            .ok()?;
        if let Err(again) = hub_run_admitted(&runs, plan, now) {
            return Some(rate_limited(Some(again)));
        }
        Some(match self.runs.start(plan, asked_by).await {
            Ok(operation) => {
                let run = HubRun {
                    plan: plan.clone(),
                    at: now,
                };
                if let Err(error) = self.ledger.ran(&run).await {
                    log::error!("the hub's run couldn't be kept: {error}");
                }
                CommandAnswer::Started(operation)
            }
            Err(HubRunRefused::UnknownPlan) => refused(ErrorCode::PlanUnknown),
            Err(HubRunRefused::RunWaiting) => refused(ErrorCode::PlanRunWaiting),
            Err(HubRunRefused::KeyFilesNotLeftOut) => refused(ErrorCode::KeyFilesNotLeftOut),
            Err(HubRunRefused::Refused) => refused(ErrorCode::MachineRunRefused),
        })
    }
}

fn refused(code: ErrorCode) -> CommandAnswer {
    CommandAnswer::Refused {
        code,
        retry_at: None,
    }
}

fn rate_limited(retry_at: Option<Timestamp>) -> CommandAnswer {
    CommandAnswer::Refused {
        code: ErrorCode::CommandRateLimited,
        retry_at,
    }
}

#[cfg(test)]
mod tests;
