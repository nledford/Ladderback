//! The hub's commands to its machines (§16, decision 176): a person on the
//! hub asks a live machine to run a plan's backup or cancel one, the hub
//! sends the command on the machine's connection and waits at most
//! [`COMMAND_ANSWER_WITHIN`] for its answer. Nothing is kept to send
//! later: a machine offline is told so at once.

use std::sync::PoisonError;

use api::ErrorCode;
use domain::AgentId;
use domain::access::SessionTag;
use domain::fleet::{
    AgentStatus, COMMAND_ANSWER_WITHIN, CommandId, CommandRequest, EpochSeconds, HubCommand,
    MAX_COMMANDS_IN_FLIGHT,
};
use futures::channel::oneshot;
use futures::future::{Either, select};

use super::presence::{Awaited, OutgoingCommand};
use super::{Answered, HubPairing, MessageRefused};
use crate::hub_commands::CommandAnswer;
use crate::pairing::wire::{command_payload, read_command_result};

/// How many random bytes a command's ID holds.
const ID_RANDOM_BYTES: usize = 10;

/// Why the hub didn't get a machine's answer to its command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAskRefused {
    /// The hub has no approved machine of that ID.
    MachineUnknown,
    /// The machine isn't connected: nothing was sent.
    MachineOffline,
    /// The machine runs a release that takes no commands, or not this one.
    CommandsUnsupported,
    /// The machine turned its hub's commands off.
    CommandsOff,
    /// As many commands wait for the machine's answer as it takes at once.
    Busy,
    /// The machine didn't answer in time, or its connection ended first.
    NoAnswer,
    /// The hub couldn't read its machines or make the command's ID.
    Unavailable,
}

impl CommandAskRefused {
    /// The code a person asking is answered with.
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::MachineUnknown => ErrorCode::MachineUnknown,
            Self::MachineOffline => ErrorCode::MachineOffline,
            Self::CommandsUnsupported => ErrorCode::MachineCommandsUnsupported,
            Self::CommandsOff => ErrorCode::MachineCommandsOff,
            Self::Busy => ErrorCode::CommandRateLimited,
            Self::NoAnswer => ErrorCode::MachineNoAnswer,
            Self::Unavailable => ErrorCode::HubUnavailable,
        }
    }
}

impl HubPairing {
    /// Sends the machine `agent` the hub's `command`, which the hub's
    /// session `asked_by` asked for, and waits up to
    /// [`COMMAND_ANSWER_WITHIN`] for its answer. A machine unknown,
    /// offline or that doesn't take the command is refused at once.
    pub async fn command_asked(
        &self,
        agent: AgentId,
        command: HubCommand,
        asked_by: Option<SessionTag>,
    ) -> Result<CommandAnswer, CommandAskRefused> {
        match self.registry.agent(agent).await {
            Ok(Some(record)) if matches!(record.status, AgentStatus::Approved { .. }) => {}
            Ok(_) => return Err(CommandAskRefused::MachineUnknown),
            Err(error) => {
                log::warn!("the hub couldn't read the machine it was asked to command: {error:?}");
                return Err(CommandAskRefused::Unavailable);
            }
        }
        let Some((accepts, mut line)) = self.command_line(agent) else {
            return Err(CommandAskRefused::MachineOffline);
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
                return Err(CommandAskRefused::Busy);
            }
            awaited.insert(id, Awaited { agent, answer });
        }
        if let Err(error) = line.try_send(OutgoingCommand { kind, payload }) {
            self.forget_command(id);
            return Err(if error.is_full() {
                CommandAskRefused::Busy
            } else {
                CommandAskRefused::MachineOffline
            });
        }
        let within = std::pin::pin!(self.clock.sleep(COMMAND_ANSWER_WITHIN));
        let heard = match select(answered, within).await {
            Either::Left((Ok(answer), _)) => Ok(answer),
            Either::Left((Err(_), _)) | Either::Right(_) => Err(CommandAskRefused::NoAnswer),
        };
        self.forget_command(id);
        heard
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
