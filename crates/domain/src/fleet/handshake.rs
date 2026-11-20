//! The handshake's messages as the core takes them (§16, decision 120, "The
//! handshake" and "Pairing"), already parsed: what an agent and its hub send
//! each other before either trusts the other. Their wire form, and the caps
//! on what is read from the network, are the transport's.

use super::hostname::Hostname;
use super::keys::{KEY_BYTES, PublicKey, Signature};
use super::signed::{Binding, EpochSeconds, Handshakes, Nonce, WireName};
use super::token::TokenTls;
use crate::AgentId;

/// The handshakes this build speaks: handshake 1, `/sync/v1`.
pub const HANDSHAKES: Handshakes = Handshakes::from_bits(1);

/// Which binding a connection's signatures carry (decision 120, "Binding to
/// the TLS session"): kind 0 behind a proxy that ends TLS, kind 1 on the
/// hub's own TLS session. The agent names it in `Connect`; it is never
/// negotiated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// Kind 0: no binding.
    None,
    /// Kind 1: the session's `tls-exporter` value.
    TlsExporter,
}

impl BindingKind {
    /// The kind byte `Connect` and the token carry.
    pub const fn byte(self) -> u8 {
        match self {
            Self::None => 0,
            Self::TlsExporter => 1,
        }
    }

    /// The kind a byte names, or `None` for a kind this build doesn't know.
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::None),
            1 => Some(Self::TlsExporter),
            _ => None,
        }
    }

    /// The kind of a token's address: a pinned certificate is the hub's own
    /// TLS (kind 1), a certificate authority's is a proxy's (kind 0).
    pub const fn of_token(tls: TokenTls) -> Self {
        match tls {
            TokenTls::Ca => Self::None,
            TokenTls::Pinned(_) => Self::TlsExporter,
        }
    }

    /// The binding signatures carry under this kind on a session whose
    /// `tls-exporter` value is `exporter`: none for kind 0, whatever the
    /// session; the exporter for kind 1, and `None` when the session has
    /// none to give (`binding-unavailable`).
    pub const fn binding(self, exporter: Option<[u8; KEY_BYTES]>) -> Option<Binding> {
        match (self, exporter) {
            (Self::None, _) => Some(Binding::None),
            (Self::TlsExporter, Some(value)) => Some(Binding::TlsExporter(value)),
            (Self::TlsExporter, None) => None,
        }
    }
}

/// The agent's first message: who it says it is, its fresh nonce, and the
/// binding kind it signs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connect {
    pub agent_id: AgentId,
    pub agent_nonce: Nonce,
    pub binding_kind: BindingKind,
}

/// The hub's answer to every `Connect`: its fresh nonce, its key, the
/// handshakes it speaks and its proof over them (`hub-proof`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Challenge {
    pub hub_nonce: Nonce,
    pub hub_key: PublicKey,
    pub hub_handshakes: Handshakes,
    pub hub_proof: Signature,
}

/// The agent's pairing, in place of `Proof`: the token's pairing key, which
/// lets the hub find the token, the key the agent registers, its counter
/// and hostname, and the `pair` message signed by both keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub pairing_key: PublicKey,
    pub agent_key: PublicKey,
    pub counter: u64,
    pub hostname: Hostname,
    pub pairing_signature: Signature,
    pub agent_signature: Signature,
}

/// The agent's proof, once the hub's `Challenge` has proved the hub: its
/// connection counter and the `agent-proof` message signed by its agent key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proof {
    pub counter: u64,
    pub agent_proof: Signature,
}

/// The hub's acceptance of a pairing: its recovery key, signed with the hub
/// key over the connection (`paired`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paired {
    pub recovery_key: PublicKey,
    pub signature: Signature,
}

/// Which agent holds a hostname another asked for, as the hub shows it so
/// the person can find it (decision 121, `hostname-taken`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    /// The hostname as the holder registered it.
    pub hostname: Hostname,
    /// When the hub last heard from the holder.
    pub last_seen: EpochSeconds,
}

/// A refusal from a hub that has proved itself, signed over its code and
/// the connection (`hub-refused`). `holder` names the agent holding a taken
/// hostname; only the code is signed, so the agent shows the holder and
/// acts on nothing in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub code: WireName,
    pub holder: Option<Holder>,
    pub signature: Signature,
}

/// What the hub answers a `Pair` with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairAnswer {
    Paired(Paired),
    Refused(Refused),
}

#[cfg(test)]
mod tests;
