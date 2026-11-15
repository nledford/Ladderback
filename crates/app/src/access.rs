//! Sign-in (technical design §10.1, decision 70): the use case behind the
//! sign-in form, the session cookie and the CSRF token. It exists only while
//! sign-in is on, which is while an admin password is kept.

use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domain::access::{
    Client, CookieLife, Password, PasswordFingerprint, SessionDigest, SessionLength, Sessions,
    SignInWaits, Token,
};
use domain::{SignedDuration, Timestamp};

use crate::ports::{
    AdminPassword, AdminPasswordError, ApiTokenStore, Clock, Random, RandomUnavailable,
    SessionStore, StoreError,
};

mod admin_password;
mod api_tokens;

pub use admin_password::{
    PasswordChangeRefused, PasswordSet, SetPasswordRefused, set_admin_password,
    set_admin_password_leaving_sessions,
};
pub use api_tokens::{ApiTokenGrant, ApiTokenRefused, ApiTokensRefused, CreatedApiToken};

/// The admin's sessions and the waits after failed sign-ins. The sessions
/// are kept in memory, which decides while the daemon runs, and written
/// through to `sessions`, so they outlive a restart ([`Self::restore`]); the
/// waits are kept in memory only. The API tokens are kept in `tokens`.
pub struct Access {
    password: Arc<dyn AdminPassword>,
    tokens: Arc<dyn ApiTokenStore>,
    sessions: Arc<dyn SessionStore>,
    random: Arc<dyn Random>,
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    sessions: Sessions,
    waits: SignInWaits,
}

/// A session that has just started: the token of its cookie, the CSRF token
/// its changed requests carry, when it ends however it is used, how long it
/// may go unused, and how long its cookie lasts.
#[derive(Debug)]
pub struct SignedIn {
    pub token: Token,
    pub csrf: Token,
    /// How long it lasts however it is used; `None` when nothing but its
    /// idle limit, a sign-out or a new admin password ends it.
    pub ends_in: Option<SignedDuration>,
    /// `None` when it was signed in with Keep me signed in on this device.
    pub idle_limit: Option<SignedDuration>,
    pub cookie: CookieLife,
}

/// A session that is live, as a request found it.
#[derive(Debug)]
pub struct LiveSession {
    csrf: Token,
    ends_in: Option<SignedDuration>,
    idle_limit: Option<SignedDuration>,
    renewed: Option<CookieLife>,
}

impl LiveSession {
    /// The token its changed requests carry.
    pub fn csrf(&self) -> &Token {
        &self.csrf
    }

    /// Whether `presented` is that token.
    pub fn accepts(&self, presented: &Token) -> bool {
        self.csrf.matches(presented)
    }

    /// How long the session has left however it is used: until the end of
    /// the session length after its sign-in; `None` when nothing but its
    /// idle limit, a sign-out or a new admin password ends it.
    pub fn ends_in(&self) -> Option<SignedDuration> {
        self.ends_in
    }

    /// How long the session may go unused; `None` when it was signed in
    /// with Keep me signed in on this device and the session length offers
    /// that.
    pub fn idle_limit(&self) -> Option<SignedDuration> {
        self.idle_limit
    }

    /// The cookie this use renewed, for the answer to set; `None` when no
    /// renewal was due, and always for a look that was no use.
    pub fn renewed_cookie(&self) -> Option<CookieLife> {
        self.renewed
    }

    /// Whether its cookie is marked `Secure`, as at its sign-in.
    pub fn secure(&self) -> bool {
        todo!()
    }

    fn of(session: &domain::access::Session, length: SessionLength, now: Timestamp) -> Self {
        Self {
            csrf: session.csrf().clone(),
            ends_in: session.ends_at(length).map(|end| end.duration_since(now)),
            idle_limit: session.idle_limit(length),
            renewed: None,
        }
    }
}

/// Why a sign-in started no session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInRefused {
    /// The password is not the admin password.
    WrongPassword,
    /// The address's wait after failed sign-ins has `seconds` left, rounded
    /// up; the password was not checked.
    Wait { seconds: u64 },
    /// The admin password could not be checked, or no token drawn: the text
    /// is the platform's own message.
    Unavailable(String),
    /// The password was right, but the store could not keep the session.
    Store(StoreError),
}

/// With sign-in off, an admin password `ladderback set-password` has kept
/// since the daemon started (§10.1, decision 70). Sign-in is decided at
/// start, so the daemon says this and never turns sign-in on by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordWaiting {
    /// A hash the daemon can check: the next start turns sign-in on.
    Set,
    /// A file the daemon can't read or check: the next start refuses, as it
    /// does for such a file, until the password is set anew.
    Unreadable,
}

impl PasswordWaiting {
    /// What `kept` holds now; `None` while it keeps no admin password.
    pub fn of(kept: &dyn AdminPassword) -> Option<Self> {
        match kept.fingerprint() {
            Ok(_) => Some(Self::Set),
            Err(AdminPasswordError::NotSet) => None,
            Err(AdminPasswordError::Unreadable(_) | AdminPasswordError::Unwritable(_)) => {
                Some(Self::Unreadable)
            }
        }
    }
}

/// What [`Access::restore`] did: how many sessions it brought back, and how
/// many the store kept that couldn't be read and were forgotten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restored {
    pub sessions: usize,
    pub unreadable: usize,
}

/// Why the sessions kept from before the start were not brought back: the
/// daemon starts with none, and the store keeps them.
#[derive(Debug)]
pub enum RestoreFailed {
    /// The store couldn't be read.
    Store(StoreError),
    /// No CSRF token could be drawn for them.
    Random(RandomUnavailable),
    /// The admin password they were started under couldn't be compared with
    /// the one kept now.
    Password(AdminPasswordError),
}

impl Access {
    pub fn new(
        password: Arc<dyn AdminPassword>,
        random: Arc<dyn Random>,
        clock: Arc<dyn Clock>,
        tokens: Arc<dyn ApiTokenStore>,
        sessions: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            password,
            tokens,
            sessions,
            random,
            clock,
            state: Mutex::new(State::default()),
        }
    }

    /// Brings back the sessions the store kept from before the daemon
    /// started, each with a new CSRF token; the ones that have ended since,
    /// or that started under another admin password, are forgotten, there
    /// too, and so are the ones the store can't read.
    pub async fn restore(&self) -> Result<Restored, RestoreFailed> {
        let _ = &self.sessions;
        todo!()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Checks `attempt` against the admin password for `client`, in its
    /// waits: the attempt counts as failed before it is checked, so two sent
    /// at once are not both checked, and during a wait it is refused
    /// unchecked. `Ok` is the admin password as it was before the check;
    /// the caller starts the client's waits over once it has done what the
    /// password was for.
    async fn check(
        &self,
        client: Client,
        attempt: &Password,
    ) -> Result<PasswordFingerprint, SignInRefused> {
        let now = self.clock.now();
        self.lock()
            .waits
            .attempt(client, now)
            .map_err(|left| SignInRefused::Wait {
                seconds: api_tokens::seconds_left(left),
            })?;
        let unavailable = |error| SignInRefused::Unavailable(format!("{error:?}"));
        let under = self.password.fingerprint().map_err(unavailable)?;
        match self.password.verify(attempt).await {
            Ok(true) => Ok(under),
            Ok(false) => Err(SignInRefused::WrongPassword),
            Err(error) => Err(unavailable(error)),
        }
    }

    /// Signs in from `address` with `attempt`, with no idle limit when
    /// `keep_signed_in` and the session length offers Keep me signed in on
    /// this device. The attempt counts as failed before the password is
    /// checked, so two sent at once are not both checked; one that succeeds
    /// starts the address's waits over. The session is tied to the admin
    /// password as it was before the check: a password set during the check
    /// then ends the session at its first use, and never lets one started
    /// under the old password outlive it.
    pub async fn sign_in(
        &self,
        address: IpAddr,
        attempt: &Password,
        keep_signed_in: bool,
        secure: bool,
    ) -> Result<SignedIn, SignInRefused> {
        let client = Client::of(address);
        let under = self.check(client, attempt).await?;
        let draw = || {
            self.random
                .token()
                .map(Token::from_bytes)
                .map_err(|error| SignInRefused::Unavailable(error.0))
        };
        let (token, csrf) = (draw()?, draw()?);
        let now = self.clock.now();
        let mut state = self.lock();
        state.waits.succeeded(client);
        let length = state.sessions.length();
        let session = state
            .sessions
            .start(token.clone(), csrf.clone(), under, keep_signed_in, secure, now);
        Ok(SignedIn {
            token,
            csrf,
            ends_in: session.ends_at(length).map(|end| end.duration_since(now)),
            idle_limit: session.idle_limit(length),
            cookie: session.cookie_at(now, length),
        })
    }

    /// The session of `token`, if it is live, used once more by asking,
    /// with the cookie the use renewed, if one was due. A session started
    /// under another admin password than the one kept now has ended, and so
    /// has every session while the kept one can't be read.
    pub async fn session(&self, token: &Token) -> Option<LiveSession> {
        let under = self.password.fingerprint().ok()?;
        let now = self.clock.now();
        let mut state = self.lock();
        let length = state.sessions.length();
        state
            .sessions
            .use_at(token, under, now)
            .map(|session| LiveSession::of(session, length, now))
    }

    /// The session of `token`, if it is live, without using it: the events
    /// stream and the reads a page sends on its own look at it this way, so
    /// only what a person does keeps a session (security §10.1).
    pub fn peek(&self, token: &Token) -> Option<LiveSession> {
        let under = self.password.fingerprint().ok()?;
        let now = self.clock.now();
        let state = self.lock();
        let length = state.sessions.length();
        state
            .sessions
            .peek(token, under, now)
            .map(|session| LiveSession::of(session, length, now))
    }

    /// Ends the session of `token`, in memory and then in the store; an
    /// error says the store still holds it.
    pub async fn sign_out(&self, token: &Token) -> Result<(), StoreError> {
        self.lock().sessions.end(token);
        Ok(())
    }

    /// Sign out everywhere: ends every session, in memory and then in the
    /// store, which forgets every one it keeps. API tokens are left alone.
    /// An error says the store still holds them, and is told again at its
    /// next write.
    pub async fn sign_out_everywhere(&self) -> Result<(), StoreError> {
        todo!()
    }

    /// How many addresses' failed sign-ins are remembered.
    pub fn remembered_addresses(&self) -> usize {
        self.lock().waits.len()
    }

    /// The session length every session is measured by.
    pub fn session_length(&self) -> SessionLength {
        todo!()
    }

    /// Whether the sign-in form offers Keep me signed in on this device.
    pub fn offers_keep_signed_in(&self) -> bool {
        self.session_length().offers_keep_signed_in()
    }

    /// Sets the session length: the store keeps it first, and an error
    /// changes nothing. Then every session is measured by it, the ones open
    /// included, and a session it ends is forgotten at once.
    pub async fn set_session_length(&self, length: SessionLength) -> Result<(), StoreError> {
        let _ = length;
        todo!()
    }
}

#[cfg(test)]
mod tests;
