//! The `Signing` port's contract (§16, decision 120): what the hub and its
//! agents rely on whichever implementation signs. Each case is run by
//! [`run`]; the real adapter's own tests add RFC 8032's vectors.

use domain::fleet::{
    Binding, EpochSeconds, Handshakes, Nonce, PublicKey, SIGNATURE_BYTES, Signature, SignedBytes,
    SignedMessage, SigningSeed,
};
use domain::{AgentId, Uuid};

use crate::ports::Signing;

pub fn run(signing: &dyn Signing) {
    a_seed_makes_one_key(signing);
    a_signature_verifies_under_its_key(signing);
    a_signature_is_deterministic(signing);
    another_message_is_refused(signing);
    another_key_is_refused(signing);
    a_changed_signature_is_refused(signing);
    a_proof_never_verifies_as_another_message(signing);
}

fn seed(byte: u8) -> SigningSeed {
    SigningSeed::from_bytes([byte; 32])
}

/// The key `seed(byte)` makes; `None`, after a failed assertion, if none.
fn key_of(signing: &dyn Signing, byte: u8) -> Option<PublicKey> {
    let key = signing.public_key(&seed(byte));
    assert!(key.is_ok(), "seed {byte:#04x} made no key: {key:?}");
    key.ok()
}

fn sign(signing: &dyn Signing, byte: u8, message: &SignedBytes) -> Option<Signature> {
    let signature = signing.sign(&seed(byte), message);
    assert!(
        signature.is_ok(),
        "seed {byte:#04x} signed nothing: {signature:?}"
    );
    signature.ok()
}

/// A fixed public key, as a peer's arrives.
fn peer(byte: u8) -> Option<PublicKey> {
    PublicKey::from_bytes([byte; 32]).ok()
}

fn handover(made_at: u64) -> Option<SignedBytes> {
    Some(
        SignedMessage::Handover {
            old_key: peer(0x11)?,
            new_key: peer(0x44)?,
            made_at: EpochSeconds::from_seconds(made_at),
        }
        .to_bytes(),
    )
}

/// The same seed makes the same key, and two seeds two keys.
fn a_seed_makes_one_key(signing: &dyn Signing) {
    assert_eq!(key_of(signing, 1), key_of(signing, 1));
    assert_ne!(key_of(signing, 1), key_of(signing, 2));
}

fn a_signature_verifies_under_its_key(signing: &dyn Signing) {
    let Some(message) = handover(1_800_000_000) else {
        return;
    };
    let (Some(key), Some(signature)) = (key_of(signing, 1), sign(signing, 1, &message)) else {
        return;
    };
    assert!(signing.verify(&key, &message, &signature));
}

/// Ed25519 signs deterministically (RFC 8032, §5.1.6).
fn a_signature_is_deterministic(signing: &dyn Signing) {
    let (Some(message), Some(later)) = (handover(1_800_000_000), handover(1_800_000_001)) else {
        return;
    };
    assert_eq!(sign(signing, 1, &message), sign(signing, 1, &message));
    assert_ne!(sign(signing, 1, &message), sign(signing, 1, &later));
}

fn another_message_is_refused(signing: &dyn Signing) {
    let (Some(message), Some(later)) = (handover(1_800_000_000), handover(1_800_000_001)) else {
        return;
    };
    let (Some(key), Some(signature)) = (key_of(signing, 1), sign(signing, 1, &message)) else {
        return;
    };
    assert!(!signing.verify(&key, &later, &signature));
}

fn another_key_is_refused(signing: &dyn Signing) {
    let Some(message) = handover(1_800_000_000) else {
        return;
    };
    let (Some(other), Some(signature)) = (key_of(signing, 2), sign(signing, 1, &message)) else {
        return;
    };
    assert!(!signing.verify(&other, &message, &signature));
}

/// A bit flipped anywhere in `R`, or low in `S` where it stays canonical,
/// makes a signature that doesn't verify.
fn a_changed_signature_is_refused(signing: &dyn Signing) {
    let Some(message) = handover(1_800_000_000) else {
        return;
    };
    let (Some(key), Some(signature)) = (key_of(signing, 1), sign(signing, 1, &message)) else {
        return;
    };
    for at in [0, 17, 31, 32, 48] {
        let mut bytes: [u8; SIGNATURE_BYTES] = *signature.as_bytes();
        if let Some(byte) = bytes.get_mut(at) {
            *byte ^= 0x01;
        }
        let changed = Signature::from_bytes(bytes);
        assert!(
            changed.is_ok(),
            "a bit flipped at byte {at} made S too large"
        );
        if let Ok(changed) = changed {
            assert!(
                !signing.verify(&key, &message, &changed),
                "a bit flipped at byte {at} still verified"
            );
        }
    }
}

/// A hub proof signed by a key doesn't verify as an agent proof with the
/// same fields, nor as a proof for another connection.
fn a_proof_never_verifies_as_another_message(signing: &dyn Signing) {
    let Some(hub_key) = key_of(signing, 1) else {
        return;
    };
    let agent_id = AgentId::from_uuid(Uuid::from_u128(7));
    let proof = |hub_nonce: u8| {
        SignedMessage::HubProof {
            hub_key,
            agent_id,
            agent_nonce: Nonce::from_bytes([0xaa; 32]),
            hub_nonce: Nonce::from_bytes([hub_nonce; 32]),
            binding: Binding::None,
            handshakes: Handshakes::from_bits(1),
        }
        .to_bytes()
    };
    let as_agent_proof = SignedMessage::AgentProof {
        hub_key,
        agent_id,
        agent_key: hub_key,
        agent_nonce: Nonce::from_bytes([0xaa; 32]),
        hub_nonce: Nonce::from_bytes([0xbb; 32]),
        binding: Binding::None,
        counter: 1,
    }
    .to_bytes();
    let Some(signature) = sign(signing, 1, &proof(0xbb)) else {
        return;
    };
    assert!(signing.verify(&hub_key, &proof(0xbb), &signature));
    assert!(!signing.verify(&hub_key, &as_agent_proof, &signature));
    assert!(!signing.verify(&hub_key, &proof(0xbc), &signature));
}
