use std::sync::Arc;

use domain::fleet::{
    Ack, AgentEnvelope, Binding, EpochSeconds, FIRST_SEQUENCE, HubEnvelope, Nonce, PayloadDigest,
    PublicKey, SignedMessage, SigningSeed, WireName, ack_kind, fresh_on_hub_time, report_kind,
};
use domain::{AgentId, SignedDuration};

use super::wire::{HubMessage, agent_envelope_frame, hub_message, read_ack, report_payload};
use crate::ports::{Clock, HubLink, LinkError, OutboxEntry, Signing, SigningError};

/// Why a connection after the handshake went on no further. None carries a
/// seed (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelError {
    /// The connection ended or didn't carry the message.
    Link(LinkError),
    /// A message the hub didn't sign on this connection, or signed out of
    /// sequence: nothing in it is acted on.
    Unsigned,
    /// A message whose expiry has passed on the hub's time, or lies further
    /// ahead than a hub signs.
    Expired,
    /// A message of a kind the agent doesn't take there, or whose payload
    /// isn't the kind's.
    OutOfTurn,
    /// The agent's count of its own messages ran out.
    SequenceSpent,
    Signing(SigningError),
}

impl From<LinkError> for ChannelError {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

impl From<SigningError> for ChannelError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

/// The connection to the hub after its `Welcome` (decision 120, "Signed
/// messages after the handshake"): each message the agent sends is signed
/// for this agent and connection with the next sequence number, and each
/// the hub sends is acted on only once its signature, its sequence number
/// and its expiry, on the hub's time, hold.
pub struct AgentChannel {
    link: Box<dyn HubLink>,
    signing: Arc<dyn Signing>,
    clock: Arc<dyn Clock>,
    seed: SigningSeed,
    bound: Bound,
    /// The sequence number of the agent's next message.
    next_sent: u64,
    /// The sequence number the hub's next message must carry.
    next_received: u64,
    /// When the hub signed its `Welcome`, on its clock.
    welcomed_at: EpochSeconds,
    /// When the agent sent `Hello`, on its monotonic clock.
    hello_sent: SignedDuration,
}

/// What every signed message on one connection is bound to.
#[derive(Clone, Copy)]
pub(super) struct Bound {
    pub(super) hub_key: PublicKey,
    pub(super) agent_id: AgentId,
    pub(super) agent_nonce: Nonce,
    pub(super) hub_nonce: Nonce,
    pub(super) binding: Binding,
}

impl std::fmt::Debug for AgentChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentChannel")
            .field("next_sent", &self.next_sent)
            .field("next_received", &self.next_received)
            .finish_non_exhaustive()
    }
}

impl AgentChannel {
    /// The connection whose `Hello`, sent at `hello_sent`, the hub answered
    /// with a `Welcome` signed at `welcomed_at`; both carried sequence
    /// number 1.
    pub(super) fn after_welcome(
        link: Box<dyn HubLink>,
        signing: Arc<dyn Signing>,
        clock: Arc<dyn Clock>,
        seed: SigningSeed,
        bound: Bound,
        welcomed_at: EpochSeconds,
        hello_sent: SignedDuration,
    ) -> Self {
        let next = FIRST_SEQUENCE.saturating_add(1);
        Self {
            link,
            signing,
            clock,
            seed,
            bound,
            next_sent: next,
            next_received: next,
            welcomed_at,
            hello_sent,
        }
    }

    /// Sends the report `entry`, signed with the next sequence number.
    pub async fn send_report(&mut self, entry: &OutboxEntry) -> Result<(), ChannelError> {
        self.send(report_kind(), report_payload(entry)).await
    }

    /// The hub's next acknowledgement, once it holds. A signed message of
    /// a kind this build doesn't take is counted and passed over.
    pub async fn receive_ack(&mut self) -> Result<Ack, ChannelError> {
        loop {
            let envelope = self.receive().await?;
            if envelope.kind == ack_kind() {
                return read_ack(&envelope.payload).map_err(|_| ChannelError::OutOfTurn);
            }
        }
    }

    /// Sends a message of `kind` holding `payload`, signed with the next
    /// sequence number.
    async fn send(&mut self, kind: WireName, payload: Vec<u8>) -> Result<(), ChannelError> {
        let sequence = self.next_sent;
        let next = sequence.checked_add(1).ok_or(ChannelError::SequenceSpent)?;
        let bound = self.bound;
        let said = SignedMessage::AgentMessage {
            hub_key: bound.hub_key,
            agent_id: bound.agent_id,
            agent_nonce: bound.agent_nonce,
            hub_nonce: bound.hub_nonce,
            binding: bound.binding,
            sequence,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        let signature = self.signing.sign(&self.seed, &said.to_bytes())?;
        self.next_sent = next;
        self.link
            .send(&agent_envelope_frame(&AgentEnvelope {
                kind,
                seq: sequence,
                payload,
                signature,
            }))
            .await?;
        Ok(())
    }

    /// The hub's next signed message, once it holds.
    async fn receive(&mut self) -> Result<HubEnvelope, ChannelError> {
        let frame = self.link.receive().await?;
        let since = self.clock.monotonic().saturating_sub(self.hello_sent);
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return Err(ChannelError::OutOfTurn);
        };
        let bound = self.bound;
        let signed = SignedMessage::HubMessage {
            hub_key: bound.hub_key,
            agent_id: bound.agent_id,
            agent_nonce: bound.agent_nonce,
            hub_nonce: bound.hub_nonce,
            binding: bound.binding,
            sequence: envelope.seq,
            expires_at: envelope.expires_at,
            kind: &envelope.kind,
            payload: PayloadDigest::of(&envelope.payload),
        };
        if envelope.seq != self.next_received
            || !self
                .signing
                .verify(&bound.hub_key, &signed.to_bytes(), &envelope.signature)
        {
            return Err(ChannelError::Unsigned);
        }
        self.next_received = self.next_received.saturating_add(1);
        if !fresh_on_hub_time(self.welcomed_at, since, envelope.expires_at) {
            return Err(ChannelError::Expired);
        }
        Ok(envelope)
    }
}
