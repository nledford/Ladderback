//! Sign-in (technical design §10.1, decision 70): the admin password as typed,
//! the admin's sessions, the token a changed request carries, and the wait
//! after failed sign-ins. Rules only: the time, the random bytes and the
//! client's address are passed in.

use std::collections::HashMap;
use std::net::IpAddr;

use jiff::{SignedDuration, Timestamp};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

/// A session ends once it has gone this long unused.
pub const SESSION_IDLE: SignedDuration = SignedDuration::from_hours(12);

/// A session ends this long after it started, however much it is used.
pub const SESSION_LIFETIME: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The most sessions kept at once. Only the admin password starts one, so
/// the cap only bounds what a person who has it can make the daemon keep;
/// the session used longest ago makes room for a new one.
pub const MAX_SESSIONS: usize = 16;

/// The longest password taken, in bytes: an attempt is hashed, and a hash of
/// a body-sized text would cost the daemon for nothing.
pub const PASSWORD_MAX_BYTES: usize = 1024;

/// The shortest admin password `set-password` keeps, in characters.
pub const PASSWORD_MIN_CHARS: usize = 8;

/// How many random bytes a token holds.
pub const TOKEN_BYTES: usize = 32;

/// The longest wait after failed sign-ins.
pub const MAX_WAIT: SignedDuration = SignedDuration::from_mins(5);

/// An address that has tried nothing for this long starts over.
pub const FORGET_AFTER: SignedDuration = SignedDuration::from_hours(24);

/// The most addresses whose failed sign-ins are remembered. Addresses come
/// from whoever reaches the daemon, so the table is bounded.
pub const MAX_CLIENTS: usize = 1024;

/// A password as typed, at the sign-in form or to `set-password`. Never
/// shown: its `Debug` hides it (I4).
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
    /// A password attempt: anything from one byte to [`PASSWORD_MAX_BYTES`].
    pub fn parse(text: String) -> Result<Self, PasswordProblem> {
        if text.is_empty() {
            Err(PasswordProblem::Empty)
        } else if text.len() > PASSWORD_MAX_BYTES {
            Err(PasswordProblem::TooLong)
        } else {
            Ok(Self(text))
        }
    }

    /// A new admin password: an attempt of at least [`PASSWORD_MIN_CHARS`]
    /// characters.
    pub fn new_admin(text: String) -> Result<Self, PasswordProblem> {
        let password = Self::parse(text)?;
        if password.0.chars().count() < PASSWORD_MIN_CHARS {
            return Err(PasswordProblem::TooShort);
        }
        Ok(password)
    }

    /// The text, for the hash that checks it and nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(***)")
    }
}

/// A secret the daemon hands out: a session's cookie or its CSRF token,
/// [`TOKEN_BYTES`] random bytes written as lower-case hex. Its `Debug` hides
/// it, and it has no `==`: it is compared only by [`Self::matches`], in
/// constant time, and looked up only by its SHA-256, so neither the time a
/// comparison takes nor the time a lookup takes tells how much of a guess
/// was right.
#[derive(Clone)]
pub struct Token([u8; TOKEN_BYTES]);

impl Token {
    pub fn from_bytes(bytes: [u8; TOKEN_BYTES]) -> Self {
        Self(bytes)
    }

    /// Exactly `2 * TOKEN_BYTES` lower-case hex digits, as [`Self::to_text`]
    /// writes them; anything else is no token.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != TOKEN_BYTES * 2 {
            return None;
        }
        let mut bytes = [0; TOKEN_BYTES];
        let (pairs, _) = text.as_bytes().as_chunks::<2>();
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (hex_digit(*high)? << 4) | hex_digit(*low)?;
        }
        Some(Self(bytes))
    }

    pub fn to_text(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(TOKEN_BYTES * 2);
        for byte in self.0 {
            for nibble in [byte >> 4, byte & 0x0f] {
                text.push(char::from(
                    DIGITS.get(usize::from(nibble)).copied().unwrap_or(b'0'),
                ));
            }
        }
        text
    }

    /// Whether `other` is this token, in a time that doesn't depend on where
    /// they differ.
    pub fn matches(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }

    /// The SHA-256 of the token, which the sessions are kept by: a lookup
    /// compares digests, and a digest's matching prefix says nothing of the
    /// token's.
    fn digest(&self) -> Digest {
        Sha256::digest(self.0).into()
    }
}

/// A token's SHA-256.
type Digest = [u8; 32];

fn hex_digit(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(***)")
    }
}

/// Which admin password a session started under: a value the keeper of the
/// password derives from what it keeps, which tells two kept passwords apart
/// and is neither the password nor its hash (I4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PasswordFingerprint(u64);

impl PasswordFingerprint {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// One signed-in browser: when it started, when it was last used, the CSRF
/// token its changed requests carry, and the admin password it started under.
#[derive(Clone, Debug)]
pub struct Session {
    csrf: Token,
    password: PasswordFingerprint,
    started: Timestamp,
    last_used: Timestamp,
}

impl Session {
    /// The token a changed request of this session must carry.
    pub fn csrf(&self) -> &Token {
        &self.csrf
    }

    /// Whether `presented` is this session's CSRF token.
    pub fn accepts(&self, presented: &Token) -> bool {
        self.csrf.matches(presented)
    }

    /// When the session ends however much it is used.
    pub fn ends_at(&self) -> Timestamp {
        after(self.started, SESSION_LIFETIME)
    }

    /// Whether the session is live at `now`: used within [`SESSION_IDLE`]
    /// and started within [`SESSION_LIFETIME`].
    pub fn live_at(&self, now: Timestamp) -> bool {
        now < after(self.last_used, SESSION_IDLE) && now < self.ends_at()
    }
}

/// `at + by`, or the last instant there is when that is past it.
fn after(at: Timestamp, by: SignedDuration) -> Timestamp {
    at.checked_add(by).unwrap_or(Timestamp::MAX)
}

/// The live sessions, by the SHA-256 of the token of their cookie. A session
/// that has ended is gone the first time it is looked for, or when one
/// starts. A session lives only under the admin password it started under:
/// once the password is set anew, every session started before ends.
#[derive(Debug, Default)]
pub struct Sessions {
    by_digest: HashMap<Digest, Session>,
}

impl Sessions {
    /// Starts a session for `token` at `now`, under the admin password
    /// `password`. When [`MAX_SESSIONS`] are live, the one used longest ago
    /// ends.
    pub fn start(
        &mut self,
        token: Token,
        csrf: Token,
        password: PasswordFingerprint,
        now: Timestamp,
    ) -> Session {
        self.by_digest
            .retain(|_, session| session.live_at(now) && session.password == password);
        if self.by_digest.len() >= MAX_SESSIONS
            && let Some(oldest) = self
                .by_digest
                .iter()
                .min_by_key(|(_, session)| session.last_used)
                .map(|(digest, _)| *digest)
        {
            self.by_digest.remove(&oldest);
        }
        let session = Session {
            csrf,
            password,
            started: now,
            last_used: now,
        };
        self.by_digest.insert(token.digest(), session.clone());
        session
    }

    /// The session of `token`, used at `now` while the admin password is
    /// `password`: a live one is used once more, and one that has ended, or
    /// that started under another password, is forgotten and is `None`.
    pub fn use_at(
        &mut self,
        token: &Token,
        password: PasswordFingerprint,
        now: Timestamp,
    ) -> Option<&Session> {
        let digest = token.digest();
        let session = self.by_digest.get(&digest)?;
        if !session.live_at(now) || session.password != password {
            self.by_digest.remove(&digest);
            return None;
        }
        let session = self.by_digest.get_mut(&digest)?;
        session.last_used = session.last_used.max(now);
        Some(session)
    }

    /// The session of `token`, if it is live at `now` while the admin
    /// password is `password`, without using it: a stream that stays open
    /// and a read the page sends on its own look at it this way, so an open
    /// tab alone never keeps a session from ending.
    pub fn peek(
        &self,
        token: &Token,
        password: PasswordFingerprint,
        now: Timestamp,
    ) -> Option<&Session> {
        self.by_digest
            .get(&token.digest())
            .filter(|session| session.live_at(now) && session.password == password)
    }

    /// Ends the session of `token`, if there is one.
    pub fn end(&mut self, token: &Token) {
        self.by_digest.remove(&token.digest());
    }

    /// How many sessions are kept.
    pub fn len(&self) -> usize {
        self.by_digest.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_digest.is_empty()
    }
}

/// Who a sign-in came from, as far as its wait goes: an IPv4 address, or
/// the /64 network of an IPv6 address, which one host can fill with
/// addresses of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Client {
    V4(u32),
    V6Network(u64),
}

impl Client {
    pub fn of(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(v4) => Self::V4(u32::from(v4)),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => Self::V4(u32::from(v4)),
                None => Self::V6Network(u64::try_from(u128::from(v6) >> 64).unwrap_or(u64::MAX)),
            },
        }
    }
}

/// What one client's sign-ins have come to.
#[derive(Clone, Copy, Debug)]
struct Attempts {
    /// Sign-ins counted as failed since the last that succeeded.
    failed: u32,
    /// The first moment the next sign-in is checked.
    next: Timestamp,
    /// When the last sign-in was counted.
    last: Timestamp,
}

/// The wait after failed sign-ins (§10.1): each sign-in from a client counts
/// as failed until it succeeds, and the client's next one waits 1 second
/// after the first, twice as long after each one more, and never longer than
/// [`MAX_WAIT`]. A sign-in during the wait is refused unchecked, so a guess
/// then never counts; once the wait has passed, the admin password signs in
/// whatever came before, so nobody is locked out. Counting before the check
/// keeps two sign-ins sent at once from both being checked.
#[derive(Debug, Default)]
pub struct SignInWaits {
    by_client: HashMap<Client, Attempts>,
}

impl SignInWaits {
    /// Counts a sign-in from `client` at `now`, or refuses it with what is
    /// left of the client's wait.
    pub fn attempt(&mut self, client: Client, now: Timestamp) -> Result<(), SignedDuration> {
        let failed = match self.by_client.get(&client) {
            Some(attempts) if now < attempts.next => {
                return Err(attempts.next.duration_since(now));
            }
            Some(attempts) if now < after(attempts.last, FORGET_AFTER) => attempts.failed,
            _ => 0,
        };
        if !self.by_client.contains_key(&client) {
            self.make_room(now);
        }
        let failed = failed.saturating_add(1);
        self.by_client.insert(
            client,
            Attempts {
                failed,
                next: after(now, wait_after(failed)),
                last: now,
            },
        );
        Ok(())
    }

    /// The sign-in from `client` succeeded: its waits start over.
    pub fn succeeded(&mut self, client: Client) {
        self.by_client.remove(&client);
    }

    /// How many clients are remembered.
    pub fn len(&self) -> usize {
        self.by_client.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_client.is_empty()
    }

    /// Makes room for one more client: the forgotten go first, and then, while
    /// the table is full, the client whose wait ends first, so a client that
    /// keeps failing is the last to go, however many others fail once.
    fn make_room(&mut self, now: Timestamp) {
        if self.by_client.len() < MAX_CLIENTS {
            return;
        }
        self.by_client
            .retain(|_, attempts| now < after(attempts.last, FORGET_AFTER));
        while self.by_client.len() >= MAX_CLIENTS {
            let Some(first) = self
                .by_client
                .iter()
                .min_by_key(|(_, attempts)| attempts.next)
                .map(|(client, _)| *client)
            else {
                return;
            };
            self.by_client.remove(&first);
        }
    }
}

/// The wait after `failed` sign-ins in a row: 1 s after the first, doubling,
/// at most [`MAX_WAIT`].
fn wait_after(failed: u32) -> SignedDuration {
    let doublings = failed.saturating_sub(1).min(16);
    SignedDuration::from_secs(1_i64 << doublings).min(MAX_WAIT)
}

#[cfg(test)]
mod tests;
