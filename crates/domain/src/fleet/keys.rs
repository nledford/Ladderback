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
    /// Text that isn't 64 lower-case hex digits, as a key file or the pin
    /// holds a key.
    NotHex,
}

/// An Ed25519 public key in its 32-byte encoding, canonical and not of small
/// order. Keys are public, so they compare in ordinary time.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; KEY_BYTES]);

impl PublicKey {
    /// Parses a key that came from outside: a token, a file, the wire.
    pub fn from_bytes(bytes: [u8; KEY_BYTES]) -> Result<Self, KeyRefused> {
        if !y_below_p(&bytes) || x_is_zero_with_sign(&bytes) {
            return Err(KeyRefused::NotCanonical);
        }
        if SMALL_ORDER_KEYS.contains(&bytes) {
            return Err(KeyRefused::SmallOrder);
        }
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    /// The key from 64 lower-case hex digits, as a key file and the pin
    /// hold it (§16, decision 120, "The hub key").
    pub fn from_hex(text: &str) -> Result<Self, KeyRefused> {
        let mut bytes = [0; KEY_BYTES];
        if !decode_hex32(text, &mut bytes) {
            return Err(KeyRefused::NotHex);
        }
        Self::from_bytes(bytes)
    }

    /// The key as 64 lower-case hex digits.
    pub fn to_hex(&self) -> String {
        lower_hex(&self.0)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", lower_hex(&self.0))
    }
}

/// The 255-bit `y` (the sign bit cleared), compared with `p` from the most
/// significant byte down.
fn y_below_p(bytes: &[u8; KEY_BYTES]) -> bool {
    let mut y = *bytes;
    y[KEY_BYTES - 1] &= 0x7f;
    less_than_le(&y, &FIELD_PRIME)
}

/// The two points with `x = 0` are `y = 1` and `y = p - 1`; their canonical
/// encodings have no sign bit, so one with it is another encoding of a
/// small-order point.
fn x_is_zero_with_sign(bytes: &[u8; KEY_BYTES]) -> bool {
    if bytes[KEY_BYTES - 1] & 0x80 == 0 {
        return false;
    }
    let mut y = *bytes;
    y[KEY_BYTES - 1] &= 0x7f;
    y == SMALL_ORDER_KEYS[0] || y == SMALL_ORDER_KEYS[1]
}

/// Whether the little-endian number `a` is below `b`.
fn less_than_le(a: &[u8; KEY_BYTES], b: &[u8; KEY_BYTES]) -> bool {
    a.iter().rev().lt(b.iter().rev())
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
        let s = bytes.last_chunk::<KEY_BYTES>();
        if s.is_some_and(|s| less_than_le(s, &GROUP_ORDER)) {
            Ok(Self(bytes))
        } else {
            Err(SignatureRefused::NotCanonical)
        }
    }

    pub const fn as_bytes(&self) -> &[u8; SIGNATURE_BYTES] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", lower_hex(&self.0))
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
        let seed = Self(Zeroizing::new(bytes));
        bytes.zeroize();
        seed
    }

    /// The seed itself, for the signing adapter alone.
    pub fn expose(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    /// The seed from 64 lower-case hex digits, as its key file holds it,
    /// decoded in place so no copy of it is left behind; `None` for any
    /// other text.
    pub fn from_hex(text: &str) -> Option<Self> {
        let mut seed = Self(Zeroizing::new([0; KEY_BYTES]));
        decode_hex32(text, &mut seed.0).then_some(seed)
    }

    /// The seed as 64 lower-case hex digits, wiped when dropped.
    /// Its buffer is made at its full size, so no copy is left by a
    /// regrowth.
    pub fn to_hex(&self) -> Zeroizing<String> {
        use std::fmt::Write as _;
        let mut text = Zeroizing::new(String::with_capacity(KEY_BYTES * 2));
        for byte in self.0.iter() {
            // Writing to a `String` can't fail.
            let _ = write!(text, "{byte:02x}");
        }
        text
    }
}

impl fmt::Debug for SigningSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SigningSeed(***)")
    }
}

/// Lower-case hex, two digits a byte.
pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a `String` can't fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Decodes exactly 64 lower-case hex digits into `out`, in place; `false`,
/// with `out` partly written, for any other text.
pub(crate) fn decode_hex32(text: &str, out: &mut [u8; KEY_BYTES]) -> bool {
    fn digit(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let text = text.as_bytes();
    if text.len() != KEY_BYTES * 2 {
        return false;
    }
    let (pairs, _) = text.as_chunks::<2>();
    for (byte, &[high, low]) in out.iter_mut().zip(pairs) {
        let (Some(high), Some(low)) = (digit(high), digit(low)) else {
            return false;
        };
        *byte = high << 4 | low;
    }
    true
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
