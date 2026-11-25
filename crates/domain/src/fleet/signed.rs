//! The bytes every hub and agent signature is made over (§16, decision 120,
//! "The signed messages"): one byte for the label's length, the label, then
//! the message's fields in a fixed order with no separator. Every field but a
//! hostname, a machine zone, a message kind and a refusal code has a fixed
//! width, and those four carry a length byte (a zone the agent has no name
//! for is a zero length), so two messages with different labels or
//! fields never encode to the same bytes. Changing a byte of this is a new
//! handshake, which the fixed vectors in the tests stand guard over.

use std::fmt;

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

use super::Hostname;
use super::MachineZone;
use super::keys::{KEY_BYTES, PublicKey, lower_hex};
use crate::{AgentId, RepositoryId};

/// What every label starts with: `v1` names the handshake, as `/sync/v1`
/// does.
pub const LABEL_PREFIX: &str = "ladderback-sync-v1 ";

/// A connection's fresh 32 bytes from one side's `Random` port.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Nonce([u8; KEY_BYTES]);

impl Nonce {
    pub const fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

impl fmt::Debug for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Nonce({})", lower_hex(&self.0))
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
        match self {
            Self::None => 0,
            Self::TlsExporter(_) => 1,
        }
    }
}

/// The handshakes a hub speaks, a set of 64: bit `n - 1` is handshake `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handshakes(u64);

impl Handshakes {
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }
}

/// A time as the signed bytes carry it: whole seconds since the Unix epoch,
/// unsigned, so a time before 1970 has no encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EpochSeconds(u64);

impl EpochSeconds {
    /// The whole seconds of `time`, or `None` before the epoch.
    pub fn from_timestamp(time: Timestamp) -> Option<Self> {
        u64::try_from(time.as_second()).ok().map(Self)
    }

    pub const fn from_seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn seconds(self) -> u64 {
        self.0
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
        let fits = (1..=MAX_WIRE_NAME_BYTES).contains(&text.len());
        if fits && text.bytes().all(|byte| byte.is_ascii_graphic()) {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidWireName)
        }
    }

    /// A kind or code this build names itself, which its tests parse.
    pub(super) fn known(name: &'static str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The SHA-256 of a signed message's payload, as its envelope decodes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadDigest([u8; KEY_BYTES]);

impl PayloadDigest {
    pub fn of(payload: &[u8]) -> Self {
        Self(Sha256::digest(payload).into())
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
        /// None when the agent's zone has no IANA name.
        zone: Option<&'a MachineZone>,
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
    /// One lease the hub granted, signed by the hub key apart from the
    /// message that carried it, so the machine can show it back as proof
    /// of its epoch floor (§16, decision 137, "A report counts only with
    /// its proof"). It names no nonce, so it outlives the connection.
    LeaseEntry {
        hub_key: PublicKey,
        agent_id: AgentId,
        repository: &'a RepositoryId,
        generation: u64,
        epoch: u64,
        expires_at: EpochSeconds,
    },
}

/// The bytes a signature is made and checked over. Only
/// [`SignedMessage::to_bytes`] makes them, so the signing port can't sign
/// anything that isn't one of the protocol's messages: never a bare nonce.
#[derive(Clone, PartialEq, Eq)]
pub struct SignedBytes(Vec<u8>);

impl SignedBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SignedBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SignedBytes({})", lower_hex(&self.0))
    }
}

/// The bytes being laid out.
struct Writer(Vec<u8>);

impl Writer {
    fn label(name: &str) -> Self {
        let mut writer = Self(Vec::with_capacity(512));
        let label = [LABEL_PREFIX, name].concat();
        writer.short(label.as_bytes());
        writer
    }

    fn key(&mut self, key: &PublicKey) -> &mut Self {
        self.0.extend_from_slice(key.as_bytes());
        self
    }

    fn agent(&mut self, id: AgentId) -> &mut Self {
        self.0.extend_from_slice(id.as_uuid().as_bytes());
        self
    }

    fn nonce(&mut self, nonce: &Nonce) -> &mut Self {
        self.0.extend_from_slice(nonce.as_bytes());
        self
    }

    fn binding(&mut self, binding: &Binding) -> &mut Self {
        self.0.push(binding.kind());
        match binding {
            Binding::None => self.0.extend_from_slice(&[0; KEY_BYTES]),
            Binding::TlsExporter(value) => self.0.extend_from_slice(value),
        }
        self
    }

    fn number(&mut self, number: u64) -> &mut Self {
        self.0.extend_from_slice(&number.to_be_bytes());
        self
    }

    fn digest(&mut self, digest: &PayloadDigest) -> &mut Self {
        self.0.extend_from_slice(&digest.0);
        self
    }

    /// One length byte, then the bytes. Every caller's type holds at most
    /// 255 bytes; the cap here only keeps a mistake from wrapping the length.
    fn short(&mut self, bytes: &[u8]) -> &mut Self {
        let bytes = bytes.get(..usize::from(u8::MAX)).unwrap_or(bytes);
        self.0.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
        self.0.extend_from_slice(bytes);
        self
    }

    fn done(&mut self) -> SignedBytes {
        SignedBytes(std::mem::take(&mut self.0))
    }
}

impl SignedMessage<'_> {
    /// The label's last part, after [`LABEL_PREFIX`].
    pub const fn label(&self) -> &'static str {
        match self {
            Self::HubProof { .. } => "hub-proof",
            Self::AgentProof { .. } => "machine-proof",
            Self::Pair { .. } => "pair",
            Self::Paired { .. } => "paired",
            Self::HubMessage { .. } => "hub-signed",
            Self::AgentMessage { .. } => "machine-signed",
            Self::Refusal { .. } => "hub-refused",
            Self::Handover { .. } => "handover",
            Self::RecoveryHandover { .. } => "recovery-handover",
            Self::RecoveryRotation { .. } => "recovery-rotation",
            Self::LeaseEntry { .. } => "lease-entry",
        }
    }

    /// The bytes the message's signature covers.
    pub fn to_bytes(&self) -> SignedBytes {
        let mut w = Writer::label(self.label());
        match self {
            Self::HubProof {
                hub_key,
                agent_id,
                agent_nonce,
                hub_nonce,
                binding,
                handshakes,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding)
                .number(handshakes.bits()),
            Self::AgentProof {
                hub_key,
                agent_id,
                agent_key,
                agent_nonce,
                hub_nonce,
                binding,
                counter,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .key(agent_key)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding)
                .number(*counter),
            Self::Pair {
                hub_key,
                agent_id,
                agent_key,
                agent_nonce,
                hub_nonce,
                binding,
                counter,
                hostname,
                zone,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .key(agent_key)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding)
                .number(*counter)
                .short(hostname.as_str().as_bytes())
                .short(zone.map_or(&[][..], |zone| zone.as_str().as_bytes())),
            Self::Paired {
                hub_key,
                agent_id,
                agent_key,
                recovery_key,
                agent_nonce,
                hub_nonce,
                binding,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .key(agent_key)
                .key(recovery_key)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding),
            Self::HubMessage {
                hub_key,
                agent_id,
                agent_nonce,
                hub_nonce,
                binding,
                sequence,
                expires_at,
                kind,
                payload,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding)
                .number(*sequence)
                .number(expires_at.seconds())
                .short(kind.as_str().as_bytes())
                .digest(payload),
            Self::AgentMessage {
                hub_key,
                agent_id,
                agent_nonce,
                hub_nonce,
                binding,
                sequence,
                kind,
                payload,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding)
                .number(*sequence)
                .short(kind.as_str().as_bytes())
                .digest(payload),
            Self::Refusal {
                hub_key,
                agent_id,
                agent_nonce,
                hub_nonce,
                binding,
                code,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .nonce(agent_nonce)
                .nonce(hub_nonce)
                .binding(binding)
                .short(code.as_str().as_bytes()),
            Self::Handover {
                old_key,
                new_key,
                made_at,
            } => w.key(old_key).key(new_key).number(made_at.seconds()),
            Self::RecoveryHandover {
                recovery_key,
                new_hub_key,
                recovery_number,
                made_at,
            } => w
                .key(recovery_key)
                .key(new_hub_key)
                .number(*recovery_number)
                .number(made_at.seconds()),
            Self::RecoveryRotation {
                old_recovery_key,
                new_recovery_key,
                recovery_number,
                made_at,
            } => w
                .key(old_recovery_key)
                .key(new_recovery_key)
                .number(*recovery_number)
                .number(made_at.seconds()),
            Self::LeaseEntry {
                hub_key,
                agent_id,
                repository,
                generation,
                epoch,
                expires_at,
            } => w
                .key(hub_key)
                .agent(*agent_id)
                .short(repository.as_str().as_bytes())
                .number(*generation)
                .number(*epoch)
                .number(expires_at.seconds()),
        }
        .done()
    }
}

#[cfg(test)]
mod tests;
