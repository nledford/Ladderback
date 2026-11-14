//! API tokens (technical design §10.1, decision 93): what lets a program
//! other than a browser use the API while sign-in is on. A token is
//! `ladderback_<id>_<secret>`; the daemon keeps its label, its scopes, the
//! SHA-256 of its secret and the admin password it was created under, never
//! the secret. Rules only: the time and the random bytes are passed in.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

use super::{PasswordFingerprint, TOKEN_BYTES};

/// What every token's text starts with, so a secret scanner knows one.
pub const PREFIX: &str = "ladderback_";

/// The most tokens kept at once.
pub const MAX_API_TOKENS: usize = 100;

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

/// What a token lets a program do, each opening the routes the daemon's scope
/// table names for it. Scopes don't imply each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    Read,
    Run,
    Preview,
    Propose,
}

impl Scope {
    /// Every scope, in the order they are listed.
    pub const ALL: [Self; 4] = [Self::Read, Self::Run, Self::Preview, Self::Propose];

    /// Its name on the wire and in the store.
    pub fn as_str(self) -> &'static str {
        todo!()
    }

    pub fn parse(text: &str) -> Option<Self> {
        todo!()
    }

    fn bit(self) -> u8 {
        todo!()
    }
}

/// A token's scopes: one at least.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scopes(u8);

impl Scopes {
    pub fn new(scopes: impl IntoIterator<Item = Scope>) -> Result<Self, ApiTokenProblem> {
        todo!()
    }

    /// Whether the token was given `scope`.
    pub fn allows(self, scope: Scope) -> bool {
        todo!()
    }

    /// The scopes, in [`Scope::ALL`]'s order.
    pub fn iter(self) -> impl Iterator<Item = Scope> {
        todo!() as std::iter::Empty<Scope>
    }
}

/// Why a token can't be created as asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiTokenProblem {
    /// The label is empty, blank, longer than [`LABEL_MAX_CHARS`], or holds
    /// a control character.
    Label,
    /// No scope was given.
    NoScope,
}

/// A token's ID: what it is looked up, listed and revoked by. Not a secret.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ApiTokenId(String);

impl ApiTokenId {
    /// The ID random `bytes` make: their first 60 bits, five to a
    /// character, so every bit drawn counts.
    pub fn from_bytes(bytes: &[u8; TOKEN_BYTES]) -> Self {
        todo!()
    }

    /// Exactly [`ID_CHARS`] characters of lower-case base32.
    pub fn parse(text: &str) -> Option<Self> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        todo!()
    }
}

/// What the admin calls a token: 1 to [`LABEL_MAX_CHARS`] characters, not
/// all blank, with no control character.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiTokenLabel(String);

impl ApiTokenLabel {
    pub fn new(text: String) -> Result<Self, ApiTokenProblem> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        todo!()
    }
}

/// A token's secret: [`TOKEN_BYTES`] random bytes. Its `Debug` hides it, and
/// it has no `==`: it is checked only through its SHA-256 (I4).
#[derive(Clone)]
pub struct ApiTokenSecret([u8; TOKEN_BYTES]);

impl ApiTokenSecret {
    pub fn from_bytes(bytes: [u8; TOKEN_BYTES]) -> Self {
        todo!()
    }

    fn digest(&self) -> ApiTokenDigest {
        todo!()
    }
}

impl std::fmt::Debug for ApiTokenSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

/// The SHA-256 of a token's secret: all the daemon keeps of it. Compared only
/// in constant time.
#[derive(Clone)]
pub struct ApiTokenDigest([u8; 32]);

impl ApiTokenDigest {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        todo!()
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        todo!()
    }
}

impl std::fmt::Debug for ApiTokenDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

/// A token's whole text, `ladderback_<id>_<secret>`, as shown once when it
/// is created. Its `Debug` hides it (I4).
pub struct ApiTokenText(String);

impl ApiTokenText {
    pub fn new(id: &ApiTokenId, secret: &ApiTokenSecret) -> Self {
        todo!()
    }

    pub fn expose(&self) -> &str {
        todo!()
    }
}

impl std::fmt::Debug for ApiTokenText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

fn base64_char(sextet: u32) -> char {
        todo!()
    }

fn base64_value(char: u8) -> Option<u32> {
        todo!()
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
        todo!()
    }

    pub fn id(&self) -> &ApiTokenId {
        todo!()
    }
}

/// One kept token: who it is (its ID and label), what it may do (its
/// scopes), the SHA-256 of its secret, the admin password it was created
/// under, when it was created and when last used.
#[derive(Clone, Debug)]
pub struct ApiToken {
    pub id: ApiTokenId,
    pub label: ApiTokenLabel,
    pub scopes: Scopes,
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
        scopes: Scopes,
        secret: &ApiTokenSecret,
        password: PasswordFingerprint,
        now: Timestamp,
    ) -> Self {
        todo!()
    }

    /// Whether `presented` is this token, with its own secret, while the
    /// admin password it was created under is kept still. The secret is
    /// compared by its SHA-256, in constant time.
    pub fn accepts(&self, presented: &PresentedApiToken, under: PasswordFingerprint) -> bool {
        todo!()
    }

    /// Whether a use at `now` is to be written: the first, and then one at
    /// least [`LAST_USE_EVERY`] after the last written.
    pub fn use_to_record(&self, now: Timestamp) -> bool {
        todo!()
    }
}

/// Whether a token, given the `run` scope, may cancel an operation of `kind`:
/// a backup only. Stopping maintenance or an initialisation is left to a
/// signed-in admin (decision 93).
pub fn cancels(kind: crate::execution::OperationKind) -> bool {
        todo!()
    }

#[cfg(test)]
mod tests;
