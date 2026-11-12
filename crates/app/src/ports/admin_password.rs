use async_trait::async_trait;
use domain::access::{Password, PasswordFingerprint};

/// Where the admin password is kept (§10.1): only a hash of it, which checks
/// an attempt and is replaced by `set-password`. Without one, sign-in is off.
#[async_trait]
pub trait AdminPassword: Send + Sync {
    /// Whether `attempt` is the admin password.
    async fn verify(&self, attempt: &Password) -> Result<bool, AdminPasswordError>;

    /// Keeps `password` as the admin password, in place of any before it.
    async fn set(&self, password: &Password) -> Result<(), AdminPasswordError>;

    /// Which admin password is kept now: the same while it is, another once
    /// it is set anew, even to the same text. Asked on every request that
    /// carries a session, so it is cheap and never hashes an attempt.
    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError>;
}

/// Why the admin password could not be checked or kept. The text is the
/// platform's own message, never the password or its hash (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminPasswordError {
    /// No admin password has been set.
    NotSet,
    /// What is kept could not be read, or is not a hash this build checks.
    Unreadable(String),
    /// The new password could not be kept.
    Unwritable(String),
}
