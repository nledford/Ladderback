//! Ed25519 keys, seeds and signatures as the hub protocol accepts them
//! (RFC 8032; §16, decision 120, "Crates"). Parsing refuses what the
//! signing library would let through: a small-order public key, and with it
//! any encoding that isn't canonical, so no key the protocol stores or
//! verifies with can make a forged signature verify for every message.

use std::fmt;

use zeroize::{Zeroize, Zeroizing};

/// The bytes of a public key, a seed and a nonce.
pub const KEY_BYTES: usize = 32;

/// The bytes of a signature: `R`, then `S`.
pub const SIGNATURE_BYTES: usize = 64;

/// The eight encodings of the curve's small-order points (orders 1, 2, 4
/// and 8): the identity, `(0, -1)`, the two points of order 4 and the four of
/// order 8, each with the sign bit as the canonical encoding sets it.
pub const SMALL_ORDER_KEYS: [[u8; KEY_BYTES]; 8] = [
    hex32("0100000000000000000000000000000000000000000000000000000000000000"),
    hex32("ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"),
    hex32("0000000000000000000000000000000000000000000000000000000000000000"),
    hex32("0000000000000000000000000000000000000000000000000000000000000080"),
    hex32("c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a"),
    hex32("c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa"),
    hex32("26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05"),
    hex32("26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85"),
];

/// The group order `L = 2^252 + 27742317777372353535851937790883648493`,
/// little-endian, as a signature's `S` is.
const GROUP_ORDER: [u8; KEY_BYTES] =
    hex32("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");

/// The field's prime `p = 2^255 - 19`, little-endian, as a key's `y` is.
const FIELD_PRIME: [u8; KEY_BYTES] =
    hex32("edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");

/// Why bytes aren't a public key the protocol accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRefused {
    /// The encoding isn't the canonical one of any point: its `y` isn't
    /// below `p`, or a point whose `x` is 0 carries a sign bit.
    NotCanonical,
    /// A point of small order, under which a signature can verify for every
    /// message (§16, decision 120, "Crates").
    SmallOrder,
}

/// An Ed25519 public key in its 32-byte encoding, canonical and not of small
/// order. Keys are public, so they compare in ordinary time.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; KEY_BYTES]);

impl PublicKey {
    /// Parses a key that came from outside: a token, a file, the wire.
    pub fn from_bytes(bytes: [u8; KEY_BYTES]) -> Result<Self, KeyRefused> {
        todo!()
    }

    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        todo!()
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// The 255-bit `y` (the sign bit cleared), compared with `p` from the most
/// significant byte down.
fn y_below_p(bytes: &[u8; KEY_BYTES]) -> bool {
    todo!()
}

/// The two points with `x = 0` are `y = 1` and `y = p - 1`; their canonical
/// encodings have no sign bit, so one with it is another encoding of a
/// small-order point.
fn x_is_zero_with_sign(bytes: &[u8; KEY_BYTES]) -> bool {
    todo!()
}

/// Whether the little-endian number `a` is below `b`.
fn less_than_le(a: &[u8; KEY_BYTES], b: &[u8; KEY_BYTES]) -> bool {
    todo!()
}

/// Why bytes aren't a signature the protocol accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureRefused {
    /// `S` isn't below the group order, so the signature is a malleated
    /// copy of another (RFC 8032, §5.1.7).
    NotCanonical,
}

/// An Ed25519 signature whose `S` is below the group order, so one
/// signature has one encoding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; SIGNATURE_BYTES]);

impl Signature {
    pub fn from_bytes(bytes: [u8; SIGNATURE_BYTES]) -> Result<Self, SignatureRefused> {
        todo!()
    }

    pub const fn as_bytes(&self) -> &[u8; SIGNATURE_BYTES] {
        todo!()
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// The 32-byte seed an Ed25519 key pair is made from: the private key. It
/// comes from the `Random` port or a key file, its `Debug` form never shows
/// it, and it wipes its bytes when it is dropped (I4, §10.2).
#[derive(Clone)]
pub struct SigningSeed(Zeroizing<[u8; KEY_BYTES]>);

impl SigningSeed {
    /// Takes the bytes and wipes the copy it was given.
    pub fn from_bytes(mut bytes: [u8; KEY_BYTES]) -> Self {
        todo!()
    }

    /// The seed itself, for the signing adapter alone.
    pub fn expose(&self) -> &[u8; KEY_BYTES] {
        todo!()
    }
}

impl fmt::Debug for SigningSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Lower-case hex, two digits a byte.
pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    todo!()
}

/// 64 hex digits as 32 bytes, at compile time; a bad constant fails the
/// build.
#[expect(
    clippy::indexing_slicing,
    reason = "evaluated only in constants: an index out of range fails the build, never a run"
)]
const fn hex32(text: &str) -> [u8; KEY_BYTES] {
    const fn digit(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            // Not a lower-case hex digit: out of range, so the build fails.
            _ => [0][c as usize],
        }
    }
    let text = text.as_bytes();
    let mut bytes = [0; KEY_BYTES];
    let mut i = 0;
    while i < KEY_BYTES {
        bytes[i] = digit(text[2 * i]) << 4 | digit(text[2 * i + 1]);
        i += 1;
    }
    bytes
}

#[cfg(test)]
mod tests;
