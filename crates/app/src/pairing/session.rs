use api::sync::{AgentFrame, HubFrame};
use domain::AgentId;
use domain::fleet::{Accepts, AgentRecord, KEY_BYTES, WireName};

use super::hub::{
    HelloAnswer, HubHandshake, HubKeys, HubPairing, HubPairingError, MessageRefused, ProofAnswer,
    Syncing,
};
use super::wire::{
    AgentMessage, agent_message, answer_frame, challenge_frame, hub_envelope_frame, read_hello,
};
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
/// The close reason of a report at a place the hub took that isn't the
/// report it took there, from an agent whose `Hello` stated its outbox: the
/// agent reconnects, and its next `Hello` is told to start a new epoch
/// (decision 177).
pub const OUTBOX_DIVERGED: &str = "outbox-diverged";

/// One connection on the hub's `/sync/v1`, from the agent's `Connect` to
/// the hub's answer to its `Pair`, or to its `Welcome` once the agent
/// proved itself, and on through an approved agent's reports.
#[derive(Debug)]
pub struct HubSession {
    keys: HubKeys,
    exporter: Option<[u8; KEY_BYTES]>,
    stage: Stage,
}

impl HubSession {
    /// The approved agent the hub welcomed on this connection, once it
    /// takes its signed messages: the transport then holds the connection
    /// open past the handshake's bounds, as long as the agent isn't idle,
    /// and keeps it as the agent's one live connection (decision 120).
    pub fn syncing_agent(&self) -> Option<AgentId> {
        match &self.stage {
            Stage::Syncing(syncing) => Some(syncing.agent_id()),
            _ => None,
        }
    }

    /// The commands the agent welcomed on this connection takes from the
    /// hub, as its `Hello` said: none before then, or when it said none
    /// (decision 176).
    pub fn accepts(&self) -> Accepts {
        match &self.stage {
            Stage::Syncing(syncing) => syncing.accepts().clone(),
            _ => Accepts::none(),
        }
    }
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
    /// The hub welcomed an approved agent and takes its signed messages.
    Syncing(Box<Syncing>),
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
    /// `None` on any but the hub's own TLS. The hub's keys are read once
    /// per connection; they were made at the hub's start ([`Self::start`]).
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
                    Ok(Ok(HelloAnswer::Refused(refused))) => {
                        HubStep::Last(answer_frame(&PairAnswer::Refused(refused)))
                    }
                    Ok(Ok(HelloAnswer::Welcome {
                        envelope,
                        outbox_stated,
                    })) => {
                        // Read again: the `Welcome` read it whole, so it
                        // reads.
                        let accepts = read_hello(&hello.payload)
                            .ok()
                            .and_then(|read| read.accepts)
                            .unwrap_or_default();
                        match self.syncing(handshake, &agent, outbox_stated, accepts) {
                            Some(syncing) => {
                                session.stage = Stage::Syncing(Box::new(syncing));
                                HubStep::Reply(hub_envelope_frame(&envelope))
                            }
                            None => HubStep::Last(hub_envelope_frame(&envelope)),
                        }
                    }
                    Ok(Err(refused)) => HubStep::Close(close_reason(refused)),
                    Err(error) => HubStep::Fail(error),
                }
            }
            (AgentMessage::Signed(message), Stage::Syncing(mut syncing)) => {
                match self
                    .take_message(&session.keys, &mut syncing, &message)
                    .await
                {
                    Ok(Ok(answer)) => {
                        session.stage = Stage::Syncing(syncing);
                        answer.map_or(HubStep::Wait, |answer| {
                            HubStep::Reply(hub_envelope_frame(&answer))
                        })
                    }
                    Ok(Err(refused)) => HubStep::Close(close_reason(refused)),
                    Err(error) => HubStep::Fail(error),
                }
            }
            _ => HubStep::Close(PROTOCOL_ERROR),
        }
    }

    /// The hub's command of `kind` holding `payload` to the agent it syncs
    /// with on `session`, signed for this connection with the hub's next
    /// sequence number (§16, decision 176); none before the agent's
    /// `Welcome`, or when the hub can't sign.
    pub async fn command_frame(
        &self,
        session: &mut HubSession,
        kind: WireName,
        payload: Vec<u8>,
    ) -> Option<HubFrame> {
        let _ = (session, kind, payload);
        todo!()
    }
}

/// How the hub closes on a signed message it refuses.
const fn close_reason(refused: MessageRefused) -> &'static str {
    match refused {
        MessageRefused::Unexpected => PROTOCOL_ERROR,
        MessageRefused::Unsigned | MessageRefused::OutOfSequence => SEQUENCE_ERROR,
        MessageRefused::Diverged => OUTBOX_DIVERGED,
    }
}

#[cfg(test)]
mod tests;
