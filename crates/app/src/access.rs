//! Sign-in (technical design §10.1, decision 70): the use case behind the
//! sign-in form, the session cookie and the CSRF token. It exists only while
//! sign-in is on, which is while an admin password is kept.

use std::net::IpAddr;
use std::sync::Arc;

use domain::access::{Password, Token};
use domain::{SignedDuration, Timestamp};

use crate::ports::{AdminPassword, Clock, Random};

/// The admin's sessions and the waits after failed sign-ins, kept in memory.
pub struct Access {
    _password: Arc<dyn AdminPassword>,
    _random: Arc<dyn Random>,
    _clock: Arc<dyn Clock>,
}

/// A session that has just started.
#[derive(Debug)]
pub struct SignedIn {
    pub token: Token,
    pub csrf: Token,
    pub ends_at: Timestamp,
}

/// A session that is live, as a request found it.
#[derive(Debug)]
pub struct LiveSession {
    _csrf: Token,
}

impl LiveSession {
    pub fn csrf(&self) -> &Token {
        todo!()
    }

    pub fn accepts(&self, _presented: &Token) -> bool {
        todo!()
    }

    pub fn ends_in(&self) -> SignedDuration {
        todo!()
    }
}

/// Why a sign-in started no session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInRefused {
    /// The password is not the admin password.
    WrongPassword,
    /// The address's wait has `seconds` left.
    Wait { seconds: u64 },
    /// The admin password could not be checked, or no token drawn.
    Unavailable(String),
}

impl Access {
    pub fn new(
        _password: Arc<dyn AdminPassword>,
        _random: Arc<dyn Random>,
        _clock: Arc<dyn Clock>,
    ) -> Self {
        todo!()
    }

    pub async fn sign_in(
        &self,
        _address: IpAddr,
        _attempt: &Password,
    ) -> Result<SignedIn, SignInRefused> {
        todo!()
    }

    pub fn session(&self, _token: &Token) -> Option<LiveSession> {
        todo!()
    }

    pub fn peek(&self, _token: &Token) -> Option<LiveSession> {
        todo!()
    }

    pub fn sign_out(&self, _token: &Token) {
        todo!()
    }

    pub fn remembered_addresses(&self) -> usize {
        todo!()
    }
}

#[cfg(test)]
mod tests;
