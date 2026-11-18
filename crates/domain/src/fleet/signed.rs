//! The bytes every hub and agent signature is made over (§16, decision 120,
//! "The signed messages"): one byte for the label's length, the label, then
//! the message's fields in a fixed order with no separator. Every field but a
//! hostname, a message kind and a refusal code has a fixed width, and those
//! three carry a length byte, so two messages with different labels or
//! fields never encode to the same bytes. Changing a byte of this is a new
//! handshake, which the fixed vectors in the tests stand guard over.

use std::fmt;

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

use super::Hostname;
use super::keys::{KEY_BYTES, PublicKey, lower_hex};
use crate::AgentId;

/// What every label starts with: `v1` names the handshake, as `/sync/v1`
/// does.
pub const LABEL_PREFIX: &str = "ladderback-sync-v1 ";

/// A connection's fresh 32 bytes from one side's `Random` port.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Nonce([u8; KEY_BYTES]);

impl Nonce {
    pub const fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
        todo!()
    }

    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        todo!()
    }
}

impl fmt::Debug for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// What ties a signature to one TLS session (decision 120, "Binding to the
/// TLS session"): nothing behind a proxy (kind 0), or the session's
/// `tls-exporter` value (RFC 9266, kind 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    None,
    TlsExporter([u8; KEY_BYTES]),
}

impl Binding {
    /// The kind byte `Connect` names and the signed bytes carry.
    pub const fn kind(&self) -> u8 {
        todo!()
    }
}

/// The handshakes a hub speaks, a set of 64: bit `n - 1` is handshake `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handshakes(u64);

impl Handshakes {
    pub const fn from_bits(bits: u64) -> Self {
        todo!()
    }

    pub const fn bits(self) -> u64 {
        todo!()
    }
}

/// A time as the signed bytes carry it: whole seconds since the Unix epoch,
/// unsigned, so a time before 1970 has no encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EpochSeconds(u64);

impl EpochSeconds {
    /// The whole seconds of `time`, or `None` before the epoch.
    pub fn from_timestamp(time: Timestamp) -> Option<Self> {
        todo!()
    }

    pub const fn from_seconds(seconds: u64) -> Self {
        todo!()
    }

    pub const fn seconds(self) -> u64 {
        todo!()
    }
}

/// The most bytes a message kind or a refusal code has.
pub const MAX_WIRE_NAME_BYTES: usize = 255;

/// A message kind (`RepoGrants`) or a refusal code (`proof-refused`): 1 to
/// 255 visible ASCII characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WireName(String);

/// Text that isn't a message kind or a refusal code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidWireName;

impl WireName {
    pub fn parse(text: &str) -> Result<Self, InvalidWireName> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        todo!()
    }
}

/// The SHA-256 of a signed message's payload, as its envelope decodes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadDigest([u8; KEY_BYTES]);

impl PayloadDigest {
    pub fn of(payload: &[u8]) -> Self {
        todo!()
    }
}

/// One message of decision 120's table, with the fields its signature
/// covers. [`SignedMessage::to_bytes`] lays them out.
#[derive(Clone, Copy, Debug)]
pub enum SignedMessage<'a> {
    /// The hub's proof in `Challenge`, signed by the hub key.
    HubProof {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
        handshakes: Handshakes,
    },
    /// The agent's proof in `Proof`, signed by the agent key.
    AgentProof {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_key: PublicKey,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
        counter: u64,
    },
    /// `Pair`, signed by the token's pairing key and by the agent key.
    Pair {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_key: PublicKey,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
        counter: u64,
        hostname: &'a Hostname,
    },
    /// `Paired`, signed by the hub key.
    Paired {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_key: PublicKey,
        recovery_key: PublicKey,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
    },
    /// A message from the hub after the handshake.
    HubMessage {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
        sequence: u64,
        expires_at: EpochSeconds,
        kind: &'a WireName,
        payload: PayloadDigest,
    },
    /// A message from the agent after the handshake.
    AgentMessage {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
        sequence: u64,
        kind: &'a WireName,
        payload: PayloadDigest,
    },
    /// A refusal the hub signs once it has proved itself.
    Refusal {
        hub_key: PublicKey,
        agent_id: AgentId,
        agent_nonce: Nonce,
        hub_nonce: Nonce,
        binding: Binding,
        code: &'a WireName,
    },
    /// A rotation, signed by the old hub key.
    Handover {
        old_key: PublicKey,
        new_key: PublicKey,
        made_at: EpochSeconds,
    },
    /// A recovery, signed by the recovery key.
    RecoveryHandover {
        recovery_key: PublicKey,
        new_hub_key: PublicKey,
        recovery_number: u64,
        made_at: EpochSeconds,
    },
    /// A new recovery key, signed by the old one.
    RecoveryRotation {
        old_recovery_key: PublicKey,
        new_recovery_key: PublicKey,
        recovery_number: u64,
        made_at: EpochSeconds,
    },
}

/// The bytes a signature is made and checked over. Only
/// [`SignedMessage::to_bytes`] makes them, so the signing port can't sign
/// anything that isn't one of the protocol's messages: never a bare nonce.
#[derive(Clone, PartialEq, Eq)]
pub struct SignedBytes(Vec<u8>);

impl SignedBytes {
    pub fn as_bytes(&self) -> &[u8] {
        todo!()
    }
}

impl fmt::Debug for SignedBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// The bytes being laid out.
struct Writer(Vec<u8>);

impl Writer {
    fn label(name: &str) -> Self {
        todo!()
    }

    fn key(&mut self, key: &PublicKey) -> &mut Self {
        todo!()
    }

    fn agent(&mut self, id: AgentId) -> &mut Self {
        todo!()
    }

    fn nonce(&mut self, nonce: &Nonce) -> &mut Self {
        todo!()
    }

    fn binding(&mut self, binding: &Binding) -> &mut Self {
        todo!()
    }

    fn number(&mut self, number: u64) -> &mut Self {
        todo!()
    }

    fn digest(&mut self, digest: &PayloadDigest) -> &mut Self {
        todo!()
    }

    /// One length byte, then the bytes. Every caller's type holds at most
    /// 255 bytes; the cap here only keeps a mistake from wrapping the length.
    fn short(&mut self, bytes: &[u8]) -> &mut Self {
        todo!()
    }

    fn done(&mut self) -> SignedBytes {
        todo!()
    }
}

impl SignedMessage<'_> {
    /// The label's last part, after [`LABEL_PREFIX`].
    pub const fn label(&self) -> &'static str {
        todo!()
    }

    /// The bytes the message's signature covers.
    pub fn to_bytes(&self) -> SignedBytes {
        todo!()
    }
}

#[cfg(test)]
mod tests;
