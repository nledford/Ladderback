use async_trait::async_trait;
use domain::secrets::{KeyringEntry, SecretValue};

/// Keeps a repository's password in the OS keyring, where the person chose
/// to keep it rather than in the configuration (technical design §10.2).
/// [`SecretResolver`](crate::ports::SecretResolver) reads an entry kept here.
///
/// Contract (checked by `contract::secrets`):
/// - `keep` adds an entry that isn't there ([`Kept::Added`]), and the
///   resolver then reads the password from it. An entry that already holds
///   that password is left as it is ([`Kept::AlreadyThere`]). One that holds
///   anything else is refused with [`KeyringError::Taken`] and left as it
///   is: an entry is never overwritten, since it may be the only copy of the
///   password of a repository's data.
/// - `remove` removes an entry, so the resolver then finds it `Missing`; an
///   entry that isn't there is no error.
/// - With no keyring service to ask (on Linux no Secret Service, no session
///   bus to reach one by, or one that refused otherwise than as locked, as
///   when its unlock prompt was dismissed), both fail with
///   [`KeyringError::Unavailable`]. A service that says its keyring is
///   locked fails both with [`KeyringError::Locked`], as the resolver reads
///   it as [`KeyringCause::Locked`](domain::secrets::KeyringCause::Locked)
///   (#993). A call that outlives the resolver's
///   limit fails with [`KeyringError::TimedOut`], and any other failure with
///   [`KeyringError::Unwritable`].
/// - An error never holds the password.
/// - `activation` says how the keyring service is started: within a short
///   limit, never an error, and never by reading or writing an entry, so it
///   raises no unlock prompt. An answer it can't get in time is
///   [`KeyringActivation::Unknown`].
#[async_trait]
pub trait Keyring: Send + Sync {
    async fn keep(
        &self,
        entry: &KeyringEntry,
        password: &SecretValue,
    ) -> Result<Kept, KeyringError>;

    async fn remove(&self, entry: &KeyringEntry) -> Result<(), KeyringError>;

    /// Whether the keyring service can be started on demand, as the
    /// Repositories page asks it (security §10.2). Never asked before a run.
    async fn activation(&self) -> KeyringActivation;
}

/// How the keyring service is started. On Linux the session bus starts the
/// Secret Service when a program first asks for it, if an activation file
/// names it; a service with none runs only once the desktop starts it, so a
/// program that asks before then finds no keyring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyringActivation {
    /// Something starts it whenever a program asks for it: the session bus
    /// can, or the platform's keyring is always there (the macOS keychain).
    OnDemand,
    /// It runs now, but nothing can start it on demand.
    RunningOnly,
    /// It doesn't run, and nothing can start it: there is no keyring service.
    Absent,
    /// The question got no answer: no session bus, or one that failed or
    /// didn't answer in time.
    Unknown,
}

/// How a password came to be in its keyring entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// The entry wasn't there, and now holds the password.
    Added,
    /// The entry already held the password, and was left as it is.
    AlreadyThere,
}

/// Why a password couldn't be kept in, or removed from, its keyring entry.
/// The `detail` is the platform's own words, never the password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyringError {
    /// The entry holds another secret, and was left as it is.
    Taken,
    /// There is no keyring service to ask, or it refused otherwise than as
    /// locked.
    Unavailable { detail: String },
    /// The keyring service answered that its keyring is locked: the person
    /// can unlock it and try again.
    Locked { detail: String },
    /// The keyring didn't answer within the resolver's limit, as a keyring
    /// that waits for an unlock nobody gives.
    TimedOut { seconds: u64 },
    /// The keyring answered and refused, or couldn't be read or written.
    Unwritable { detail: String },
}
