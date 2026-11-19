use api::sync::{AgentFrame, HubFrame};
use domain::fleet::KEY_BYTES;

use super::hub::{HubHandshake, HubKeys, HubPairing, HubPairingError};
use super::wire::{AgentMessage, agent_message, answer_frame, challenge_frame};

/// The close reason of a message out of turn or one that doesn't read: a
/// second `Connect`, a `Pair` before the `Challenge`, a frame that isn't
/// one (decision 120, "Bounds before the proofs").
pub const PROTOCOL_ERROR: &str = "protocol-error";
/// The close reason of a `Connect` naming the hub's own TLS on a session
/// without it (decision 121, `binding-unavailable`).
pub const BINDING_UNAVAILABLE: &str = "binding-unavailable";

/// One connection on the hub's `/sync/v1`, from the agent's `Connect` to
/// the hub's answer to its `Pair`.
#[derive(Debug)]
pub struct HubSession {
    keys: HubKeys,
    exporter: Option<[u8; KEY_BYTES]>,
    handshake: Option<HubHandshake>,
}

/// What the transport does after a frame.
#[derive(Debug)]
pub enum HubStep {
    /// Send the frame and wait for the agent's next.
    Reply(HubFrame),
    /// Send the frame and close: the pairing is answered.
    Last(HubFrame),
    /// Close with this reason, unanswered.
    Close(&'static str),
    /// Close: the hub couldn't take the step. The transport logs the error.
    Fail(HubPairingError),
}

impl HubPairing {
    /// A session on a connection whose `tls-exporter` value is `exporter`,
    /// `None` on any but the hub's own TLS. The hub's keys are read, or
    /// made the first time, once per connection.
    pub async fn session(
        &self,
        exporter: Option<[u8; KEY_BYTES]>,
    ) -> Result<HubSession, HubPairingError> {
        todo!()
    }

    /// The hub's step on the agent's next frame: a `Challenge` to its
    /// `Connect`, the answer to its `Pair`, and a closed connection for a
    /// message out of turn.
    pub async fn step(&self, session: &mut HubSession, frame: &AgentFrame) -> HubStep {
        todo!()
    }
}

#[cfg(test)]
mod tests;
