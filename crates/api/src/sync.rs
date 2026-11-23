//! The `/sync/v1` handshake on the wire (§16, decision 120, "The handshake",
//! and technical design §7.1): each message is one WebSocket text frame of
//! JSON, at most [`MAX_FRAME_BYTES`], whose `type` names it. A key, a nonce,
//! a signature and a signed message's payload are the unpadded base64url of
//! their bytes, an agent ID its UUID's text, a time whole seconds since the
//! Unix epoch. A field this build doesn't know is ignored, so a later hub can
//! add one; what each field must hold is checked where the message is read
//! into the core's types (`app::pairing::wire`).

use serde::{Deserialize, Serialize};

use crate::ErrorCode;

mod live;

pub use live::{LastRunPayload, PlanPayload, PlansAckPayload, PlansPayload, ProgressPayload};

/// The handshake's path.
pub const PATH: &str = "/sync/v1";

/// The largest frame either side reads before the proofs (decision 120,
/// "Bounds before the proofs"); a larger one closes the connection.
pub const MAX_FRAME_BYTES: usize = 4096;

/// What the agent sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AgentFrame {
    /// Who the agent says it is, its fresh nonce, and the binding kind it
    /// signs under (0 none, 1 `tls-exporter`).
    Connect {
        agent_id: String,
        agent_nonce: String,
        binding_kind: u8,
    },
    /// The pairing, in place of `Proof`. `zone` is the agent's IANA time
    /// zone name, absent when its zone has no such name.
    Pair {
        pairing_key: String,
        agent_key: String,
        counter: u64,
        hostname: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<String>,
        pairing_signature: String,
        agent_signature: String,
    },
    /// A paired agent's proof: its connection counter and `agent-proof`
    /// signed with its agent key.
    Proof { counter: u64, agent_proof: String },
    /// A signed message after the proofs (decision 120, "Signed messages
    /// after the handshake"): `payload` is the message's JSON bytes.
    Signed {
        kind: String,
        seq: u64,
        payload: String,
        signature: String,
    },
}

/// What the hub sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum HubFrame {
    /// The hub's answer to every `Connect`. `hub_handshakes` is the set of
    /// handshakes it speaks, bit n−1 for handshake n.
    Challenge {
        hub_nonce: String,
        hub_key: String,
        hub_handshakes: u64,
        hub_proof: String,
    },
    /// The pairing was accepted: the hub's recovery key, signed. `hostname`
    /// is the hostname the hub holds the agent under, which may not be the
    /// one the agent asked for, and `status` whether a person has approved
    /// it, which an agent that pairs again may be (#1341). Neither is
    /// signed, as `holder` isn't, and a hub that predates one sends none. A
    /// status this build doesn't know reads as none: the agent only shows
    /// it, and the signed `Welcome` says the status it acts on.
    Paired {
        recovery_key: String,
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hostname: Option<String>,
        #[serde(
            default,
            deserialize_with = "known_approval",
            skip_serializing_if = "Option::is_none"
        )]
        status: Option<AgentApproval>,
    },
    /// The pairing or the proof was refused, with the code signed; `holder`
    /// names the agent that holds a taken hostname, and isn't signed.
    Refused {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder: Option<HolderFrame>,
        signature: String,
    },
    /// A signed message after the proofs, which the agent acts on only
    /// until `expires_at`.
    Signed {
        kind: String,
        seq: u64,
        expires_at: u64,
        payload: String,
        signature: String,
    },
}

/// The payload of the agent's `Hello`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloPayload {
    pub agent_version: String,
    pub hostname: String,
    /// The lowest and the highest protocol the agent speaks.
    pub protocols: [u64; 2],
    /// The agent's clock, in seconds since the Unix epoch.
    pub clock: u64,
    /// The agent's IANA time zone name, absent when its zone has no such
    /// name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// The machine fingerprint of the computer the agent runs on, as 64
    /// lower-case hex digits; absent from a build before fingerprints or
    /// when the computer's identifier can't be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

/// The payload of the hub's `Welcome`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomePayload {
    /// The highest protocol both sides speak; absent when they share none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u64>,
    pub hub_protocols: [u64; 2],
    pub agent_protocols: [u64; 2],
    pub generation: u64,
    pub hub_handshakes: u64,
    pub hub_version: String,
    pub signed_at: u64,
    pub status: AgentApproval,
}

/// The payload of the agent's `OpEvent`: one report from its outbox (§7.2).
/// `epoch` is the outbox's UUID, `seq` the report's place in it, `event`
/// `started`, `succeeded` or `failed`, `at` when it happened, and `code` the
/// error code of a failure, by its wire name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportPayload {
    pub epoch: String,
    pub seq: u64,
    pub op_id: String,
    pub event: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// The payload of the hub's `Ack`: it holds every report of `epoch` through
/// `seq`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckPayload {
    pub epoch: String,
    pub seq: u64,
}

/// The payload of the agent's `ReposSnapshot`: every repository it uses
/// whose ID it knows (§7.2, §7.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReposPayload {
    pub repos: Vec<RepoPayload>,
}

/// One repository in `ReposSnapshot`: its rustic ID as hex, the
/// maintenance it asks for (`local`, `hub` or `none`), and whether it holds
/// another agent's tagged snapshots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoPayload {
    pub repo_id: String,
    pub requested_maintenance: String,
    pub others_snapshots: bool,
}

/// The payload of the hub's `RepoGrants`: every lease the hub counts the
/// agent as holding, at its `generation`, with the hub's clock when it
/// signed them, in seconds since the Unix epoch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantsPayload {
    pub generation: u64,
    pub signed_at: u64,
    pub grants: Vec<GrantPayload>,
}

/// One lease in `RepoGrants`, until `expires_at` on the hub's clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantPayload {
    pub repo_id: String,
    pub epoch: u64,
    pub expires_at: u64,
}

/// The payload of the agent's `ReleaseAck`: it dropped its lease on
/// `repo_id` at `epoch` and runs no maintenance on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasePayload {
    pub repo_id: String,
    pub epoch: u64,
}

/// The payload of the hub's `Released`, its answer to a `ReleaseAck`: the
/// release it took, with the hub's clock when it signed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasedPayload {
    pub repo_id: String,
    pub epoch: u64,
    pub signed_at: u64,
}

/// Whether a person has approved the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentApproval {
    Pending,
    Approved,
}

/// The agent that holds a hostname another asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolderFrame {
    pub hostname: String,
    /// When the hub last heard from it, in seconds since the Unix epoch.
    pub last_seen: u64,
}

/// A frame that isn't one of the messages: not JSON, an unknown `type`, or
/// a field missing or of another type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnreadableFrame;

impl HelloPayload {
    /// The payload `bytes` hold.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, UnreadableFrame> {
        serde_json::from_slice(bytes).map_err(|_| UnreadableFrame)
    }

    /// The payload's bytes, which the agent signs.
    pub fn to_bytes(&self) -> Vec<u8> {
        written(self).into_bytes()
    }
}

impl WelcomePayload {
    /// The payload `bytes` hold.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, UnreadableFrame> {
        serde_json::from_slice(bytes).map_err(|_| UnreadableFrame)
    }

    /// The payload's bytes, which the hub signs.
    pub fn to_bytes(&self) -> Vec<u8> {
        written(self).into_bytes()
    }
}

impl ReportPayload {
    /// The payload `bytes` hold.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, UnreadableFrame> {
        serde_json::from_slice(bytes).map_err(|_| UnreadableFrame)
    }

    /// The payload's bytes, which the agent signs.
    pub fn to_bytes(&self) -> Vec<u8> {
        written(self).into_bytes()
    }
}

impl AckPayload {
    /// The payload `bytes` hold.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, UnreadableFrame> {
        serde_json::from_slice(bytes).map_err(|_| UnreadableFrame)
    }

    /// The payload's bytes, which the hub signs.
    pub fn to_bytes(&self) -> Vec<u8> {
        written(self).into_bytes()
    }
}

/// Each lease payload reads from and writes to its bytes as the others do.
macro_rules! payload_bytes {
    ($($payload:ty),+) => {$(
        impl $payload {
            /// The payload `bytes` hold.
            pub fn from_bytes(bytes: &[u8]) -> Result<Self, UnreadableFrame> {
                serde_json::from_slice(bytes).map_err(|_| UnreadableFrame)
            }

            /// The payload's bytes, which its sender signs.
            pub fn to_bytes(&self) -> Vec<u8> {
                written(self).into_bytes()
            }
        }
    )+};
}

payload_bytes!(ReposPayload, GrantsPayload, ReleasePayload, ReleasedPayload);

impl AgentFrame {
    /// The frame `text` holds.
    pub fn from_text(text: &str) -> Result<Self, UnreadableFrame> {
        serde_json::from_str(text).map_err(|_| UnreadableFrame)
    }

    /// The frame's text.
    pub fn to_text(&self) -> String {
        written(self)
    }
}

impl HubFrame {
    /// The frame `text` holds.
    pub fn from_text(text: &str) -> Result<Self, UnreadableFrame> {
        serde_json::from_str(text).map_err(|_| UnreadableFrame)
    }

    /// The frame's text.
    pub fn to_text(&self) -> String {
        written(self)
    }
}

/// The wire name of `code`, as a report of a failure carries it.
pub fn code_name(code: ErrorCode) -> String {
    match serde_json::to_value(code) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

/// The error code a refusal's code names, as the agent shows it:
/// [`ErrorCode::Unknown`] for one this build doesn't know.
pub fn refusal_code(code: &str) -> ErrorCode {
    let named = serde_json::from_value(serde_json::Value::String(code.to_owned()))
        .unwrap_or(ErrorCode::Unknown);
    // Only the handshake's refusals: a hub can't make an agent show the
    // message of a code that means something else.
    if matches!(
        named,
        ErrorCode::TokenUsed
            | ErrorCode::TokenExpired
            | ErrorCode::TokenRevoked
            | ErrorCode::HostnameTaken
            | ErrorCode::AgentKeyMismatch
            | ErrorCode::HubKeyMismatch
            | ErrorCode::ProofRefused
            | ErrorCode::PendingLimit
            | ErrorCode::BindingUnavailable
            | ErrorCode::IdentityConflict
    ) {
        named
    } else {
        ErrorCode::Unknown
    }
}

/// A frame's or a payload's JSON text.
#[expect(
    clippy::expect_used,
    reason = "the frames and payloads hold only strings, integers, arrays and optional structs of them, which JSON always writes"
)]
fn written(frame: &impl Serialize) -> String {
    serde_json::to_string(frame).expect("a frame is always JSON")
}

/// The approval a field holds, none when it holds a value this build
/// doesn't know, of any type.
fn known_approval<'de, D>(field: D) -> Result<Option<AgentApproval>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Read {
        Known(AgentApproval),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(match Option::<Read>::deserialize(field)? {
        Some(Read::Known(approval)) => Some(approval),
        Some(Read::Unknown(_)) | None => None,
    })
}

#[cfg(test)]
mod tests;
