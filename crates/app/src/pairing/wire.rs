//! The handshake's messages between the core's types and their frames on the
//! wire (`api::sync`): a key, a nonce and a signature as the unpadded
//! base64url of their bytes, an agent ID as its UUID's text. A frame is read
//! into the core's types here, once, as it arrives, and a field that doesn't
//! hold what it must refuses the whole frame.

use api::sync::{
    AgentApproval, AgentFrame, HelloPayload, HolderFrame, HubFrame, MAX_FRAME_BYTES, WelcomePayload,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domain::fleet::{
    AgentEnvelope, BindingKind, Challenge, Connect, EpochSeconds, Handshakes, Hello, Holder,
    Hostname, HubEnvelope, KEY_BYTES, Nonce, Pair, PairAnswer, Paired, Proof, PublicKey, Refused,
    SIGNATURE_BYTES, Signature, Welcome, WireName,
};
use domain::{AgentId, Uuid};

/// What an agent's frame holds, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentMessage {
    Connect(Connect),
    Pair(Pair),
    Proof(Proof),
    Signed(AgentEnvelope),
}

/// What a hub's frame holds, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubMessage {
    Challenge(Challenge),
    Answer(PairAnswer),
    Signed(HubEnvelope),
}

/// A frame whose field doesn't hold what it must: bytes of another length or
/// encoding, a key the protocol refuses, an agent ID that isn't a UUID, an
/// unknown binding kind, or a hostname or code that isn't one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireRefused;

pub fn connect_frame(connect: &Connect) -> AgentFrame {
    AgentFrame::Connect {
        agent_id: connect.agent_id.as_uuid().hyphenated().to_string(),
        agent_nonce: encoded(connect.agent_nonce.as_bytes()),
        binding_kind: connect.binding_kind.byte(),
    }
}

pub fn pair_frame(pair: &Pair) -> AgentFrame {
    AgentFrame::Pair {
        pairing_key: encoded(pair.pairing_key.as_bytes()),
        agent_key: encoded(pair.agent_key.as_bytes()),
        counter: pair.counter,
        hostname: pair.hostname.as_str().to_owned(),
        pairing_signature: encoded(pair.pairing_signature.as_bytes()),
        agent_signature: encoded(pair.agent_signature.as_bytes()),
    }
}

pub fn challenge_frame(challenge: &Challenge) -> HubFrame {
    HubFrame::Challenge {
        hub_nonce: encoded(challenge.hub_nonce.as_bytes()),
        hub_key: encoded(challenge.hub_key.as_bytes()),
        hub_handshakes: challenge.hub_handshakes.bits(),
        hub_proof: encoded(challenge.hub_proof.as_bytes()),
    }
}

pub fn answer_frame(answer: &PairAnswer) -> HubFrame {
    match answer {
        PairAnswer::Paired(paired) => HubFrame::Paired {
            recovery_key: encoded(paired.recovery_key.as_bytes()),
            signature: encoded(paired.signature.as_bytes()),
        },
        PairAnswer::Refused(refused) => HubFrame::Refused {
            code: refused.code.as_str().to_owned(),
            holder: refused.holder.as_ref().map(|holder| HolderFrame {
                hostname: holder.hostname.as_str().to_owned(),
                last_seen: holder.last_seen.seconds(),
            }),
            signature: encoded(refused.signature.as_bytes()),
        },
    }
}

pub fn proof_frame(proof: &Proof) -> AgentFrame {
    let _ = proof;
    todo!("proof_frame")
}

pub fn agent_envelope_frame(envelope: &AgentEnvelope) -> AgentFrame {
    let _ = envelope;
    todo!("agent_envelope_frame")
}

pub fn hub_envelope_frame(envelope: &HubEnvelope) -> HubFrame {
    let _ = envelope;
    todo!("hub_envelope_frame")
}

/// The bytes of `hello`'s payload, which the agent signs.
pub fn hello_payload(hello: &Hello) -> Vec<u8> {
    let _ = (hello, HelloPayload::from_bytes);
    todo!("hello_payload")
}

/// The `Hello` a payload's bytes hold.
pub fn read_hello(bytes: &[u8]) -> Result<Hello, WireRefused> {
    let _ = bytes;
    todo!("read_hello")
}

/// The bytes of `welcome`'s payload, which the hub signs.
pub fn welcome_payload(welcome: &Welcome) -> Vec<u8> {
    let _ = (welcome, WelcomePayload::from_bytes, AgentApproval::Pending);
    todo!("welcome_payload")
}

/// The `Welcome` a payload's bytes hold.
pub fn read_welcome(bytes: &[u8]) -> Result<Welcome, WireRefused> {
    let _ = bytes;
    todo!("read_welcome")
}

/// The message an agent's frame holds.
pub fn agent_message(frame: &AgentFrame) -> Result<AgentMessage, WireRefused> {
    match frame {
        AgentFrame::Connect {
            agent_id,
            agent_nonce,
            binding_kind,
        } => Ok(AgentMessage::Connect(Connect {
            agent_id: AgentId::from_uuid(Uuid::try_parse(agent_id).map_err(|_| WireRefused)?),
            agent_nonce: Nonce::from_bytes(decoded(agent_nonce)?),
            binding_kind: BindingKind::from_byte(*binding_kind).ok_or(WireRefused)?,
        })),
        AgentFrame::Pair {
            pairing_key,
            agent_key,
            counter,
            hostname,
            pairing_signature,
            agent_signature,
        } => Ok(AgentMessage::Pair(Pair {
            pairing_key: key(pairing_key)?,
            agent_key: key(agent_key)?,
            counter: *counter,
            hostname: Hostname::parse(hostname).map_err(|_| WireRefused)?,
            pairing_signature: signature(pairing_signature)?,
            agent_signature: signature(agent_signature)?,
        })),
        AgentFrame::Proof { .. } | AgentFrame::Signed { .. } => todo!("agent_message"),
    }
}

/// The message a hub's frame holds.
pub fn hub_message(frame: &HubFrame) -> Result<HubMessage, WireRefused> {
    match frame {
        HubFrame::Challenge {
            hub_nonce,
            hub_key,
            hub_handshakes,
            hub_proof,
        } => Ok(HubMessage::Challenge(Challenge {
            hub_nonce: Nonce::from_bytes(decoded(hub_nonce)?),
            hub_key: key(hub_key)?,
            hub_handshakes: Handshakes::from_bits(*hub_handshakes),
            hub_proof: signature(hub_proof)?,
        })),
        HubFrame::Paired {
            recovery_key,
            signature: signed,
        } => Ok(HubMessage::Answer(PairAnswer::Paired(Paired {
            recovery_key: key(recovery_key)?,
            signature: signature(signed)?,
        }))),
        HubFrame::Refused {
            code,
            holder,
            signature: signed,
        } => Ok(HubMessage::Answer(PairAnswer::Refused(Refused {
            code: WireName::parse(code).map_err(|_| WireRefused)?,
            holder: match holder {
                Some(holder) => Some(Holder {
                    hostname: Hostname::parse(&holder.hostname).map_err(|_| WireRefused)?,
                    last_seen: EpochSeconds::from_seconds(holder.last_seen),
                }),
                None => None,
            },
            signature: signature(signed)?,
        }))),
        HubFrame::Signed { .. } => {
            let _ = MAX_FRAME_BYTES;
            todo!("hub_message")
        }
    }
}

fn encoded(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The `N` bytes `text` encodes, strictly: no padding, no other alphabet,
/// no other length, no stray bits. The text is measured before it is
/// decoded, so a long field is refused unread.
fn decoded<const N: usize>(text: &str) -> Result<[u8; N], WireRefused> {
    if text.len() != (N * 4).div_ceil(3) {
        return Err(WireRefused);
    }
    // Room for the largest field and the library's estimate beyond it.
    let mut buffer = [0; SIGNATURE_BYTES + 3];
    match URL_SAFE_NO_PAD.decode_slice(text, &mut buffer) {
        Ok(length) if length == N => buffer
            .get(..N)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(WireRefused),
        _ => Err(WireRefused),
    }
}

fn key(text: &str) -> Result<PublicKey, WireRefused> {
    PublicKey::from_bytes(decoded::<KEY_BYTES>(text)?).map_err(|_| WireRefused)
}

fn signature(text: &str) -> Result<Signature, WireRefused> {
    Signature::from_bytes(decoded::<SIGNATURE_BYTES>(text)?).map_err(|_| WireRefused)
}

#[cfg(test)]
mod tests;
