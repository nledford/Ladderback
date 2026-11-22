//! How a key is shown to a person (§16, decisions 120, "The hub key", and
//! 121, "The fingerprint"): its fingerprint, the one thing a person compares
//! and the one ID a screen shows for a key, and a pairing token's token ID,
//! a label that never verifies anything.

use std::fmt::{self, Write as _};

use sha2::{Digest as _, Sha256};

use super::keys::{KEY_BYTES, PublicKey, lower_hex};

/// Crockford's base32 alphabet, upper case: no I, L, O or U.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The SHA-256 of a key: of an Ed25519 key's 32 bytes, or of a TLS
/// certificate's public-key encoding (SubjectPublicKeyInfo). It shows as its
/// first 100 bits, 20 characters of Crockford base32 in five groups of four
/// (`7K3Q-M9XD-2RTV-8HFA-QW5N`), and in full as 64 hex digits on demand.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; KEY_BYTES]);

impl Fingerprint {
    /// The fingerprint of an Ed25519 key: the hub key, an agent key, the
    /// recovery key.
    pub fn of_key(key: &PublicKey) -> Self {
        Self::of_bytes(key.as_bytes())
    }

    /// The fingerprint of any other key's encoding, such as the TLS
    /// certificate's SubjectPublicKeyInfo.
    pub fn of_bytes(encoding: &[u8]) -> Self {
        Self(Sha256::digest(encoding).into())
    }

    /// A fingerprint carried whole, as a pairing token carries the
    /// certificate's.
    pub const fn from_digest(digest: [u8; KEY_BYTES]) -> Self {
        Self(digest)
    }

    pub const fn digest(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    /// All 256 bits as 64 lower-case hex digits.
    pub fn full_hex(&self) -> String {
        lower_hex(&self.0)
    }

    /// Whether `text` is this fingerprint as a person types or pastes it:
    /// its 20 characters in any case, with or without hyphens and
    /// whitespace, with Crockford's aliases (O for 0, I and L for 1).
    pub fn is_typed_as(&self, text: &str) -> bool {
        text.chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .map(|c| match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            })
            .eq(self.symbols())
    }

    /// The first 100 bits as 20 Crockford base32 characters.
    fn symbols(&self) -> impl Iterator<Item = char> {
        let mut first = [0; 16];
        first.copy_from_slice(self.0.first_chunk::<16>().unwrap_or(&[0; 16]));
        let bits = u128::from_be_bytes(first) >> 28;
        (0..20u32).map(move |i| {
            let index = (bits >> (5 * (19 - i))) & 0x1f;
            usize::try_from(index)
                .ok()
                .and_then(|index| CROCKFORD.get(index))
                .copied()
                .map_or('0', char::from)
        })
    }
}

impl fmt::Display for Fingerprint {
    /// The first 100 bits: 20 characters, a hyphen after each four but the
    /// last.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, symbol) in self.symbols().enumerate() {
            if i > 0 && i % 4 == 0 {
                f.write_char('-')?;
            }
            f.write_char(symbol)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

/// The first 8 hex digits, upper case, of a key's SHA-256 (`3F9A8B21`): how
/// a [`TokenId`] is held. No screen shows a key by it; the fingerprint is
/// the one ID a person reads for a key (#1344).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct KeyId([u8; 4]);

impl KeyId {
    pub fn of_key(key: &PublicKey) -> Self {
        Self::of_fingerprint(&Fingerprint::of_key(key))
    }

    fn of_fingerprint(fingerprint: &Fingerprint) -> Self {
        Self(*fingerprint.0.first_chunk::<4>().unwrap_or(&[0; 4]))
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}", u32::from_be_bytes(self.0))
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({self})")
    }
}

/// A pairing token's token ID: the first 8 hex digits of its pairing key's
/// SHA-256, by which the hub lists and revokes unused tokens.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId(KeyId);

impl TokenId {
    pub fn of_pairing_key(key: &PublicKey) -> Self {
        Self(KeyId::of_key(key))
    }

    /// The token ID a person typed, as `--revoke` takes it: 8 hex digits in
    /// either case, with no space; `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.as_bytes();
        if digits.len() != 8 {
            return None;
        }
        let mut bytes = [0_u8; 4];
        let (pairs, _) = digits.as_chunks::<2>();
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (hex_digit(*high)? << 4) | hex_digit(*low)?;
        }
        Some(Self(KeyId(bytes)))
    }
}

/// The value of one hex digit in either case.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenId({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Computed apart from this code, from the definition: SHA-256 of the
    /// key's 32 bytes, its first 100 bits in Crockford base32.
    #[test]
    fn a_fingerprint_shows_its_first_100_bits_in_five_groups() {
        let key = PublicKey::from_bytes([0x11; KEY_BYTES]).unwrap();
        let fingerprint = Fingerprint::of_key(&key);
        assert_eq!(fingerprint.to_string(), "0BA4-K8RZ-QCK7-S3SN-5TCP");
        assert_eq!(
            fingerprint.full_hex(),
            "02d449a31fbb267c8f352e9968a79e3e5fc95c1bbeaa502fd6454ebde5a4bedc"
        );
        assert_eq!(
            format!("{fingerprint:?}"),
            "Fingerprint(0BA4-K8RZ-QCK7-S3SN-5TCP)"
        );
    }

    /// RFC 8032 §7.1, test 1's public key.
    #[test]
    fn another_key_has_its_own_fingerprint() {
        let hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        let bytes: Vec<u8> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let key = PublicKey::from_bytes(bytes.try_into().unwrap()).unwrap();
        assert_eq!(
            Fingerprint::of_key(&key).to_string(),
            "47Z3-3QX1-AJH6-2RKB-Z1A0"
        );
        assert_eq!(KeyId::of_key(&key).to_string(), "21FE31DF");
    }

    /// Every character is in the alphabet, which leaves out I, L, O and U,
    /// whatever the digest.
    #[test]
    fn a_fingerprint_uses_crockfords_alphabet_only() {
        for byte in [0x00, 0x5a, 0xff] {
            let shown = Fingerprint::from_digest([byte; KEY_BYTES]).to_string();
            assert_eq!(shown.len(), 24, "{shown}");
            for (i, c) in shown.chars().enumerate() {
                if i % 5 == 4 {
                    assert_eq!(c, '-', "{shown}");
                } else {
                    assert!(CROCKFORD.contains(&(c as u8)), "{shown}");
                }
            }
        }
        assert_eq!(
            Fingerprint::from_digest([0xff; KEY_BYTES]).to_string(),
            "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ"
        );
        assert_eq!(
            Fingerprint::from_digest([0x00; KEY_BYTES]).to_string(),
            "0000-0000-0000-0000-0000"
        );
    }

    /// The 100th bit is the last one shown and the 101st is not: a digest
    /// that differs at bit 100 shows differently, one at bit 101 the same.
    #[test]
    fn a_fingerprint_shows_exactly_100_bits() {
        let mut at_100 = [0; KEY_BYTES];
        at_100[12] = 0x10;
        assert_eq!(
            Fingerprint::from_digest(at_100).to_string(),
            "0000-0000-0000-0000-0001"
        );
        let mut at_101 = [0; KEY_BYTES];
        at_101[12] = 0x08;
        assert_eq!(
            Fingerprint::from_digest(at_101).to_string(),
            "0000-0000-0000-0000-0000"
        );
        let mut first = [0; KEY_BYTES];
        first[0] = 0x80;
        assert_eq!(
            Fingerprint::from_digest(first).to_string(),
            "G000-0000-0000-0000-0000"
        );
    }

    #[test]
    fn the_fingerprint_of_other_bytes_is_their_sha256() {
        assert_eq!(
            Fingerprint::of_bytes(b"abc").full_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// A key ID is the first 8 hex digits of the same digest, upper case.
    #[test]
    fn a_key_id_is_the_first_eight_hex_digits() {
        let key = PublicKey::from_bytes([0x11; KEY_BYTES]).unwrap();
        assert_eq!(KeyId::of_key(&key).to_string(), "02D449A3");
        assert_eq!(format!("{:?}", KeyId::of_key(&key)), "KeyId(02D449A3)");
        assert_eq!(TokenId::of_pairing_key(&key).to_string(), "02D449A3");
        assert_eq!(
            format!("{:?}", TokenId::of_pairing_key(&key)),
            "TokenId(02D449A3)"
        );
    }

    #[test]
    fn a_token_id_is_read_back_in_either_case() {
        let id = TokenId::of_pairing_key(&PublicKey::from_bytes([0x11; KEY_BYTES]).unwrap());
        assert_eq!(TokenId::parse("02D449A3"), Some(id));
        assert_eq!(TokenId::parse("02d449a3"), Some(id));
        for text in [
            "",
            "02D449A",
            "02D449A31",
            " 02D449A3",
            "02D4-49A3",
            "02D449AG",
            "+2D449A3",
        ] {
            assert_eq!(TokenId::parse(text), None, "{text:?}");
        }
    }
}
