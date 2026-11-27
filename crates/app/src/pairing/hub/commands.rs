//! The hub's commands to its machines (§16, decision 176): a person on the
//! hub asks a live machine to run a plan's backup or cancel one, the hub
//! sends the command on the machine's connection and waits at most
//! [`COMMAND_ANSWER_WITHIN`] for its answer. Nothing is kept to send
//! later: a machine offline is told so at once.

use std::sync::PoisonError;

use api::ErrorCode;
use domain::access::SessionTag;
use domain::fleet::{
    AgentStatus, COMMAND_ANSWER_WITHIN, CommandId, CommandRequest, EpochSeconds, HubCommand,
    MAX_COMMANDS_IN_FLIGHT, needs_sign_in,
};
use domain::{AgentId, Timestamp};
use futures::channel::oneshot;
use futures::future::{Either, select};

use super::presence::{Awaited, OutgoingCommand};
use super::{Answered, HubPairing, MessageRefused};
use crate::hub_commands::CommandAnswer;
use crate::pairing::wire::{command_payload, read_command_result};

/// How many random bytes a command's ID holds.
const ID_RANDOM_BYTES: usize = 10;

/// A command a person on the hub asks a machine for: the machine, the
/// command, the person's session, and whether the ask came from beyond
/// loopback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandAsk {
    pub agent: AgentId,
    pub command: HubCommand,
    pub asked_by: Option<SessionTag>,
    pub beyond_loopback: bool,
}

/// Why the hub didn't get a machine's answer to its command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAskRefused {
    /// Asked from beyond loopback with sign-in off: nothing was sent.
    NeedsSignIn,
    /// The hub has no approved machine of that ID.
    MachineUnknown,
    /// The machine isn't connected: nothing was sent.
    MachineOffline,
    /// As [`Self::MachineOffline`], for a run of a plan whose next run on
    /// its schedule, as the machine last reported it, is `next_run`, still
    /// to come.
    MachineOfflineNextRun { next_run: Timestamp },
    /// The machine runs a release that takes no commands, or not this one.
    CommandsUnsupported,
    /// The machine turned its hub's commands off.
    CommandsOff,
    /// As many commands wait for the machine's answer as it takes at once;
    /// one has its answer by `retry_at`.
    Busy { retry_at: Timestamp },
    /// The machine didn't answer in time, or its connection ended first.
    NoAnswer,
    /// The hub couldn't read its machines or make the command's ID.
    Unavailable,
}

impl CommandAskRefused {
    /// The code a person asking is answered with.
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::NeedsSignIn => ErrorCode::PairingNeedsSignIn,
            Self::MachineUnknown => ErrorCode::MachineUnknown,
            Self::MachineOffline => ErrorCode::MachineOffline,
            Self::MachineOfflineNextRun { .. } => ErrorCode::MachineOfflineNextRun,
            Self::CommandsUnsupported => ErrorCode::MachineCommandsUnsupported,
            Self::CommandsOff => ErrorCode::MachineCommandsOff,
            Self::Busy { .. } => ErrorCode::CommandRateLimited,
            Self::NoAnswer => ErrorCode::MachineNoAnswer,
            Self::Unavailable => ErrorCode::HubUnavailable,
        }
    }
}

impl HubPairing {
    /// Sends the machine `ask` names the hub's command, which the hub's
    /// session asked for, and waits up to [`COMMAND_ANSWER_WITHIN`] for its
    /// answer. An ask from beyond loopback with sign-in off, or for a
    /// machine unknown, offline or that doesn't take the command, is
    /// refused at once.
    pub async fn command_asked(&self, ask: CommandAsk) -> Result<CommandAnswer, CommandAskRefused> {
        let CommandAsk {
            agent,
            command,
            asked_by,
            beyond_loopback,
        } = ask;
        if needs_sign_in(self.access.is_some(), beyond_loopback) {
            return Err(CommandAskRefused::NeedsSignIn);
        }
        match self.registry.agent(agent).await {
            Ok(Some(record)) if matches!(record.status, AgentStatus::Approved { .. }) => {}
            Ok(_) => return Err(CommandAskRefused::MachineUnknown),
            Err(error) => {
                log::warn!("the hub couldn't read the machine it was asked to command: {error:?}");
                return Err(CommandAskRefused::Unavailable);
            }
        }
        let Some((accepts, mut line)) = self.command_line(agent) else {
            return Err(self.offline(agent, &command).await);
        };
        match accepts {
            Some(accepts) if accepts.takes(command.kind()) => {}
            Some(accepts) if accepts.kinds().next().is_none() => {
                return Err(CommandAskRefused::CommandsOff);
            }
            _ => return Err(CommandAskRefused::CommandsUnsupported),
        }
        let request = self.request(command, asked_by)?;
        let id = request.id;
        let (kind, payload) = command_payload(&request);
        let (answer, answered) = oneshot::channel();
        {
            let mut awaited = self
                .presence
                .awaited
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let waiting = awaited.values().filter(|held| held.agent == agent).count();
            if waiting >= MAX_COMMANDS_IN_FLIGHT {
                return Err(self.busy());
            }
            awaited.insert(id, Awaited { agent, answer });
        }
        // Waited on until this ask ends, however it ends: an asker that
        // stops waiting, as a page closed, leaves no command counted.
        let _waiting = Waiting { pairing: self, id };
        if let Err(error) = line.try_send(OutgoingCommand { kind, payload }) {
            return Err(if error.is_full() {
                self.busy()
            } else {
                CommandAskRefused::MachineOffline
            });
        }
        let within = std::pin::pin!(self.clock.sleep(COMMAND_ANSWER_WITHIN));
        match select(answered, within).await {
            Either::Left((Ok(answer), _)) => Ok(answer),
            Either::Left((Err(_), _)) | Either::Right(_) => Err(CommandAskRefused::NoAnswer),
        }
    }

    /// The refusal of `command` to the machine `agent`, offline: for a run
    /// of a plan, with its next run on its schedule as the machine last
    /// reported it, when that is still to come by the hub's clock.
    async fn offline(&self, agent: AgentId, command: &HubCommand) -> CommandAskRefused {
        let (HubCommand::RunPlan { plan }, Some(inbox)) = (command, &self.inbox) else {
            return CommandAskRefused::MachineOffline;
        };
        let report = inbox.plans(agent).await.unwrap_or_else(|error| {
            log::warn!("the hub couldn't read the plans of a machine offline: {error:?}");
            None
        });
        let now = self.clock.now();
        report
            .as_ref()
            .and_then(|report| report.plans().iter().find(|shown| shown.plan == *plan))
            .and_then(|shown| shown.next_run)
            .and_then(|at| i64::try_from(at.seconds()).ok())
            .and_then(|seconds| Timestamp::from_second(seconds).ok())
            .filter(|next_run| *next_run > now)
            .map_or(CommandAskRefused::MachineOffline, |next_run| {
                CommandAskRefused::MachineOfflineNextRun { next_run }
            })
    }

    /// A command refused while as many wait as the machine takes: the
    /// oldest has its answer, or is given up on, by
    /// [`COMMAND_ANSWER_WITHIN`] from now.
    fn busy(&self) -> CommandAskRefused {
        let now = self.clock.now();
        CommandAskRefused::Busy {
            retry_at: now.checked_add(COMMAND_ANSWER_WITHIN).unwrap_or(now),
        }
    }

    /// The machine `agent`'s `CommandResult` holding `payload`: the answer
    /// goes to whoever waits for it, and is answered with nothing. One to
    /// a command no one waits for, as one answered too late, is passed
    /// over.
    pub(super) fn take_command_result(&self, agent: AgentId, payload: &[u8]) -> Answered {
        let Ok((id, answer)) = read_command_result(payload) else {
            return Err(MessageRefused::Unexpected);
        };
        let awaited = {
            let mut awaited = self
                .presence
                .awaited
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if awaited.get(&id).is_some_and(|held| held.agent == agent) {
                awaited.remove(&id)
            } else {
                None
            }
        };
        if let Some(awaited) = awaited {
            // The asker may have stopped waiting: then no one hears it.
            let _ = awaited.answer.send(answer);
        }
        Ok(None)
    }

    /// The request of `command` that `asked_by` asks for now, under a new
    /// ID.
    fn request(
        &self,
        command: HubCommand,
        asked_by: Option<SessionTag>,
    ) -> Result<CommandRequest, CommandAskRefused> {
        let now = self.clock.now();
        let token = self.random.token().map_err(|error| {
            log::warn!("the hub couldn't make its command's ID: {error:?}");
            CommandAskRefused::Unavailable
        })?;
        let random: [u8; ID_RANDOM_BYTES] = token
            .get(..ID_RANDOM_BYTES)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(CommandAskRefused::Unavailable)?;
        let unix_ms =
            u64::try_from(now.as_millisecond()).map_err(|_| CommandAskRefused::Unavailable)?;
        Ok(CommandRequest {
            id: CommandId::v7(unix_ms, &random),
            requested_at: EpochSeconds::from_timestamp(now)
                .ok_or(CommandAskRefused::Unavailable)?,
            asked_by,
            command,
        })
    }

    /// Stops waiting for the answer to the command `id`.
    fn forget_command(&self, id: CommandId) {
        self.presence
            .awaited
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }
}

/// The hub waiting for the answer to the command `id`: dropped, it stops,
/// whether the answer came, the wait timed out or the asker went away.
struct Waiting<'a> {
    pairing: &'a HubPairing,
    id: CommandId,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.pairing.forget_command(self.id);
    }
}
