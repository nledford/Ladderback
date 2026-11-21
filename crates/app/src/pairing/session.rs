use api::sync::{AgentFrame, HubFrame};
use domain::fleet::{AgentRecord, KEY_BYTES};

use super::hub::{HubHandshake, HubKeys, HubPairing, HubPairingError, MessageRefused, ProofAnswer};
use super::wire::{AgentMessage, agent_message, answer_frame, challenge_frame, hub_envelope_frame};
use domain::fleet::PairAnswer;

/// The close reason of a message out of turn or one that doesn't read: a
/// second `Connect`, a `Pair` before the `Challenge`, a frame that isn't
/// one, a signed message of a kind the hub doesn't take there (decision
/// 120, "Bounds before the proofs").
pub const PROTOCOL_ERROR: &str = "protocol-error";
/// The close reason of a `Connect` naming the hub's own TLS on a session
/// without it (decision 121, `binding-unavailable`).
pub const BINDING_UNAVAILABLE: &str = "binding-unavailable";
/// The close reason of a signed message that doesn't hold, or whose
/// sequence number isn't the next (decision 120, "Signed messages after
/// the handshake").
pub const SEQUENCE_ERROR: &str = "sequence-error";

/// One connection on the hub's `/sync/v1`, from the agent's `Connect` to
/// the hub's answer to its `Pair`, or to its `Welcome` once the agent
/// proved itself.
#[derive(Debug)]
pub struct HubSession {
    keys: HubKeys,
    exporter: Option<[u8; KEY_BYTES]>,
    stage: Stage,
}

/// How far a connection has come.
#[derive(Debug)]
enum Stage {
    /// Nothing yet: the hub waits for `Connect`.
    Opened,
    /// The hub sent its `Challenge` and waits for `Pair` or `Proof`.
    Challenged(HubHandshake),
    /// The agent proved itself as `AgentRecord`: the hub waits for `Hello`.
    Proved(HubHandshake, AgentRecord),
    /// The hub answered the last message it takes.
    Done,
}

/// What the transport does after a frame.
#[derive(Debug)]
pub enum HubStep {
    /// Send the frame and wait for the agent's next.
    Reply(HubFrame),
    /// Send nothing and wait for the agent's next frame.
    Wait,
    /// Send the frame and close: the pairing or the greeting is answered.
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
        Ok(HubSession {
            keys: self.keys().await?,
            exporter,
            stage: Stage::Opened,
        })
    }

    /// The hub's step on the agent's next frame: a `Challenge` to its
    /// `Connect`, the answer to its `Pair`, nothing to a `Proof` that holds
    /// and a signed refusal to one that doesn't, a signed `Welcome` to its
    /// `Hello`, and a closed connection for a message out of turn.
    pub async fn step(&self, session: &mut HubSession, frame: &AgentFrame) -> HubStep {
        let Ok(message) = agent_message(frame) else {
            return HubStep::Close(PROTOCOL_ERROR);
        };
        let stage = std::mem::replace(&mut session.stage, Stage::Done);
        match (message, stage) {
            (AgentMessage::Connect(connect), Stage::Opened) => {
                match self.challenge(&session.keys, &connect, session.exporter) {
                    Ok((challenge, handshake)) => {
                        session.stage = Stage::Challenged(handshake);
                        HubStep::Reply(challenge_frame(&challenge))
                    }
                    Err(HubPairingError::BindingUnavailable) => HubStep::Close(BINDING_UNAVAILABLE),
                    Err(error) => HubStep::Fail(error),
                }
            }
            (AgentMessage::Pair(pair), Stage::Challenged(handshake)) => {
                match self.pair(&session.keys, &handshake, &pair).await {
                    Ok(answer) => HubStep::Last(answer_frame(&answer)),
                    Err(error) => HubStep::Fail(error),
                }
            }
            (AgentMessage::Proof(proof), Stage::Challenged(handshake)) => {
                match self.prove(&session.keys, &handshake, &proof).await {
                    Ok(ProofAnswer::Proved(agent)) => {
                        session.stage = Stage::Proved(handshake, agent);
                        HubStep::Wait
                    }
                    Ok(ProofAnswer::Refused(refused)) => {
                        HubStep::Last(answer_frame(&PairAnswer::Refused(refused)))
                    }
                    Err(error) => HubStep::Fail(error),
                }
            }
            (AgentMessage::Signed(hello), Stage::Proved(handshake, agent)) => {
                match self
                    .welcome(&session.keys, &handshake, &agent, &hello)
                    .await
                {
                    Ok(Ok(welcome)) => HubStep::Last(hub_envelope_frame(&welcome)),
                    Ok(Err(MessageRefused::Unexpected)) => HubStep::Close(PROTOCOL_ERROR),
                    Ok(Err(MessageRefused::Unsigned | MessageRefused::OutOfSequence)) => {
                        HubStep::Close(SEQUENCE_ERROR)
                    }
                    Err(error) => HubStep::Fail(error),
                }
            }
            _ => HubStep::Close(PROTOCOL_ERROR),
        }
    }
}

#[cfg(test)]
mod tests;
