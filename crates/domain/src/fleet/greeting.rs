//! The first signed messages after the handshake (technical design §7.2;
//! §16, decision 120, "Signed messages after the handshake" and "Versions
//! and compatibility"): the agent's `Hello`, the hub's `Welcome`, and the
//! envelopes that carry them.

use super::handshake::HANDSHAKES;
use super::hostname::Hostname;
use super::keys::Signature;
use super::registry::AgentStatus;
use super::signed::{EpochSeconds, Handshakes, WireName};

/// The sync protocols one side speaks: every one from `lowest` to
/// `highest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Protocols {
    lowest: u64,
    highest: u64,
}

impl Protocols {
    /// The protocols `lowest` to `highest`, or `None` when that holds none:
    /// protocols count from 1.
    pub const fn new(lowest: u64, highest: u64) -> Option<Self> {
        let _ = (lowest, highest);
        todo!()
    }

    pub const fn lowest(self) -> u64 {
        self.lowest
    }

    pub const fn highest(self) -> u64 {
        self.highest
    }

    /// The highest protocol both sides speak, or `None` when they share
    /// none.
    pub fn highest_shared(self, other: Self) -> Option<u64> {
        let _ = other;
        todo!("highest_shared")
    }
}

/// The protocols this build speaks.
pub const PROTOCOLS: Protocols = Protocols {
    lowest: 1,
    highest: 1,
};

/// The hub's generation, which only a move of the hub raises (#1039).
pub const GENERATION: u64 = 1;

/// The sequence number of each side's first signed message on a
/// connection.
pub const FIRST_SEQUENCE: u64 = 1;

/// How long after signing it the hub lets a message be acted on, in
/// seconds.
pub const MESSAGE_LIFE: u64 = 600;

/// How far past the hub's signing time an agent accepts an expiry, in
/// seconds: further is a message no hub of this protocol sends.
pub const MAX_EXPIRY_AHEAD: u64 = 900;

/// The kind of the agent's first signed message.
pub fn hello_kind() -> WireName {
    todo!("hello_kind")
}

/// The kind of the hub's answer to `Hello`.
pub fn welcome_kind() -> WireName {
    todo!("welcome_kind")
}

/// A signed message from an agent: its kind, sequence number and payload
/// bytes as they came, so the signature is checked over what was signed
/// before the payload is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEnvelope {
    pub kind: WireName,
    pub seq: u64,
    pub payload: Vec<u8>,
    pub signature: Signature,
}

/// A signed message from the hub, which also carries when it stops being
/// acted on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubEnvelope {
    pub kind: WireName,
    pub seq: u64,
    pub expires_at: EpochSeconds,
    pub payload: Vec<u8>,
    pub signature: Signature,
}

/// Whether a person has approved the agent, as `Welcome` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approval {
    /// The hub holds the agent pending and acts on nothing it says.
    Pending,
    Approved,
}

impl Approval {
    pub const fn of(status: AgentStatus) -> Self {
        match status {
            AgentStatus::Pending { .. } => Self::Pending,
            AgentStatus::Approved { .. } => Self::Approved,
        }
    }
}

/// The agent's first signed message on a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    /// The agent's build, as `--version` names it.
    pub agent_version: String,
    pub hostname: Hostname,
    pub protocols: Protocols,
    /// The agent's clock when it sent `Hello`.
    pub clock: EpochSeconds,
}

/// The hub's answer to `Hello`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    /// The highest protocol both sides speak, or `None` when they share
    /// none.
    pub protocol: Option<u64>,
    pub hub_protocols: Protocols,
    /// The protocols the agent's `Hello` named, echoed so a stripped range
    /// shows.
    pub agent_protocols: Protocols,
    pub generation: u64,
    pub hub_handshakes: Handshakes,
    pub hub_version: String,
    pub signed_at: EpochSeconds,
    pub approval: Approval,
}

/// Why an agent acts on no `Welcome`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WelcomeRefused {
    /// It expires at or before its signing time, or further past it than
    /// [`MAX_EXPIRY_AHEAD`].
    Expiry,
    /// The protocols it echoes aren't the ones the agent sent, or the
    /// protocol it picks isn't the highest both speak.
    Protocols,
    /// Its generation is below the one the agent has pinned: a hub the
    /// agent has since followed elsewhere.
    GenerationLower,
    /// The hub and the agent share no protocol.
    NoCommonProtocol,
}

impl Welcome {
    /// The hub's answer at `signed_at` to `hello` from an agent whose
    /// approval is `approval`.
    pub fn answering(
        hello: &Hello,
        approval: Approval,
        hub_version: String,
        signed_at: EpochSeconds,
    ) -> Self {
        let _ = (hello, approval, hub_version, signed_at, HANDSHAKES);
        todo!("answering")
    }

    /// When the hub's message carrying this `Welcome` expires.
    pub const fn expires_at(&self) -> EpochSeconds {
        EpochSeconds::from_seconds(self.signed_at.seconds().saturating_add(MESSAGE_LIFE))
    }

    /// Whether an agent that sent `sent` in its `Hello` and pinned
    /// `generation` acts on this `Welcome`, whose envelope expires at
    /// `expires_at`.
    pub fn check(
        &self,
        expires_at: EpochSeconds,
        sent: Protocols,
        generation: u64,
    ) -> Result<(), WelcomeRefused> {
        let _ = (expires_at, sent, generation);
        todo!("check")
    }
}

#[cfg(test)]
mod tests;
