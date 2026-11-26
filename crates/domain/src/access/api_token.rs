//! API tokens (technical design §10.1, decision 93): what lets a program
//! other than a browser use the API while sign-in is on. A token is
//! `ladderback_<id>_<secret>`; the daemon keeps its label, its permissions, the
//! SHA-256 of its secret and the admin password it was created under, never
//! the secret. Rules only: the time and the random bytes are passed in.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::permissions::Permissions;
use super::{PasswordFingerprint, TOKEN_BYTES};

/// What every token's text starts with, so a secret scanner knows one.
pub const PREFIX: &str = "ladderback_";

/// The most tokens kept at once.
pub const MAX_API_TOKENS: usize = 100;

/// A new token refused: `limit` tokens are kept already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiTokensFull;

/// Whether one more token may be kept while `kept` are: fewer than
/// `limit`. The store counts and asks this in the step that keeps the
/// token, so of adds racing for the last place one is kept.
pub fn admits_api_token(kept: usize, limit: usize) -> Result<(), ApiTokensFull> {
    if kept < limit {
        Ok(())
    } else {
        Err(ApiTokensFull)
    }
}

/// The longest label, in characters.
pub const LABEL_MAX_CHARS: usize = 64;

/// A token's last use is written at most this often, so a busy program
/// doesn't write on every request.
pub const LAST_USE_EVERY: SignedDuration = SignedDuration::from_mins(1);

/// How many characters a token's ID has: lower-case base32.
const ID_CHARS: usize = 12;

/// How many characters a token's secret has: [`TOKEN_BYTES`] as unpadded
/// base64url.
const SECRET_CHARS: usize = 43;

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// A token's permissions: one row set to a level at least (decision 127).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenPermissions(Permissions);

impl TokenPermissions {
    pub fn new(permissions: Permissions) -> Result<Self, ApiTokenProblem> {
        if permissions.is_empty() {
            return Err(ApiTokenProblem::NoPermission);
        }
        Ok(Self(permissions))
    }

    /// What the token may do.
    pub fn get(self) -> Permissions {
        self.0
    }
}

/// Why a token can't be created as asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiTokenProblem {
    /// The label is empty, blank, longer than [`LABEL_MAX_CHARS`], or holds
    /// a control character.
    Label,
    /// Every permission was left at no access.
    NoPermission,
}

/// A token's ID: what it is looked up, listed and revoked by. Not a secret.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ApiTokenId(String);

impl ApiTokenId {
    /// The ID random `bytes` make: their first 60 bits, five to a
    /// character, so every bit drawn counts.
    pub fn from_bytes(bytes: &[u8; TOKEN_BYTES]) -> Self {
        let bits = bytes
            .iter()
            .take(8)
            .fold(0_u64, |bits, byte| (bits << 8) | u64::from(*byte));
        Self(
            (0..ID_CHARS)
                .map(|index| {
                    let quintet = (bits >> (59 - 5 * index)) & 31;
                    let index = usize::try_from(quintet).unwrap_or(0);
                    char::from(BASE32.get(index).copied().unwrap_or(b'a'))
                })
                .collect(),
        )
    }

    /// Exactly `ID_CHARS` characters of lower-case base32.
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == ID_CHARS && text.bytes().all(|byte| BASE32.contains(&byte)))
            .then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the admin calls a token: 1 to [`LABEL_MAX_CHARS`] characters, not
/// all blank, with no control character.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiTokenLabel(String);

impl ApiTokenLabel {
    pub fn new(text: String) -> Result<Self, ApiTokenProblem> {
        let fits = !text.trim().is_empty()
            && text.chars().count() <= LABEL_MAX_CHARS
            && !text.chars().any(char::is_control);
        if fits {
            Ok(Self(text))
        } else {
            Err(ApiTokenProblem::Label)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A token's secret: [`TOKEN_BYTES`] random bytes. Its `Debug` hides it, and
/// it has no `==`: it is checked only through its SHA-256 (I4). It wipes its
/// bytes when it is dropped (§10.2).
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ApiTokenSecret([u8; TOKEN_BYTES]);

impl ApiTokenSecret {
    pub fn from_bytes(bytes: [u8; TOKEN_BYTES]) -> Self {
        Self(bytes)
    }

    fn digest(&self) -> ApiTokenDigest {
        // A slice, not `self.0`: that would copy the bytes out unwiped.
        ApiTokenDigest(Sha256::digest(self.0.as_slice()).into())
    }
}

impl std::fmt::Debug for ApiTokenSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiTokenSecret(***)")
    }
}

/// The SHA-256 of a token's secret: all the daemon keeps of it. Compared only
/// in constant time.
#[derive(Clone)]
pub struct ApiTokenDigest([u8; 32]);

impl ApiTokenDigest {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for ApiTokenDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiTokenDigest(***)")
    }
}

/// A token's whole text, `ladderback_<id>_<secret>`, as shown once when it
/// is created. Its `Debug` hides it (I4), and it wipes its bytes when it is
/// dropped (§10.2).
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ApiTokenText(String);

impl ApiTokenText {
    pub fn new(id: &ApiTokenId, secret: &ApiTokenSecret) -> Self {
        let mut text = String::with_capacity(PREFIX.len() + ID_CHARS + 1 + SECRET_CHARS);
        text.push_str(PREFIX);
        text.push_str(id.as_str());
        text.push('_');
        let (triples, rest) = secret.0.as_chunks::<3>();
        for [a, b, c] in triples {
            let bits = (u32::from(*a) << 16) | (u32::from(*b) << 8) | u32::from(*c);
            for shift in [18, 12, 6, 0] {
                text.push(base64_char(bits >> shift));
            }
        }
        if let [a, b] = rest {
            let bits = (u32::from(*a) << 16) | (u32::from(*b) << 8);
            for shift in [18, 12, 6] {
                text.push(base64_char(bits >> shift));
            }
        }
        Self(text)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiTokenText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiTokenText(***)")
    }
}

fn base64_char(sextet: u32) -> char {
    let index = usize::try_from(sextet & 63).unwrap_or(0);
    char::from(BASE64URL.get(index).copied().unwrap_or(b'A'))
}

fn base64_value(char: u8) -> Option<u32> {
    BASE64URL
        .iter()
        .position(|candidate| *candidate == char)
        .and_then(|index| u32::try_from(index).ok())
}

/// A token as a request presents it: its ID and the secret to check.
#[derive(Debug)]
pub struct PresentedApiToken {
    id: ApiTokenId,
    secret: ApiTokenSecret,
}

impl PresentedApiToken {
    /// Exactly what [`ApiTokenText`] writes: the prefix, the ID, `_` and the
    /// secret, the secret's unused last bits zero, so a secret has one text.
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix(PREFIX)?;
        let (id, rest) = (rest.get(..ID_CHARS)?, rest.get(ID_CHARS..)?);
        let id = ApiTokenId::parse(id)?;
        let encoded = rest.strip_prefix('_')?.as_bytes();
        if encoded.len() != SECRET_CHARS {
            return None;
        }
        // Decoded straight into the secret, which wipes itself if this fails.
        let mut secret = ApiTokenSecret([0; TOKEN_BYTES]);
        let (quads, tail) = encoded.as_chunks::<4>();
        let (triples, last) = secret.0.as_chunks_mut::<3>();
        for (triple, quad) in triples.iter_mut().zip(quads) {
            let mut bits = 0;
            for char in quad {
                bits = (bits << 6) | base64_value(*char)?;
            }
            let [_, high, middle, low] = bits.to_be_bytes();
            *triple = [high, middle, low];
        }
        let [a, b, c] = tail else { return None };
        let bits = (base64_value(*a)? << 12) | (base64_value(*b)? << 6) | base64_value(*c)?;
        if bits & 0b11 != 0 {
            return None;
        }
        let [_, _, high, low] = (bits >> 2).to_be_bytes();
        let [first, second] = last else { return None };
        (*first, *second) = (high, low);
        Some(Self { id, secret })
    }

    pub fn id(&self) -> &ApiTokenId {
        &self.id
    }
}

/// One kept token: who it is (its ID and label), what it may do (its
/// permissions), the SHA-256 of its secret, the admin password it was created
/// under, when it was created and when last used.
#[derive(Clone, Debug)]
pub struct ApiToken {
    pub id: ApiTokenId,
    pub label: ApiTokenLabel,
    pub permissions: TokenPermissions,
    pub digest: ApiTokenDigest,
    pub password: PasswordFingerprint,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
}

impl ApiToken {
    /// A token created at `now` under the admin password `password`, which
    /// keeps only the SHA-256 of `secret`.
    pub fn issue(
        id: ApiTokenId,
        label: ApiTokenLabel,
        permissions: TokenPermissions,
        secret: &ApiTokenSecret,
        password: PasswordFingerprint,
        now: Timestamp,
    ) -> Self {
        Self {
            id,
            label,
            permissions,
            digest: secret.digest(),
            password,
            created_at: now,
            last_used_at: None,
        }
    }

    /// Whether `presented` is this token, with its own secret, while the
    /// admin password it was created under is kept still. The secret is
    /// compared by its SHA-256, in constant time.
    pub fn accepts(&self, presented: &PresentedApiToken, under: PasswordFingerprint) -> bool {
        let same_secret: bool = presented.secret.digest().0.ct_eq(&self.digest.0).into();
        same_secret && presented.id == self.id && under == self.password
    }

    /// Whether a use at `now` is to be written: the first, and then one at
    /// least [`LAST_USE_EVERY`] after the last written.
    pub fn use_to_record(&self, now: Timestamp) -> bool {
        self.last_used_at
            .is_none_or(|last| now.duration_since(last) >= LAST_USE_EVERY)
    }
}

/// Whether a token, given the `run` scope, may cancel an operation of `kind`:
/// a backup only. Stopping maintenance or an initialization is left to a
/// signed-in admin (decision 93).
pub fn cancels(kind: crate::execution::OperationKind) -> bool {
    kind == crate::execution::OperationKind::Backup
}

#[cfg(test)]
mod tests;
