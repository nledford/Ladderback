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
mod step_up;

pub use admin_password::{
    PasswordChangeRefused, PasswordSet, SetPasswordRefused, set_admin_password,
    set_admin_password_leaving_sessions,
};
pub use api_tokens::{ApiTokenGrant, ApiTokenRefused, ApiTokensRefused, CreatedApiToken};
pub(crate) use step_up::{StepUpRefused, confirm_step_up};

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
    /// Held while the session length is set, so two set at once reach the
    /// store and the sessions in the same order.
    setting_length: futures::lock::Mutex<()>,
    /// Held while a sign-in starts its session and the store keeps it, and
    /// while Sign out everywhere ends every session and the store forgets
    /// them, so the store never keeps a session Sign out everywhere ended. A
    /// sign-in still checking its password takes it after, and its session
    /// lives on.
    adding_or_ending_all: futures::lock::Mutex<()>,
}

#[derive(Default)]
struct State {
    sessions: Sessions,
    waits: SignInWaits,
    /// The sessions forgotten here that the store may still keep: each
    /// stays until a write that told the store returns, so a write that
    /// fails, or a request dropped before it returns, leaves it for the
    /// next write to tell again.
    unkept: Vec<SessionDigest>,
    /// Whether a write whose failure no caller sees has failed since the
    /// last write that succeeded: it was logged, and the next such failure
    /// is logged only once a write has succeeded again.
    store_failing: bool,
}

impl State {
    /// Every session the store is still to forget: the ones no write has
    /// told it of yet, and the ones forgotten since. They stay until
    /// [`Self::forgotten_by_store`].
    fn still_to_forget(&mut self) -> Vec<SessionDigest> {
        for digest in self.sessions.take_forgotten() {
            if !self.unkept.contains(&digest) {
                self.unkept.push(digest);
            }
        }
        self.unkept.clone()
    }

    /// A write that told the store to forget `digests` returned.
    fn forgotten_by_store(&mut self, digests: &[SessionDigest]) {
        self.unkept.retain(|digest| !digests.contains(digest));
    }
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
    secure: bool,
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

    /// Whether its cookie is marked `Secure`: as the sign-in decided, so a
    /// renewal sets it as the sign-in did, whatever the request renewing it
    /// shows.
    pub fn secure(&self) -> bool {
        self.secure
    }

    fn of(session: &domain::access::Session, length: SessionLength, now: Timestamp) -> Self {
        Self {
            csrf: session.csrf().clone(),
            ends_in: session.ends_at(length).map(|end| end.duration_since(now)),
            idle_limit: session.idle_limit(length),
            renewed: None,
            secure: session.secure(),
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

/// A step-up password that was right (§10.4): the address's waits started
/// over, and the person may go on with what it confirms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepUpConfirmed;

/// Why a password entered as the admin password was refused, however it was
/// asked for: at a sign-in, a step-up (§10.4) or a change of password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasswordRefused {
    /// The password is not the admin password: it counted as a failed
    /// sign-in.
    WrongPassword,
    /// The address's wait after failed sign-ins has `seconds` left, rounded
    /// up; the password was not checked.
    Wait { seconds: u64 },
    /// The admin password could not be checked: the platform's own text.
    Unavailable(String),
}

impl From<PasswordRefused> for SignInRefused {
    fn from(refused: PasswordRefused) -> Self {
        match refused {
            PasswordRefused::WrongPassword => Self::WrongPassword,
            PasswordRefused::Wait { seconds } => Self::Wait { seconds },
            PasswordRefused::Unavailable(text) => Self::Unavailable(text),
        }
    }
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
            setting_length: futures::lock::Mutex::new(()),
            adding_or_ending_all: futures::lock::Mutex::new(()),
        }
    }

    /// Brings back the sessions the store kept from before the daemon
    /// started, each with a new CSRF token; the ones that have ended since,
    /// or that started under another admin password, are forgotten, there
    /// too, and so are the ones the store can't read. The session length is
    /// the one kept, as soon as it is read, whatever fails after; one that
    /// can't be read is Until I close the browser, the shortest, for this
    /// run. On an error no session is brought back and the store keeps
    /// them all.
    pub async fn restore(&self) -> Result<Restored, RestoreFailed> {
        let length = match self.sessions.length().await {
            Ok(length) => length.unwrap_or_default(),
            Err(error) => {
                self.measure_by(SessionLength::UntilBrowserCloses);
                return Err(RestoreFailed::Store(error));
            }
        };
        self.measure_by(length);
        let under = self
            .password
            .fingerprint()
            .map_err(RestoreFailed::Password)?;
        let unreadable = self
            .sessions
            .forget_unreadable()
            .await
            .map_err(RestoreFailed::Store)?;
        let kept = self.sessions.all().await.map_err(RestoreFailed::Store)?;
        let csrfs = kept
            .iter()
            .map(|_| self.random.token().map(Token::from_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(RestoreFailed::Random)?;
        let now = self.clock.now();
        let (sessions, forget) = {
            let mut state = self.lock();
            for (session, csrf) in kept.into_iter().zip(csrfs) {
                state.sessions.restore(session, csrf, under, now);
            }
            (state.sessions.len(), state.still_to_forget())
        };
        // One the store still keeps after this has ended all the same, and
        // is forgotten again at the next start.
        if let Err(error) = self.forget(forget).await {
            self.tried_again_later(&error);
        }
        Ok(Restored {
            sessions,
            unreadable,
        })
    }

    /// Measures the sessions by `length` from now on, for this run only.
    fn measure_by(&self, length: SessionLength) {
        let now = self.clock.now();
        self.lock().sessions.set_length(length, now);
    }

    /// Tells the store to forget the sessions of `digests`; until it has,
    /// they wait for the next write.
    async fn forget(&self, digests: Vec<SessionDigest>) -> Result<(), StoreError> {
        if digests.is_empty() {
            return Ok(());
        }
        self.wrote(self.sessions.remove(&digests).await)?;
        self.lock().forgotten_by_store(&digests);
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Passes on how a write to the store went; one that succeeded means
    /// the store writes again, so the next failure no caller sees is logged.
    fn wrote(&self, written: Result<(), StoreError>) -> Result<(), StoreError> {
        if written.is_ok() {
            self.lock().store_failing = false;
        }
        written
    }

    /// A write that failed with `error` where no caller sees it: it is
    /// tried again at the next write. Logged at its first failure since a
    /// write succeeded, so a store that keeps failing (a full disk, say) is
    /// said once rather than at every write. The line holds the error's
    /// code alone: its message is the store's own words, which nobody has
    /// checked for a value (I4).
    fn tried_again_later(&self, error: &StoreError) {
        let first = !std::mem::replace(&mut self.lock().store_failing, true);
        if first {
            log::warn!(
                "the database could not keep a change to the sessions ({}); it is tried again at the next write, and further failures of this kind are not logged until a write succeeds",
                error.code()
            );
        }
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
    ) -> Result<PasswordFingerprint, PasswordRefused> {
        let now = self.clock.now();
        self.lock()
            .waits
            .attempt(client, now)
            .map_err(|left| PasswordRefused::Wait {
                seconds: api_tokens::seconds_left(left),
            })?;
        let unavailable = |error| PasswordRefused::Unavailable(format!("{error:?}"));
        let under = self.password.fingerprint().map_err(unavailable)?;
        match self.password.verify(attempt).await {
            Ok(true) => Ok(under),
            Ok(false) => Err(PasswordRefused::WrongPassword),
            Err(error) => Err(unavailable(error)),
        }
    }

    /// Signs in from `address` with `attempt`, with no idle limit when
    /// `keep_signed_in` and the session length offers Keep me signed in on
    /// this device, and its cookie marked `Secure` when `secure`, at every
    /// renewal too. The attempt counts as failed before the password is
    /// checked, so two sent at once are not both checked; once the password
    /// is right and the session's tokens drawn, the address's waits start
    /// over, even when the store can't keep the session. The session is tied
    /// to the admin password as it was before the check: a password set
    /// during the check then ends the session at its first use, and never
    /// lets one started under the old password outlive it.
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
        let _adding = self.adding_or_ending_all.lock().await;
        let (kept, forget, ends_in, idle_limit, cookie) = {
            let mut state = self.lock();
            // The password was right: no guess, whatever the store answers.
            state.waits.succeeded(client);
            let length = state.sessions.length();
            let session = state.sessions.start(
                token.clone(),
                csrf.clone(),
                under,
                keep_signed_in,
                secure,
                now,
            );
            (
                session.kept(token.digest()),
                state.still_to_forget(),
                session.ends_at(length).map(|end| end.duration_since(now)),
                session.idle_limit(length),
                session.cookie_at(now, length),
            )
        };
        if let Err(error) = self.wrote(self.sessions.add(&kept, &forget).await) {
            // Ended here, and forgotten by the store at its next write, in
            // case the write was kept after all.
            self.lock().sessions.end(&token);
            return Err(SignInRefused::Store(error));
        }
        self.lock().forgotten_by_store(&forget);
        Ok(SignedIn {
            token,
            csrf,
            ends_in,
            idle_limit,
            cookie,
        })
    }

    /// Checks `attempt`, the admin password entered again before a restore
    /// or a download (§10.4), from `address`, in the same waits a sign-in
    /// counts against: a wrong one makes the address's next sign-in wait, as
    /// a wrong sign-in does, and a right one starts its waits over. No
    /// session is started or used.
    pub async fn step_up(
        &self,
        address: IpAddr,
        attempt: &Password,
    ) -> Result<StepUpConfirmed, PasswordRefused> {
        let client = Client::of(address);
        self.check(client, attempt).await?;
        self.lock().waits.succeeded(client);
        Ok(StepUpConfirmed)
    }

    /// The session of `token`, if it is live, used once more by asking,
    /// with the cookie the use renewed, if one was due. A session started
    /// under another admin password than the one kept now has ended, and so
    /// has every session while the kept one can't be read.
    pub async fn session(&self, token: &Token) -> Option<LiveSession> {
        let under = self.password.fingerprint().ok()?;
        let now = self.clock.now();
        let (live, used, forget) = {
            let mut state = self.lock();
            let length = state.sessions.length();
            let live = state
                .sessions
                .use_at(token, under, now)
                .map(|session| LiveSession::of(session, length, now));
            let live = live.map(|live| LiveSession {
                renewed: state.sessions.renew_cookie(token, now),
                ..live
            });
            let used = state.sessions.use_to_keep(token);
            (live, used, state.still_to_forget())
        };
        // A write that fails here is tried again at the next one.
        if let Err(error) = self.forget(forget).await {
            self.tried_again_later(&error);
        }
        if let Some((digest, at)) = used {
            // A last use the store misses can only end the session sooner
            // after a restart, never keep it longer.
            if let Err(error) = self.wrote(self.sessions.record_use(&digest, at).await) {
                self.tried_again_later(&error);
            }
        }
        live
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
    /// error says the store still holds it, and is told again at its next
    /// write.
    pub async fn sign_out(&self, token: &Token) -> Result<(), StoreError> {
        let forget = {
            let mut state = self.lock();
            state.sessions.end(token);
            state.still_to_forget()
        };
        self.forget(forget).await
    }

    /// Sign out everywhere: ends every session, in memory and then in the
    /// store, which forgets every one it keeps. API tokens are left alone.
    /// An error says the store still holds them, and is told again at its
    /// next write. A sign-in already starting its session finishes first,
    /// so the store never keeps its session once this has ended it; one
    /// still checking its password starts its session after.
    pub async fn sign_out_everywhere(&self) -> Result<(), StoreError> {
        let _ending = self.adding_or_ending_all.lock().await;
        let forget = {
            let mut state = self.lock();
            state.sessions.end_all();
            state.still_to_forget()
        };
        self.wrote(self.sessions.remove_all().await)?;
        self.lock().forgotten_by_store(&forget);
        Ok(())
    }

    /// How many addresses' failed sign-ins are remembered.
    pub fn remembered_addresses(&self) -> usize {
        self.lock().waits.len()
    }

    /// The session length every session is measured by.
    pub fn session_length(&self) -> SessionLength {
        self.lock().sessions.length()
    }

    /// Whether the sign-in form offers Keep me signed in on this device.
    pub fn offers_keep_signed_in(&self) -> bool {
        self.session_length().offers_keep_signed_in()
    }

    /// Sets the session length: the store keeps it first, and an error
    /// changes nothing. Then every session is measured by it, the ones open
    /// included, and a session it ends is forgotten at once.
    pub async fn set_session_length(&self, length: SessionLength) -> Result<(), StoreError> {
        let _setting = self.setting_length.lock().await;
        self.wrote(self.sessions.set_length(length).await)?;
        let now = self.clock.now();
        let forget = {
            let mut state = self.lock();
            state.sessions.set_length(length, now);
            state.still_to_forget()
        };
        // A write that fails here is tried again at the next one.
        if let Err(error) = self.forget(forget).await {
            self.tried_again_later(&error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
