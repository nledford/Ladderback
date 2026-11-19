//! The `/sync/v1` handshake on the wire (§16, decision 120, "The handshake",
//! and technical design §7.1): each message is one WebSocket text frame of
//! JSON, at most [`MAX_FRAME_BYTES`], whose `type` names it. A key, a nonce
//! and a signature are the unpadded base64url of their bytes, an agent ID
//! its UUID's text, a time whole seconds since the Unix epoch. A field this
//! build doesn't know is ignored, so a later hub can add one; what each
//! field must hold is checked where the message is read into the core's
//! types (`app::pairing::wire`).

use serde::{Deserialize, Serialize};

use crate::ErrorCode;

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
    /// The pairing, in place of `Proof`.
    Pair {
        pairing_key: String,
        agent_key: String,
        counter: u64,
        hostname: String,
        pairing_signature: String,
        agent_signature: String,
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
    /// The pairing was accepted: the hub's recovery key, signed.
    Paired {
        recovery_key: String,
        signature: String,
    },
    /// The pairing was refused, with the code signed; `holder` names the
    /// agent that holds a taken hostname, and isn't signed.
    Refused {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder: Option<HolderFrame>,
        signature: String,
    },
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

impl AgentFrame {
    /// The frame `text` holds.
    pub fn from_text(text: &str) -> Result<Self, UnreadableFrame> {
        todo!()
    }

    /// The frame's text.
    pub fn to_text(&self) -> String {
        todo!()
    }
}

impl HubFrame {
    /// The frame `text` holds.
    pub fn from_text(text: &str) -> Result<Self, UnreadableFrame> {
        todo!()
    }

    /// The frame's text.
    pub fn to_text(&self) -> String {
        todo!()
    }
}

/// The error code a refusal's code names, as the agent shows it:
/// [`ErrorCode::Unknown`] for one this build doesn't know.
pub fn refusal_code(code: &str) -> ErrorCode {
        todo!()
    }

/// A frame's JSON text.
#[expect(
    clippy::expect_used,
    reason = "the frames hold only strings, integers and optional structs of them, which JSON always writes"
)]
fn written(frame: &impl Serialize) -> String {
        todo!()
    }

#[cfg(test)]
mod tests;
