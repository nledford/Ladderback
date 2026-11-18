//! The pairing token (§16, decision 120, "Pairing"): `ladderback_pair_` and
//! the unpadded base64url of a version byte (1), the hub key (32 bytes), a
//! TLS byte (0 CA, 1 pinned), the certificate's fingerprint (32, zeros for
//! CA), the seed of a one-time pairing key (32), the hub address (a length
//! byte, then at most 200 bytes of UTF-8) and the first 4 bytes of the
//! SHA-256 of all before them. The checksum only catches a mistyped token;
//! it is no security. The token is a secret until it is used (I4).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use super::fingerprint::Fingerprint;
use super::keys::{KEY_BYTES, PublicKey, SigningSeed};
use crate::secrets::SecretValue;

/// What every token starts with, so a secret scanner can find a leaked one.
pub const TOKEN_PREFIX: &str = "ladderback_pair_";

/// The token layout this build reads and writes.
pub const TOKEN_VERSION: u8 = 1;

/// The most bytes a token's hub address has.
pub const MAX_ADDRESS_BYTES: usize = 200;

/// The bytes around the address: version, hub key, TLS byte, fingerprint,
/// seed, address length and checksum.
const FIXED_BYTES: usize = 1 + KEY_BYTES + 1 + KEY_BYTES + KEY_BYTES + 1 + CHECKSUM_BYTES;

const CHECKSUM_BYTES: usize = 4;

/// The longest token text: the prefix and the base64 of the most bytes. A
/// longer text is refused before it is decoded.
const MAX_TOKEN_TEXT: usize =
    TOKEN_PREFIX.len() + (FIXED_BYTES + MAX_ADDRESS_BYTES).div_ceil(3) * 4;

/// How the agent checks the hub's TLS certificate at the token's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenTls {
    /// Behind a certificate authority's certificate, as a proxy's is.
    Ca,
    /// The hub's own certificate, pinned by its public key's fingerprint.
    Pinned(Fingerprint),
}

/// The address the agent first connects to: `https://` or `http://` and a
/// host, at most 200 bytes, with no space or control character. Whether a
/// plain `http://` one may be used is the connection's rule (loopback only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubAddress(String);

/// Text that isn't a hub address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidHubAddress;

impl HubAddress {
    pub fn parse(text: &str) -> Result<Self, InvalidHubAddress> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        todo!()
    }
}

/// Why text isn't a pairing token this build can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenRefused {
    /// Not a whole token: the prefix, the encoding, the length, the checksum
    /// or a field is wrong (`token-malformed`).
    Malformed,
    /// A whole token of a later layout (`token-unsupported`).
    Unsupported,
}

/// A pairing token: what the hub shows once to pair one agent.
#[derive(Clone, Debug)]
pub struct PairingToken {
    hub_key: PublicKey,
    tls: TokenTls,
    pairing_seed: SigningSeed,
    address: HubAddress,
}

impl PairingToken {
    pub fn new(
        hub_key: PublicKey,
        tls: TokenTls,
        pairing_seed: SigningSeed,
        address: HubAddress,
    ) -> Self {
        todo!()
    }

    pub const fn hub_key(&self) -> &PublicKey {
        todo!()
    }

    pub const fn tls(&self) -> TokenTls {
        todo!()
    }

    /// The pairing key's seed, which the agent signs `Pair` with and never
    /// sends.
    pub const fn pairing_seed(&self) -> &SigningSeed {
        todo!()
    }

    pub const fn address(&self) -> &HubAddress {
        todo!()
    }

    /// The token as text, a secret until it is used.
    pub fn to_text(&self) -> SecretValue {
        todo!()
    }

    /// Reads a token as it was shown: the prefix, the checksum and the
    /// version are checked before any field is read. The decoding is strict,
    /// so one token has one text.
    pub fn parse(text: &str) -> Result<Self, TokenRefused> {
        todo!()
    }
}

/// The first 4 bytes of the SHA-256 of `bytes`.
fn checksum(bytes: &[u8]) -> [u8; CHECKSUM_BYTES] {
    todo!()
}

/// The token's bytes not read yet; a field past the end is a malformed token.
struct Fields<'a>(&'a [u8]);

impl<'a> Fields<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], TokenRefused> {
        todo!()
    }

    fn byte(&mut self) -> Result<u8, TokenRefused> {
        todo!()
    }

    fn array(&mut self) -> Result<[u8; KEY_BYTES], TokenRefused> {
        todo!()
    }
}

#[cfg(test)]
mod tests;
