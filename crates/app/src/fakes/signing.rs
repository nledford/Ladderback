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
    *Fingerprint::of_bytes(&parts.concat()).digest()
}

impl FakeSigning {
    fn mac(key: &PublicKey, message: &SignedBytes) -> [u8; SIGNATURE_BYTES] {
        let r = hash(&[b"fake-signature-r", key.as_bytes(), message.as_bytes()]);
        let s = hash(&[b"fake-signature-s", &r]);
        let mut signature = [0; SIGNATURE_BYTES];
        for (to, from) in signature.iter_mut().zip(r.iter().chain(s.iter())) {
            *to = *from;
        }
        // Keeps `S` below the group order, as a real signature's is.
        if let Some(last) = signature.last_mut() {
            *last &= 0x0f;
        }
        signature
    }
}

impl Signing for FakeSigning {
    fn public_key(&self, seed: &SigningSeed) -> Result<PublicKey, SigningError> {
        let mut key = hash(&[b"fake-public-key", seed.expose()]);
        // Keeps `y` below `p` and the sign bit off.
        if let Some(last) = key.last_mut() {
            *last &= 0x3f;
        }
        PublicKey::from_bytes(key).map_err(|refused| SigningError(format!("{refused:?}")))
    }

    fn sign(&self, seed: &SigningSeed, message: &SignedBytes) -> Result<Signature, SigningError> {
        let key = self.public_key(seed)?;
        Signature::from_bytes(Self::mac(&key, message))
            .map_err(|refused| SigningError(format!("{refused:?}")))
    }

    fn verify(&self, key: &PublicKey, message: &SignedBytes, signature: &Signature) -> bool {
        Self::mac(key, message) == *signature.as_bytes()
    }
}
