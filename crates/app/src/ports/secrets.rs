use async_trait::async_trait;
use domain::secrets::{SecretError, SecretRef, SecretValue, SecretWarning};

/// Reads a secret from where it is kept (technical design §10.2).
///
/// Contract (checked by `contract::secrets`):
/// - An inline secret is its own value, with no warning.
/// - An environment variable is its value as it is; one that is set to
///   nothing is the empty secret, and one that isn't set is `Missing`, never
///   an empty secret.
/// - A file is read whole, and the secret is its first line, or all of it
///   minus one trailing newline, as the reference says. A file that isn't
///   there is `Missing`; one that can't be read, `Unreachable`; one that
///   isn't UTF-8, `NotUnicode`.
/// - A command is started without a shell, with no input and in a process
///   group of its own. The secret is the first line of its output. It fails as
///   `CommandFailed` with the exit status and a short, one-line `detail` from
///   its stderr, never from its output; as `Unreachable` when it can't be
///   started; and as `TimedOut` when it outlives the resolver's limit, after
///   which nothing it started is left running.
/// - A keyring entry that isn't there is `Missing`; a keyring that can't be
///   reached is `Unreachable`.
/// - Whatever a source gives is limited to `MAX_SECRET_BYTES`, the whole of
///   it and not only the secret in it: more is `TooLarge`.
/// - A file that group or others can read comes with a
///   `SecretWarning::FileReadableByOthers`, unless it is under a directory
///   where a container runtime or systemd sets permissions
///   (`domain::secrets::file_warning`).
/// - An error names its source and never holds a secret's value.
#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, secret: &SecretRef) -> Result<Resolved, SecretError>;
}

/// A secret that was read, and what the user should know about how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub value: SecretValue,
    pub warnings: Vec<SecretWarning>,
}
