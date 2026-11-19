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

/// The address the agent first connects to, at most 200 bytes: `https://`
/// or `http://`, then at least one character, with no space or control
/// character. Only this much is checked here; the address carries no trust
/// (the hub key and the fingerprint do), and the connection reads the host
/// and port from it and refuses a plain `http://` one off loopback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubAddress(String);

/// Text that isn't a hub address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidHubAddress;

impl HubAddress {
    pub fn parse(text: &str) -> Result<Self, InvalidHubAddress> {
        let rest = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://"));
        let well_formed = rest.is_some_and(|rest| !rest.is_empty())
            && text.len() <= MAX_ADDRESS_BYTES
            && !text.chars().any(|c| c.is_whitespace() || c.is_control());
        if well_formed {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidHubAddress)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
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
        Self {
            hub_key,
            tls,
            pairing_seed,
            address,
        }
    }

    pub const fn hub_key(&self) -> &PublicKey {
        &self.hub_key
    }

    pub const fn tls(&self) -> TokenTls {
        self.tls
    }

    /// The pairing key's seed, which the agent signs `Pair` with and never
    /// sends.
    pub const fn pairing_seed(&self) -> &SigningSeed {
        &self.pairing_seed
    }

    pub const fn address(&self) -> &HubAddress {
        &self.address
    }

    /// The token as text, a secret until it is used.
    pub fn to_text(&self) -> SecretValue {
        let address = self.address.as_str().as_bytes();
        let mut bytes = Zeroizing::new(Vec::with_capacity(FIXED_BYTES + address.len()));
        bytes.push(TOKEN_VERSION);
        bytes.extend_from_slice(self.hub_key.as_bytes());
        match self.tls {
            TokenTls::Ca => {
                bytes.push(0);
                bytes.extend_from_slice(&[0; KEY_BYTES]);
            }
            TokenTls::Pinned(fingerprint) => {
                bytes.push(1);
                bytes.extend_from_slice(fingerprint.digest());
            }
        }
        bytes.extend_from_slice(self.pairing_seed.expose());
        // `HubAddress` holds at most 200 bytes.
        bytes.push(u8::try_from(address.len()).unwrap_or(u8::MAX));
        bytes.extend_from_slice(address);
        let checksum = checksum(&bytes);
        bytes.extend_from_slice(&checksum);
        let mut text = Zeroizing::new(String::with_capacity(MAX_TOKEN_TEXT));
        text.push_str(TOKEN_PREFIX);
        URL_SAFE_NO_PAD.encode_string(bytes.as_slice(), &mut text);
        SecretValue::new(text.as_str())
    }

    /// Reads a token as it was shown: the prefix, the checksum and the
    /// version are checked before any field is read. The decoding is strict,
    /// so one token has one text. Text longer than this version's longest
    /// token is malformed before it is decoded, so a later layout that is
    /// longer shows as malformed rather than unsupported.
    pub fn parse(text: &str) -> Result<Self, TokenRefused> {
        if text.len() > MAX_TOKEN_TEXT {
            return Err(TokenRefused::Malformed);
        }
        let encoded = text
            .strip_prefix(TOKEN_PREFIX)
            .ok_or(TokenRefused::Malformed)?;
        // Decoded into a buffer that is wiped however the decoding ends: a
        // bad character late in the text leaves the seed's bytes written.
        let mut bytes = Zeroizing::new(vec![0; MAX_TOKEN_TEXT]);
        let length = URL_SAFE_NO_PAD
            .decode_slice(encoded, bytes.as_mut_slice())
            .map_err(|_| TokenRefused::Malformed)?;
        let decoded = bytes.get(..length).ok_or(TokenRefused::Malformed)?;
        let (body, sum) = decoded
            .split_last_chunk::<CHECKSUM_BYTES>()
            .ok_or(TokenRefused::Malformed)?;
        if checksum(body) != *sum {
            return Err(TokenRefused::Malformed);
        }
        let mut fields = Fields(body);
        match fields.byte()? {
            TOKEN_VERSION => {}
            // No token had version 0.
            0 => return Err(TokenRefused::Malformed),
            _ => return Err(TokenRefused::Unsupported),
        }
        let hub_key =
            PublicKey::from_bytes(fields.array()?).map_err(|_| TokenRefused::Malformed)?;
        let tls_byte = fields.byte()?;
        let fingerprint = fields.array()?;
        let tls = match tls_byte {
            0 if fingerprint == [0; KEY_BYTES] => TokenTls::Ca,
            1 => TokenTls::Pinned(Fingerprint::from_digest(fingerprint)),
            _ => return Err(TokenRefused::Malformed),
        };
        let pairing_seed = SigningSeed::from_bytes(fields.array()?);
        let length = usize::from(fields.byte()?);
        let address = fields.take(length)?;
        if !fields.0.is_empty() {
            return Err(TokenRefused::Malformed);
        }
        let address = std::str::from_utf8(address)
            .ok()
            .and_then(|text| HubAddress::parse(text).ok())
            .ok_or(TokenRefused::Malformed)?;
        Ok(Self::new(hub_key, tls, pairing_seed, address))
    }

    /// Reads a token as a person pasted it (§16, decision 121, "`ladderback
    /// pair`"): whitespace and line breaks anywhere, zero-width characters,
    /// and quotes or angle brackets around it are dropped, and the prefix is
    /// read in any case with `-` for `_`. The rest must be the token as
    /// [`parse`](Self::parse) reads it: base64 is case-sensitive, so a
    /// letter whose case was changed fails the checksum. Text longer than a
    /// token could be with all of that around it is malformed unread.
    pub fn parse_pasted(text: &str) -> Result<Self, TokenRefused> {
        if text.len() > MAX_PASTED_TEXT {
            return Err(TokenRefused::Malformed);
        }
        let mut kept = Zeroizing::new(String::with_capacity(text.len()));
        kept.extend(
            text.chars()
                .filter(|&c| !c.is_whitespace() && !ZERO_WIDTH.contains(&c)),
        );
        let mut inner = kept.as_str();
        while let Some(unwrapped) = unquoted(inner) {
            inner = unwrapped;
        }
        let mut token = Zeroizing::new(String::with_capacity(inner.len()));
        match inner.get(..TOKEN_PREFIX.len()) {
            Some(prefix) if is_prefix_as_pasted(prefix) => {
                token.push_str(TOKEN_PREFIX);
                token.push_str(inner.get(TOKEN_PREFIX.len()..).unwrap_or_default());
            }
            _ => token.push_str(inner),
        }
        Self::parse(&token)
    }

    /// The same token for an agent that reaches the hub at `address`
    /// rather than the token's (decision 121, `ladderback pair <address>`).
    /// The hub key and the certificate stay the token's: the address
    /// carries no trust.
    #[must_use]
    pub fn with_address(self, address: HubAddress) -> Self {
        Self { address, ..self }
    }
}

/// The first 4 bytes of the SHA-256 of `bytes`.
fn checksum(bytes: &[u8]) -> [u8; CHECKSUM_BYTES] {
    let digest: [u8; KEY_BYTES] = Sha256::digest(bytes).into();
    *digest
        .first_chunk::<CHECKSUM_BYTES>()
        .unwrap_or(&[0; CHECKSUM_BYTES])
}

/// The longest pasted text read: room for a token wrapped on every
/// character with a line break and a zero-width character.
const MAX_PASTED_TEXT: usize = MAX_TOKEN_TEXT * 8;

/// The zero-width characters a chat app or a web page may put in a paste.
const ZERO_WIDTH: [char; 5] = ['\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}'];

/// `text` without one pair of quotes or angle brackets around it, or `None`
/// when it has none.
fn unquoted(text: &str) -> Option<&str> {
    const PAIRS: [(char, char); 6] = [
        ('"', '"'),
        ('\'', '\''),
        ('`', '`'),
        ('<', '>'),
        ('\u{201c}', '\u{201d}'),
        ('\u{2018}', '\u{2019}'),
    ];
    PAIRS.iter().find_map(|&(open, close)| {
        text.strip_prefix(open)
            .and_then(|rest| rest.strip_suffix(close))
    })
}

/// Whether `text` is the token's prefix in any case, with `-` for `_`.
fn is_prefix_as_pasted(text: &str) -> bool {
    text.len() == TOKEN_PREFIX.len()
        && text.bytes().zip(TOKEN_PREFIX.bytes()).all(|(pasted, own)| {
            pasted.eq_ignore_ascii_case(&own) || (pasted == b'-' && own == b'_')
        })
}

/// The token's bytes not read yet; a field past the end is a malformed token.
struct Fields<'a>(&'a [u8]);

impl<'a> Fields<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], TokenRefused> {
        if count > self.0.len() {
            return Err(TokenRefused::Malformed);
        }
        let (field, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(field)
    }

    fn byte(&mut self) -> Result<u8, TokenRefused> {
        let (&first, rest) = self.0.split_first().ok_or(TokenRefused::Malformed)?;
        self.0 = rest;
        Ok(first)
    }

    fn array(&mut self) -> Result<[u8; KEY_BYTES], TokenRefused> {
        let (field, rest) = self
            .0
            .split_first_chunk::<KEY_BYTES>()
            .ok_or(TokenRefused::Malformed)?;
        self.0 = rest;
        Ok(*field)
    }
}

#[cfg(test)]
mod tests;
