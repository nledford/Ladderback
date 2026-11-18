use domain::fleet::{
    Fingerprint, KEY_BYTES, PublicKey, SIGNATURE_BYTES, Signature, SignedBytes, SigningSeed,
};

use crate::ports::{Signing, SigningError};

/// A stand-in for Ed25519 that keeps its shape: a key and a signature are
/// SHA-256 hashes of the seed, and of the key and the message, so they are
/// deterministic, and a signature holds for its own key and message only.
/// Anyone can forge one; it is for use-case tests, never for security.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeSigning;

/// The SHA-256 of the parts, one after the other.
fn hash(parts: &[&[u8]]) -> [u8; KEY_BYTES] {
    todo!()
}

impl FakeSigning {
    fn mac(key: &PublicKey, message: &SignedBytes) -> [u8; SIGNATURE_BYTES] {
        todo!()
    }
}

impl Signing for FakeSigning {
    fn public_key(&self, seed: &SigningSeed) -> Result<PublicKey, SigningError> {
        todo!()
    }

    fn sign(&self, seed: &SigningSeed, message: &SignedBytes) -> Result<Signature, SigningError> {
        todo!()
    }

    fn verify(&self, key: &PublicKey, message: &SignedBytes, signature: &Signature) -> bool {
        todo!()
    }
}
