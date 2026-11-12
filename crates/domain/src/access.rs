//! Sign-in (technical design §10.1, decision 70): the admin password as typed,
//! the admin's sessions, the token a changed request carries, and the wait
//! after failed sign-ins. Rules only: the time, the random bytes and the
//! client's address are passed in.

use std::collections::HashMap;
use std::net::IpAddr;

use jiff::{SignedDuration, Timestamp};

/// A session ends once it has gone this long unused.
pub const SESSION_IDLE: SignedDuration = SignedDuration::from_hours(12);

/// A session ends this long after it started, however much it is used.
pub const SESSION_LIFETIME: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The most sessions kept at once.
pub const MAX_SESSIONS: usize = 16;

/// The longest password taken, in bytes.
pub const PASSWORD_MAX_BYTES: usize = 1024;

/// The shortest admin password `set-password` keeps, in characters.
pub const PASSWORD_MIN_CHARS: usize = 8;

/// How many random bytes a token holds.
pub const TOKEN_BYTES: usize = 32;

/// The longest wait after failed sign-ins.
pub const MAX_WAIT: SignedDuration = SignedDuration::from_mins(5);

/// An address that has tried nothing for this long starts over.
pub const FORGET_AFTER: SignedDuration = SignedDuration::from_hours(24);

/// The most addresses whose failed sign-ins are remembered.
pub const MAX_CLIENTS: usize = 1024;

/// A password as typed, at the sign-in form or to `set-password`.
#[derive(Clone)]
pub struct Password(String);

/// Why a text is not a password Ladderback takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordProblem {
    /// Nothing was typed.
    Empty,
    /// An admin password shorter than [`PASSWORD_MIN_CHARS`].
    TooShort,
    /// Longer than [`PASSWORD_MAX_BYTES`].
    TooLong,
}

impl Password {
    pub fn parse(_text: String) -> Result<Self, PasswordProblem> {
        todo!()
    }

    pub fn new_admin(_text: String) -> Result<Self, PasswordProblem> {
        todo!()
    }

    pub fn expose(&self) -> &str {
        todo!()
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

/// A secret the daemon hands out: a session's cookie or its CSRF token.
#[derive(Clone)]
pub struct Token([u8; TOKEN_BYTES]);

impl Token {
    pub fn from_bytes(_bytes: [u8; TOKEN_BYTES]) -> Self {
        todo!()
    }

    pub fn parse(_text: &str) -> Option<Self> {
        todo!()
    }

    pub fn to_text(&self) -> String {
        todo!()
    }

    pub fn matches(&self, _other: &Self) -> bool {
        todo!()
    }

    fn digest(&self) -> Digest {
        todo!()
    }
}

/// A token's SHA-256.
type Digest = [u8; 32];

impl std::fmt::Debug for Token {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

/// Which admin password a session started under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PasswordFingerprint(u64);

impl PasswordFingerprint {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// One signed-in browser.
#[derive(Clone, Debug)]
pub struct Session {
    csrf: Token,
    password: PasswordFingerprint,
    started: Timestamp,
    last_used: Timestamp,
}

impl Session {
    pub fn csrf(&self) -> &Token {
        todo!()
    }

    pub fn accepts(&self, _presented: &Token) -> bool {
        todo!()
    }

    pub fn ends_at(&self) -> Timestamp {
        todo!()
    }

    pub fn live_at(&self, _now: Timestamp) -> bool {
        todo!()
    }
}

/// The live sessions.
#[derive(Debug, Default)]
pub struct Sessions {
    by_digest: HashMap<Digest, Session>,
}

impl Sessions {
    pub fn start(
        &mut self,
        _token: Token,
        _csrf: Token,
        _password: PasswordFingerprint,
        _now: Timestamp,
    ) -> Session {
        todo!()
    }

    pub fn use_at(
        &mut self,
        _token: &Token,
        _password: PasswordFingerprint,
        _now: Timestamp,
    ) -> Option<&Session> {
        todo!()
    }

    pub fn peek(
        &self,
        _token: &Token,
        _password: PasswordFingerprint,
        _now: Timestamp,
    ) -> Option<&Session> {
        todo!()
    }

    pub fn end(&mut self, _token: &Token) {
        todo!()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        todo!()
    }
}

/// Who a sign-in came from, as far as its wait goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Client {
    V4(u32),
    V6Network(u64),
}

impl Client {
    pub fn of(_address: IpAddr) -> Self {
        todo!()
    }
}

/// The wait after failed sign-ins (§10.1).
#[derive(Debug, Default)]
pub struct SignInWaits {
    by_client: HashMap<Client, ()>,
}

impl SignInWaits {
    pub fn attempt(&mut self, _client: Client, _now: Timestamp) -> Result<(), SignedDuration> {
        todo!()
    }

    pub fn succeeded(&mut self, _client: Client) {
        todo!()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        todo!()
    }
}

/// The wait after `failed` sign-ins in a row.
fn wait_after(_failed: u32) -> SignedDuration {
    todo!()
}

#[cfg(test)]
mod tests;
