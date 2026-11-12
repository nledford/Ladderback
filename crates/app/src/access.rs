//! Sign-in (technical design §10.1, decision 70): the use case behind the
//! sign-in form, the session cookie and the CSRF token. It exists only while
//! sign-in is on, which is while an admin password is kept.

use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domain::access::{Client, Password, Sessions, SignInWaits, Token};
use domain::{SignedDuration, Timestamp};

use crate::ports::{AdminPassword, Clock, Random};

/// The admin's sessions and the waits after failed sign-ins, kept in memory:
/// a daemon that restarts asks everyone to sign in again.
pub struct Access {
    password: Arc<dyn AdminPassword>,
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
/// its changed requests carry, and when it ends however it is used.
#[derive(Debug)]
pub struct SignedIn {
    pub token: Token,
    pub csrf: Token,
    pub ends_at: Timestamp,
}

/// A session that is live, as a request found it.
#[derive(Debug)]
pub struct LiveSession {
    csrf: Token,
    ends_in: SignedDuration,
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

    /// How long the session has left however it is used: until its
    /// [`domain::access::SESSION_LIFETIME`] is up.
    pub fn ends_in(&self) -> SignedDuration {
        self.ends_in
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
}

impl Access {
    pub fn new(
        password: Arc<dyn AdminPassword>,
        random: Arc<dyn Random>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            password,
            random,
            clock,
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Signs in from `address` with `attempt`. The attempt counts as failed
    /// before the password is checked, so two sent at once are not both
    /// checked; one that succeeds starts the address's waits over. The
    /// session is tied to the admin password as it was before the check: a
    /// password set during the check then ends the session at its first use,
    /// and never lets one started under the old password outlive it.
    pub async fn sign_in(
        &self,
        address: IpAddr,
        attempt: &Password,
    ) -> Result<SignedIn, SignInRefused> {
        let client = Client::of(address);
        let now = self.clock.now();
        self.lock().waits.attempt(client, now).map_err(|left| {
            let millis = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
            SignInRefused::Wait {
                seconds: millis.div_ceil(1000),
            }
        })?;
        let unavailable = |error| SignInRefused::Unavailable(format!("{error:?}"));
        let under = self.password.fingerprint().map_err(unavailable)?;
        match self.password.verify(attempt).await {
            Ok(true) => {}
            Ok(false) => return Err(SignInRefused::WrongPassword),
            Err(error) => return Err(unavailable(error)),
        }
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
        let session = state
            .sessions
            .start(token.clone(), csrf.clone(), under, now);
        Ok(SignedIn {
            token,
            csrf,
            ends_at: session.ends_at(),
        })
    }

    /// The session of `token`, if it is live, used once more by asking. A
    /// session started under another admin password than the one kept now
    /// has ended, and so has every session while the kept one can't be read.
    pub fn session(&self, token: &Token) -> Option<LiveSession> {
        let under = self.password.fingerprint().ok()?;
        let now = self.clock.now();
        self.lock()
            .sessions
            .use_at(token, under, now)
            .map(|session| LiveSession {
                csrf: session.csrf().clone(),
                ends_in: session.ends_at().duration_since(now),
            })
    }

    /// The session of `token`, if it is live, without using it: the events
    /// stream and the reads a page sends on its own look at it this way, so
    /// only what a person does keeps a session (security §10.1).
    pub fn peek(&self, token: &Token) -> Option<LiveSession> {
        let under = self.password.fingerprint().ok()?;
        let now = self.clock.now();
        self.lock()
            .sessions
            .peek(token, under, now)
            .map(|session| LiveSession {
                csrf: session.csrf().clone(),
                ends_in: session.ends_at().duration_since(now),
            })
    }

    /// Ends the session of `token`.
    pub fn sign_out(&self, token: &Token) {
        self.lock().sessions.end(token);
    }

    /// How many addresses' failed sign-ins are remembered.
    pub fn remembered_addresses(&self) -> usize {
        self.lock().waits.len()
    }
}

#[cfg(test)]
mod tests;
