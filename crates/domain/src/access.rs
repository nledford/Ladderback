//! Sign-in (technical design §10.1, decision 70): the admin password as typed,
//! the admin's sessions, the token a changed request carries, and the wait
//! after failed sign-ins. Rules only: the time, the random bytes and the
//! client's address are passed in.

use std::collections::HashMap;
use std::net::IpAddr;

use jiff::{SignedDuration, Timestamp};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

pub mod api_token;

/// A session with an idle limit ends once it has gone this long unused. A
/// session signed in with **Keep me signed in on this device** has none,
/// unless the session length is Until I close the browser.
pub const SESSION_IDLE: SignedDuration = SignedDuration::from_hours(12);

/// The longest a browser keeps a cookie: it caps a `Max-Age` at 400 days, so
/// a session that lasts longer has its cookie renewed by its use.
pub const COOKIE_MOST: SignedDuration = SignedDuration::from_hours(400 * 24);

/// How often a person's use renews the session's cookie at most, unless the
/// session now ends before that cookie would.
pub const COOKIE_RENEWED_EVERY: SignedDuration = SignedDuration::from_hours(24);

/// How long a session's cookie lasts, as its sign-in or a renewal sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookieLife {
    /// No `Max-Age`: the browser drops the cookie when it closes.
    UntilBrowserCloses,
    /// A `Max-Age` of this long.
    Lasts(SignedDuration),
}

/// The session length: how long every session lasts after its sign-in, set
/// in Settings as **Stay signed in for** (§3.1, §10.1). It applies to the
/// sessions already open, measured from each one's sign-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionLength {
    /// No end after sign-in: the browser drops the cookie when it closes,
    /// and every session has an idle limit.
    UntilBrowserCloses,
    SevenDays,
    #[default]
    ThirtyDays,
    NinetyDays,
    OneYear,
    /// No end after sign-in: a session ends at sign-out, at a new admin
    /// password, or unused past its idle limit.
    UntilSignOut,
}

impl SessionLength {
    /// Every session length, shortest first.
    pub const ALL: [Self; 6] = [
        Self::UntilBrowserCloses,
        Self::SevenDays,
        Self::ThirtyDays,
        Self::NinetyDays,
        Self::OneYear,
        Self::UntilSignOut,
    ];

    /// How long a session lasts after its sign-in, or `None` when nothing
    /// but its idle limit, a sign-out or a new admin password ends it.
    pub fn after_sign_in(self) -> Option<SignedDuration> {
        let days = match self {
            Self::UntilBrowserCloses | Self::UntilSignOut => return None,
            Self::SevenDays => 7,
            Self::ThirtyDays => 30,
            Self::NinetyDays => 90,
            Self::OneYear => 365,
        };
        Some(SignedDuration::from_hours(days * 24))
    }

    /// Its name on the wire and in the store.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UntilBrowserCloses => "until-browser-closes",
            Self::SevenDays => "7-days",
            Self::ThirtyDays => "30-days",
            Self::NinetyDays => "90-days",
            Self::OneYear => "1-year",
            Self::UntilSignOut => "until-sign-out",
        }
    }

    /// The session length named `text`, as [`Self::as_str`] writes it.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|length| length.as_str() == text)
    }

    /// Whether the sign-in form offers **Keep me signed in on this device**:
    /// not under Until I close the browser, which gives every session an
    /// idle limit.
    pub fn offers_keep_signed_in(self) -> bool {
        self != Self::UntilBrowserCloses
    }
}

/// The most sessions kept at once. Only the admin password starts one, so
/// the cap only bounds what a person who has it can make the daemon keep;
/// the session used longest ago makes room for a new one.
pub const MAX_SESSIONS: usize = 16;

/// How often a session's last use is written to the store at most: a person
/// using the page doesn't write on every request, and a restart loses at most
/// this much of a session's idle time.
pub const SESSION_USE_KEPT_EVERY: SignedDuration = SignedDuration::from_mins(1);

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

    /// The SHA-256 of the token, which the sessions are kept by, in memory
    /// and in the store: a lookup compares digests, and a digest's matching
    /// prefix says nothing of the token's.
    pub fn digest(&self) -> SessionDigest {
        SessionDigest(Sha256::digest(self.0).into())
    }
}

/// A session token's SHA-256: what the sessions are kept by, in memory and in
/// the store, which never holds the token itself (I4).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionDigest([u8; 32]);

impl SessionDigest {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for SessionDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionDigest(..)")
    }
}

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

    /// The value, as a store keeps it beside an API token.
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// One signed-in browser: when it started, when it was last used, the CSRF
/// token its changed requests carry, the admin password it started under,
/// whether it was signed in with Keep me signed in on this device, whether
/// its cookie is marked `Secure`, and the cookie last set for it.
#[derive(Clone, Debug)]
pub struct Session {
    csrf: Token,
    password: PasswordFingerprint,
    started: Timestamp,
    last_used: Timestamp,
    /// The last use the store holds.
    kept_use: Timestamp,
    keep_signed_in: bool,
    /// Decided at sign-in, as the request that signed in showed it, and
    /// kept for every renewal: the requests that renew a cookie may not
    /// show it.
    secure: bool,
    /// `None` until one is set after a restart.
    cookie: Option<CookieSet>,
}

/// A cookie set for a session: when, under which session length, and when
/// it ends; `None` when the browser drops it as it closes.
#[derive(Clone, Copy, Debug)]
struct CookieSet {
    at: Timestamp,
    length: SessionLength,
    ends: Option<Timestamp>,
}

impl CookieSet {
    /// The cookie set at `at` under `length` that lasts `life`.
    fn of(at: Timestamp, length: SessionLength, life: CookieLife) -> Self {
        let ends = match life {
            CookieLife::UntilBrowserCloses => None,
            CookieLife::Lasts(lasts) => Some(after(at, lasts)),
        };
        Self { at, length, ends }
    }
}

/// A session as the store keeps it, so it outlives a restart: its token's
/// digest, never the token, and no CSRF token, which a restored session draws
/// anew; when it started and was last used, the admin password it started
/// under, whether it was signed in with Keep me signed in on this device, and
/// whether its cookie is marked `Secure`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptSession {
    pub digest: SessionDigest,
    pub password: PasswordFingerprint,
    pub started: Timestamp,
    pub last_used: Timestamp,
    pub keep_signed_in: bool,
    pub secure: bool,
}

impl Session {
    /// Whether its cookie is marked `Secure`, at sign-in and at every
    /// renewal alike.
    pub fn secure(&self) -> bool {
        self.secure
    }

    /// The token a changed request of this session must carry.
    pub fn csrf(&self) -> &Token {
        &self.csrf
    }

    /// Whether `presented` is this session's CSRF token.
    pub fn accepts(&self, presented: &Token) -> bool {
        self.csrf.matches(presented)
    }

    /// When the session ends however much it is used, under the session
    /// length `length`; `None` when nothing but its idle limit, a sign-out
    /// or a new admin password ends it.
    pub fn ends_at(&self, length: SessionLength) -> Option<Timestamp> {
        length
            .after_sign_in()
            .map(|lasts| after(self.started, lasts))
    }

    /// How long the session may go unused under the session length
    /// `length`: [`SESSION_IDLE`]; `None` for a session signed in with Keep
    /// me signed in on this device, unless `length` is Until I close the
    /// browser.
    pub fn idle_limit(&self, length: SessionLength) -> Option<SignedDuration> {
        (!self.keep_signed_in || !length.offers_keep_signed_in()).then_some(SESSION_IDLE)
    }

    /// Whether the session is live at `now` under the session length
    /// `length`: used within its idle limit, if it has one, and short of its
    /// end after sign-in.
    pub fn live_at(&self, now: Timestamp, length: SessionLength) -> bool {
        self.idle_limit(length)
            .is_none_or(|idle| now < after(self.last_used, idle))
            && self.ends_at(length).is_none_or(|end| now < end)
    }

    /// The cookie a sign-in or a renewal at `now` sets under the session
    /// length `length`: under Until I close the browser one the browser
    /// drops as it closes; otherwise one that lasts until the session ends,
    /// and at most [`COOKIE_MOST`].
    pub fn cookie_at(&self, now: Timestamp, length: SessionLength) -> CookieLife {
        if length == SessionLength::UntilBrowserCloses {
            return CookieLife::UntilBrowserCloses;
        }
        let left = self.ends_at(length).map_or(COOKIE_MOST, |end| {
            end.duration_since(now)
                .clamp(SignedDuration::ZERO, COOKIE_MOST)
        });
        CookieLife::Lasts(left)
    }

    /// The session as the store keeps it, under its token's `digest`.
    pub fn kept(&self, digest: SessionDigest) -> KeptSession {
        KeptSession {
            digest,
            password: self.password,
            started: self.started,
            last_used: self.last_used,
            keep_signed_in: self.keep_signed_in,
            secure: self.secure,
        }
    }
}

/// `at + by`, or the last instant there is when that is past it.
fn after(at: Timestamp, by: SignedDuration) -> Timestamp {
    at.checked_add(by).unwrap_or(Timestamp::MAX)
}

/// The live sessions, by the SHA-256 of the token of their cookie. A session
/// that has ended is gone the first time it is looked for, or when one
/// starts. A session lives only under the admin password it started under:
/// once the password is set anew, every session started before ends. The
/// digests of the sessions forgotten wait in `forgotten` until the store is
/// told ([`Self::take_forgotten`]).
#[derive(Debug, Default)]
pub struct Sessions {
    by_digest: HashMap<SessionDigest, Session>,
    forgotten: Vec<SessionDigest>,
    length: SessionLength,
}

impl Sessions {
    /// Starts a session for `token` at `now`, under the admin password
    /// `password`, with no idle limit when `keep_signed_in` and the session
    /// length offers it, and its cookie marked `Secure` when `secure`. When
    /// [`MAX_SESSIONS`] are live, the one used longest ago ends.
    pub fn start(
        &mut self,
        token: Token,
        csrf: Token,
        password: PasswordFingerprint,
        keep_signed_in: bool,
        secure: bool,
        now: Timestamp,
    ) -> Session {
        let ended: Vec<SessionDigest> = self
            .by_digest
            .iter()
            .filter(|(_, session)| !self.lives(session, password, now))
            .map(|(digest, _)| *digest)
            .collect();
        for digest in ended {
            self.forget(digest);
        }
        if let Some(oldest) = self.oldest_when_full() {
            self.forget(oldest);
        }
        let mut session = Session {
            csrf,
            password,
            started: now,
            last_used: now,
            kept_use: now,
            keep_signed_in: keep_signed_in && self.length.offers_keep_signed_in(),
            secure,
            cookie: None,
        };
        session.cookie = Some(CookieSet::of(
            now,
            self.length,
            session.cookie_at(now, self.length),
        ));
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
        if !self.lives(session, password, now) {
            self.forget(digest);
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
            .filter(|session| self.lives(session, password, now))
    }

    /// Whether `session` is live at `now` under the session length while
    /// the admin password is `password`.
    fn lives(&self, session: &Session, password: PasswordFingerprint, now: Timestamp) -> bool {
        session.live_at(now, self.length) && session.password == password
    }

    /// Ends the session of `token`, if there is one.
    pub fn end(&mut self, token: &Token) {
        self.forget(token.digest());
    }

    /// Ends every session: Sign out everywhere. Each is forgotten, for the
    /// store to forget too.
    pub fn end_all(&mut self) {
        let all: Vec<SessionDigest> = self.by_digest.keys().copied().collect();
        for digest in all {
            self.forget(digest);
        }
    }

    /// Forgets the session of `digest`, for the store to forget too.
    fn forget(&mut self, digest: SessionDigest) {
        if self.by_digest.remove(&digest).is_some() {
            self.forgotten.push(digest);
        }
    }

    /// The session used longest ago, when [`MAX_SESSIONS`] are kept.
    fn oldest_when_full(&self) -> Option<SessionDigest> {
        if self.by_digest.len() < MAX_SESSIONS {
            return None;
        }
        self.by_digest
            .iter()
            .min_by_key(|(_, session)| session.last_used)
            .map(|(digest, _)| *digest)
    }

    /// How many sessions are kept.
    pub fn len(&self) -> usize {
        self.by_digest.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_digest.is_empty()
    }

    /// The session length every session is measured by.
    pub fn length(&self) -> SessionLength {
        self.length
    }

    /// Measures every session by `length` from now on, the ones open
    /// included, from each one's sign-in: a session it ends by `now` is
    /// forgotten at once, for the store to forget too.
    pub fn set_length(&mut self, length: SessionLength, now: Timestamp) {
        self.length = length;
        let ended: Vec<SessionDigest> = self
            .by_digest
            .iter()
            .filter(|(_, session)| !session.live_at(now, length))
            .map(|(digest, _)| *digest)
            .collect();
        for digest in ended {
            self.forget(digest);
        }
    }

    /// Brings back a session the store kept, with `csrf` as its new CSRF
    /// token, when it is still live at `now` under the session length and
    /// the admin password `password`; one that has ended, or started under
    /// another password, is forgotten instead. So is one started or last
    /// used after `now`, as a clock set back leaves: counted from `now`, it
    /// would start again, or be used again, at every restart and outlive
    /// its length or its idle limit. Past [`MAX_SESSIONS`], the one used
    /// longest ago ends, which may be this one. Whether it was brought back.
    pub fn restore(
        &mut self,
        kept: KeptSession,
        csrf: Token,
        password: PasswordFingerprint,
        now: Timestamp,
    ) -> bool {
        let session = Session {
            csrf,
            password: kept.password,
            started: kept.started,
            last_used: kept.last_used,
            kept_use: kept.last_used,
            keep_signed_in: kept.keep_signed_in,
            secure: kept.secure,
            cookie: None,
        };
        let dated_ahead = kept.started > now || kept.last_used > now;
        if dated_ahead || !self.lives(&session, password, now) {
            self.forgotten.push(kept.digest);
            return false;
        }
        if let Some(oldest) = self.oldest_when_full() {
            let older = self
                .by_digest
                .get(&oldest)
                .is_some_and(|other| session.last_used < other.last_used);
            if older {
                self.forgotten.push(kept.digest);
                return false;
            }
            self.forget(oldest);
        }
        self.by_digest.insert(kept.digest, session);
        true
    }

    /// The digests of the sessions forgotten since this was last asked, for
    /// the store to forget too.
    pub fn take_forgotten(&mut self) -> Vec<SessionDigest> {
        std::mem::take(&mut self.forgotten)
    }

    /// The last use of the session of `token` for the store to keep, once it
    /// is [`SESSION_USE_KEPT_EVERY`] past the one the store holds; `None`
    /// before then, or for no session.
    pub fn use_to_keep(&mut self, token: &Token) -> Option<(SessionDigest, Timestamp)> {
        let digest = token.digest();
        let session = self.by_digest.get_mut(&digest)?;
        if session.last_used < after(session.kept_use, SESSION_USE_KEPT_EVERY) {
            return None;
        }
        session.kept_use = session.last_used;
        Some((digest, session.last_used))
    }

    /// The cookie a person's use of the session of `token` at `now` renews,
    /// if it is due: a day after the one set last, or at once when the
    /// session length has changed since that one was set, or no cookie has
    /// been set since a restart. `None` when no renewal is due, or for no
    /// session.
    pub fn renew_cookie(&mut self, token: &Token, now: Timestamp) -> Option<CookieLife> {
        let length = self.length;
        let session = self.by_digest.get_mut(&token.digest())?;
        let due = session.cookie.is_none_or(|set| {
            set.length != length || set.ends.is_some() && now >= after(set.at, COOKIE_RENEWED_EVERY)
        });
        if !due {
            return None;
        }
        let fresh = session.cookie_at(now, length);
        session.cookie = Some(CookieSet::of(now, length, fresh));
        Some(fresh)
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

    /// What is left of `client`'s wait at `now`, if it is waiting: a
    /// sign-in or an API token from it now is refused unchecked.
    pub fn waiting(&self, client: Client, now: Timestamp) -> Option<SignedDuration> {
        self.by_client
            .get(&client)
            .filter(|attempts| now < attempts.next)
            .map(|attempts| attempts.next.duration_since(now))
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
