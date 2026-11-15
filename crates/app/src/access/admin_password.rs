//! Setting the admin password (technical design §10.1, decision 70): the one
//! use case `ladderback set-password` and Settings both call, so the length
//! rules, the hash and what a new password ends are the same however it is
//! set. With sign-in on, Settings changes it only with the current password,
//! which counts as a sign-in in the same waits.

use std::net::IpAddr;

use domain::access::Password;

use super::{Access, SignInRefused};
use crate::ports::AdminPassword;

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
    /// The current password was refused as a sign-in with it would be: not
    /// the admin password (counted), during the client's wait (unchecked), or
    /// not checkable.
    Current(SignInRefused),
    /// The current password was right, and the new one was not set.
    New(SetPasswordRefused),
}

/// Keeps `typed` as the admin password in `kept`, in place of any before it,
/// once it meets the rules for one. A new admin password ends every session
/// and API token started under the one before it, even when it is the same
/// text: they remember the fingerprint of the password they started under.
pub async fn set_admin_password(
    kept: &dyn AdminPassword,
    typed: String,
) -> Result<(), SetPasswordRefused> {
    todo!("{:p}{}", kept, typed.len())
}

impl Access {
    /// Changes the admin password to `typed` for the client at `address`,
    /// given the `current` one. The current password is checked as a
    /// sign-in is, in the client's waits: it counts as failed before it is
    /// checked, a wrong one starts the next wait, and during a wait it is
    /// refused unchecked. A right one starts the waits over, then
    /// [`set_admin_password`] sets the new one, which ends every session,
    /// the caller's included, and every API token.
    pub async fn change_password(
        &self,
        address: IpAddr,
        current: &Password,
        typed: String,
    ) -> Result<(), PasswordChangeRefused> {
        todo!("{address}{:?}{}", current, typed.len())
    }
}
