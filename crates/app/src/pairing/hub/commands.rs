//! The hub's commands to its machines (§16, decision 176): a person on the
//! hub asks a live machine to run a plan's backup or cancel one, the hub
//! sends the command on the machine's connection and waits at most
//! [`COMMAND_ANSWER_WITHIN`] for its answer. Nothing is kept to send
//! later: a machine offline is told so at once.

use api::ErrorCode;
use domain::AgentId;
use domain::access::SessionTag;
use domain::fleet::{COMMAND_ANSWER_WITHIN, HubCommand};

use super::HubPairing;
use crate::hub_commands::CommandAnswer;

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
    /// [`COMMAND_ANSWER_WITHIN`] for its answer.
    pub async fn command_asked(
        &self,
        agent: AgentId,
        command: HubCommand,
        asked_by: Option<SessionTag>,
    ) -> Result<CommandAnswer, CommandAskRefused> {
        let _ = (agent, command, asked_by, COMMAND_ANSWER_WITHIN);
        todo!()
    }
}
