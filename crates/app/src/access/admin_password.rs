//! Setting the admin password (technical design §10.1, decision 70): the one
//! use case `ladderback set-password` and Settings both call, so the length
//! rules, the hash and what a new password ends are the same however it is
//! set. With sign-in on, Settings changes it only with the current password,
//! which counts as a sign-in in the same waits. A new password forgets every
//! session the store keeps, as well as ending it by the fingerprint.

use std::net::IpAddr;

use domain::access::{Client, Password, PasswordProblem};

use super::{Access, PasswordRefused};
use crate::ports::{AdminPassword, SessionStore, StoreError};

/// Why a new admin password was not set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetPasswordRefused {
    /// Shorter than [`domain::access::PASSWORD_MIN_CHARS`] characters, or
    /// nothing at all.
    TooShort,
    /// Longer than [`domain::access::PASSWORD_MAX_BYTES`] bytes.
    TooLong,
    /// It could not be kept: the platform's own message, never the password.
    Unwritable(String),
}

/// Why the admin password was not changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasswordChangeRefused {
    /// The current password was refused as at a sign-in: not the admin
    /// password (counted), during the client's wait (unchecked), or not
    /// checkable.
    Current(PasswordRefused),
    /// The current password was right, and the new one was not set.
    New(SetPasswordRefused),
}

/// A new admin password was kept: what became of the sessions kept from
/// before it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub enum PasswordSet {
    /// The store forgot every one.
    SessionsForgotten,
    /// The store couldn't forget them. They have ended all the same, since
    /// each remembers the fingerprint of the password it started under: none
    /// is let in again, and the next start forgets them.
    SessionsLeft(StoreError),
}

/// Keeps `typed` as the admin password in `kept`, in place of any before it,
/// once it meets the rules for one, then has `sessions` forget every session
/// it keeps. A new admin password ends every session and API token started
/// under the one before it, even when it is the same text: they remember the
/// fingerprint of the password they started under, which also ends a session
/// the store kept when it couldn't forget it ([`PasswordSet::SessionsLeft`]).
pub async fn set_admin_password(
    kept: &dyn AdminPassword,
    sessions: &dyn SessionStore,
    typed: String,
) -> Result<PasswordSet, SetPasswordRefused> {
    set_admin_password_leaving_sessions(kept, typed).await?;
    // After the password, never before: a refused one forgets nothing. A
    // session kept between the two is forgotten too: one started under the
    // old password has ended by its fingerprint anyway, and one signed in
    // with the new password in that moment lives until the next restart.
    Ok(match sessions.remove_all().await {
        Ok(()) => PasswordSet::SessionsForgotten,
        Err(error) => PasswordSet::SessionsLeft(error),
    })
}

/// [`set_admin_password`] for a caller that can't reach the session store,
/// which a running daemon holds: the sessions it keeps end by the
/// fingerprint alone, at their next use or at the next start.
pub async fn set_admin_password_leaving_sessions(
    kept: &dyn AdminPassword,
    typed: String,
) -> Result<(), SetPasswordRefused> {
    let password = match Password::new_admin(typed) {
        Ok(password) => password,
        Err(PasswordProblem::Empty | PasswordProblem::TooShort) => {
            return Err(SetPasswordRefused::TooShort);
        }
        Err(PasswordProblem::TooLong) => return Err(SetPasswordRefused::TooLong),
    };
    kept.set(&password)
        .await
        .map_err(|error| SetPasswordRefused::Unwritable(format!("{error:?}")))
}

impl Access {
    /// Changes the admin password to `typed` for the client at `address`,
    /// given the `current` one. The current password is checked as a
    /// sign-in is, in the client's waits: it counts as failed before it is
    /// checked, a wrong one starts the next wait, and during a wait it is
    /// refused unchecked. A right one starts the waits over, then
    /// [`set_admin_password`] sets the new one, which ends every session,
    /// the caller's included, and every API token, and has the store forget
    /// every session it keeps.
    pub async fn change_password(
        &self,
        address: IpAddr,
        current: &Password,
        typed: String,
    ) -> Result<PasswordSet, PasswordChangeRefused> {
        let client = Client::of(address);
        self.check(client, current)
            .await
            .map_err(PasswordChangeRefused::Current)?;
        self.lock().waits.succeeded(client);
        set_admin_password(self.password.as_ref(), self.sessions.as_ref(), typed)
            .await
            .map_err(PasswordChangeRefused::New)
    }
}
