use domain::fleet::{PublicKey, Signature, SignedBytes, SigningSeed};

/// Ed25519 (RFC 8032) for the hub and its agents (§16, decision 120): a key
/// pair from its seed, a signature, a check of one. It signs only
/// [`SignedBytes`], which only the protocol's messages make, so nothing can
/// ask it to sign a bare nonce. Keys and signatures arrive parsed: a
/// small-order key or a malleated signature never reaches [`Signing::verify`].
pub trait Signing: Send + Sync {
    /// The public key of the key pair `seed` makes; the same seed always
    /// makes the same key.
    fn public_key(&self, seed: &SigningSeed) -> Result<PublicKey, SigningError>;

    /// The signature of `message` under the key pair `seed` makes. Ed25519
    /// is deterministic: the same seed and message make the same signature.
    fn sign(&self, seed: &SigningSeed, message: &SignedBytes) -> Result<Signature, SigningError>;

    /// Whether `signature` was made over `message` by the holder of `key`.
    fn verify(&self, key: &PublicKey, message: &SignedBytes, signature: &Signature) -> bool;
}

/// The library refused a seed, or made a key or signature the protocol
/// refuses. Neither happens with a working library; the text is its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningError(pub String);
