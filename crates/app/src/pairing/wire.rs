//! The handshake's messages between the core's types and their frames on the
//! wire (`api::sync`): a key, a nonce and a signature as the unpadded
//! base64url of their bytes, an agent ID as its UUID's text. A frame is read
//! into the core's types here, once, as it arrives, and a field that doesn't
//! hold what it must refuses the whole frame.

use api::sync::{AgentFrame, HolderFrame, HubFrame};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domain::fleet::{
    BindingKind, Challenge, Connect, EpochSeconds, Handshakes, Holder, Hostname, KEY_BYTES, Nonce,
    Pair, PairAnswer, Paired, PublicKey, Refused, SIGNATURE_BYTES, Signature, WireName,
};
use domain::{AgentId, Uuid};

/// What an agent's frame holds, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentMessage {
    Connect(Connect),
    Pair(Pair),
}

/// What a hub's frame holds, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubMessage {
    Challenge(Challenge),
    Answer(PairAnswer),
}

/// A frame whose field doesn't hold what it must: bytes of another length or
/// encoding, a key the protocol refuses, an agent ID that isn't a UUID, an
/// unknown binding kind, or a hostname or code that isn't one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireRefused;

pub fn connect_frame(connect: &Connect) -> AgentFrame {
        todo!()
    }

pub fn pair_frame(pair: &Pair) -> AgentFrame {
        todo!()
    }

pub fn challenge_frame(challenge: &Challenge) -> HubFrame {
        todo!()
    }

pub fn answer_frame(answer: &PairAnswer) -> HubFrame {
        todo!()
    }

/// The message an agent's frame holds.
pub fn agent_message(frame: &AgentFrame) -> Result<AgentMessage, WireRefused> {
        todo!()
    }

/// The message a hub's frame holds.
pub fn hub_message(frame: &HubFrame) -> Result<HubMessage, WireRefused> {
        todo!()
    }

fn encoded(bytes: &[u8]) -> String {
        todo!()
    }

/// The `N` bytes `text` encodes, strictly: no padding, no other alphabet,
/// no other length, no stray bits. The text is measured before it is
/// decoded, so a long field is refused unread.
fn decoded<const N: usize>(text: &str) -> Result<[u8; N], WireRefused> {
        todo!()
    }

fn key(text: &str) -> Result<PublicKey, WireRefused> {
        todo!()
    }

fn signature(text: &str) -> Result<Signature, WireRefused> {
        todo!()
    }

#[cfg(test)]
mod tests;
