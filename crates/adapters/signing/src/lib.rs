//! The `Signing` port over `aws-lc-rs`, the crypto library rustls already
//! links for TLS (§16, decision 120, "Crates"). It is strict on every
//! encoding but doesn't refuse a small-order public key; `domain`'s
//! [`PublicKey`] does, so no such key reaches [`AwsLcSigning::verify`].

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use app::ports::{Signing, SigningError};
use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use domain::fleet::{KEY_BYTES, PublicKey, SIGNATURE_BYTES, Signature, SignedBytes, SigningSeed};

/// Ed25519 through `aws-lc-rs`. The library wipes a key pair's memory when
/// it frees it.
#[derive(Clone, Copy, Debug, Default)]
pub struct AwsLcSigning;

/// The key pair `seed` makes. `from_seed_unchecked` is the one constructor
/// from a bare seed: "unchecked" means no public key is given to compare.
fn key_pair(seed: &SigningSeed) -> Result<Ed25519KeyPair, SigningError> {
    todo!()
}

/// The raw signature of any bytes; the port signs only [`SignedBytes`].
fn sign_bytes(seed: &SigningSeed, message: &[u8]) -> Result<[u8; SIGNATURE_BYTES], SigningError> {
    todo!()
}

/// Whether the raw `signature` of `message` verifies under `key`.
fn verify_bytes(key: &[u8; KEY_BYTES], message: &[u8], signature: &[u8]) -> bool {
    todo!()
}

impl Signing for AwsLcSigning {
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

#[cfg(test)]
mod tests;
