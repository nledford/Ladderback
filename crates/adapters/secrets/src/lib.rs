//! Secrets adapter: reads a secret from a command, the OS keyring, a file or
//! the environment (technical design §10.2).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod command;
mod file;
mod keyring_store;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;
use std::time::Duration;

use app::ports::{Kept, Keyring, KeyringActivation, KeyringError, Resolved, SecretResolver};
use async_trait::async_trait;
use domain::secrets::{
    FileRead, KeyringCause, KeyringEntry, Problem, SecretError, SecretRef, SecretValue, SourceName,
    decode,
};
use zeroize::Zeroizing;

use keyring_store::{Failure, KeyringStore};

/// How long a command may run unless told otherwise.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the question of how the keyring service is started may take
/// before its answer is unknown: the Repositories page waits on it.
const ACTIVATION_LIMIT: Duration = Duration::from_secs(2);

/// Reads secrets from the machine: variables of the process environment,
/// files, commands and the OS keyring.
pub struct SystemSecrets {
    /// `None`: the process environment.
    environment: Option<BTreeMap<String, String>>,
    command_timeout: Duration,
    keyring: Arc<dyn KeyringStore>,
    file_reader: FileReader,
    activation: ActivationProbe,
}

impl SystemSecrets {
    /// Reads the process environment and the OS keyring; a command may run
    /// for a minute.
    pub fn new() -> Self {
        Self {
            environment: None,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            keyring: keyring_store::os_store(),
            file_reader: Arc::new(file::read),
            activation: Arc::new(keyring_store::os_activation),
        }
    }

    /// Reads `vars` instead of the process environment, for `Env` secrets and
    /// for `CREDENTIALS_DIRECTORY`. A command still inherits the environment
    /// of the process: it is the user's own helper program.
    pub fn with_environment<K, V>(mut self, vars: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.environment = Some(
            vars.into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        );
        self
    }

    /// How long a command may run before it is killed.
    pub fn with_command_timeout(mut self, limit: Duration) -> Self {
        self.command_timeout = limit;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_keyring(mut self, store: Arc<dyn KeyringStore>) -> Self {
        self.keyring = store;
        self
    }

    /// Reads files with `reader`, which a test makes slow.
    #[cfg(test)]
    pub(crate) fn with_file_reader(mut self, reader: FileReader) -> Self {
        self.file_reader = reader;
        self
    }

    /// Asks `probe` how the keyring service is started, which a test makes
    /// answer as it likes, or slowly.
    #[cfg(test)]
    pub(crate) fn with_activation(
        mut self,
        probe: impl Fn() -> KeyringActivation + Send + Sync + 'static,
    ) -> Self {
        self.activation = Arc::new(probe);
        self
    }
}

/// Reads the file at a path, given how it is read and `$CREDENTIALS_DIRECTORY`.
pub(crate) type FileReader =
    Arc<dyn Fn(&str, FileRead, Option<&str>) -> Result<Resolved, SecretError> + Send + Sync>;

/// Asks the platform how the keyring service is started. It may block, on
/// D-Bus.
pub(crate) type ActivationProbe = Arc<dyn Fn() -> KeyringActivation + Send + Sync>;

impl Default for SystemSecrets {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SystemSecrets {
    // Never the variables: they may hold secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemSecrets")
            .field(
                "environment",
                &if self.environment.is_some() {
                    "given"
                } else {
                    "process"
                },
            )
            .field("command_timeout", &self.command_timeout)
            .finish_non_exhaustive()
    }
}

/// A secret that was read, with nothing to warn about.
fn resolved(value: SecretValue) -> Resolved {
    Resolved {
        value,
        warnings: Vec::new(),
    }
}

impl SystemSecrets {
    /// A variable of the environment this reads. `None` when it is not set
    /// or can't be a variable's name (`std::env::var_os` panics on those).
    fn var(&self, name: &str) -> Option<OsString> {
        if name.is_empty() || name.contains(['=', '\0']) {
            return None;
        }
        match &self.environment {
            Some(variables) => variables.get(name).map(OsString::from),
            None => std::env::var_os(name),
        }
    }

    fn env(&self, name: &str) -> Result<Resolved, SecretError> {
        let source = SourceName::Env {
            name: name.to_owned(),
        };
        let value = self
            .var(name)
            .ok_or_else(|| SecretError::missing(source.clone()))?;
        let value = decode(&source, value.as_bytes(), |text| text)?;
        Ok(resolved(value))
    }

    async fn file(&self, path: &str, read: FileRead) -> Result<Resolved, SecretError> {
        let source = SourceName::File {
            path: path.to_owned(),
        };
        let path = path.to_owned();
        let credentials = self
            .var("CREDENTIALS_DIRECTORY")
            .and_then(|directory| directory.into_string().ok());
        let reader = Arc::clone(&self.file_reader);
        let reading =
            tokio::task::spawn_blocking(move || (reader)(&path, read, credentials.as_deref()));
        self.limited(&source, reading).await?
    }

    async fn keyring(&self, entry: &KeyringEntry) -> Result<Resolved, SecretError> {
        let source = SourceName::Keyring {
            entry: entry.to_string(),
        };
        let store = Arc::clone(&self.keyring);
        let (service, name) = (entry.service().to_owned(), entry.name().to_owned());
        let fail = |problem| SecretError {
            source: source.clone(),
            problem,
        };
        // Wrapped where it is read, so the copy is wiped wherever it is
        // dropped: here, or later for a read that outlived the time limit.
        let reading =
            tokio::task::spawn_blocking(move || store.get(&service, &name).map(Zeroizing::new));
        let got = self.limited(&source, reading).await?;
        match got {
            Ok(secret) => Ok(resolved(decode(&source, secret.as_bytes(), |text| text)?)),
            // As a read finds it: no entry it can tell is there.
            Err(Failure::NoEntry | Failure::Unconfirmed(_)) => {
                Err(SecretError::missing(source.clone()))
            }
            Err(Failure::NotUnicode) => Err(fail(Problem::NotUnicode)),
            Err(Failure::Unreachable(detail)) => Err(fail(Problem::Unreachable { detail })),
            Err(Failure::NoService(detail)) => Err(fail(Problem::KeyringUnavailable {
                detail,
                cause: KeyringCause::NoService,
            })),
            Err(Failure::Refused(detail)) => Err(fail(Problem::KeyringUnavailable {
                detail,
                cause: KeyringCause::Refused,
            })),
        }
    }

    /// Waits for a blocking read for the time limit and no longer: a keyring
    /// that waits for the user to unlock it, or a mount that has hung, must
    /// not hold a job back for good. The read itself can't be cancelled; it
    /// ends when the system lets it go.
    async fn limited<T>(
        &self,
        source: &SourceName,
        reading: tokio::task::JoinHandle<T>,
    ) -> Result<T, SecretError> {
        self.within(reading)
            .await
            .map_err(|unfinished| SecretError {
                source: source.clone(),
                problem: match unfinished {
                    Unfinished::TimedOut { seconds } => Problem::TimedOut { seconds },
                    Unfinished::Lost(detail) => Problem::Unreachable { detail },
                },
            })
    }

    /// A blocking call, waited for within the time limit, as [`limited`]
    /// waits for a read.
    ///
    /// [`limited`]: Self::limited
    async fn within<T>(&self, call: tokio::task::JoinHandle<T>) -> Result<T, Unfinished> {
        match tokio::time::timeout(self.command_timeout, call).await {
            Ok(Ok(done)) => Ok(done),
            Ok(Err(error)) => Err(Unfinished::Lost(error.to_string())),
            Err(_elapsed) => Err(Unfinished::TimedOut {
                seconds: command::whole_seconds_up(self.command_timeout),
            }),
        }
    }
}

/// Why a blocking call gave no answer.
enum Unfinished {
    /// It outlived the time limit, in whole seconds.
    TimedOut { seconds: u64 },
    /// It was lost: it panicked or the runtime is shutting down.
    Lost(String),
}

#[async_trait]
impl SecretResolver for SystemSecrets {
    async fn resolve(&self, secret: &SecretRef) -> Result<Resolved, SecretError> {
        match secret {
            SecretRef::Inline(value) => Ok(resolved(value.clone())),
            SecretRef::Env(name) => self.env(name),
            SecretRef::File { path, read } => self.file(path, *read).await,
            SecretRef::Command(command) => command::run(command, self.command_timeout)
                .await
                .map(resolved),
            SecretRef::Keyring(entry) => self.keyring(entry).await,
        }
    }
}

#[async_trait]
impl Keyring for SystemSecrets {
    /// Reads the entry, then writes it only when it isn't there, all in one
    /// blocking call within the time limit. An entry holding anything but
    /// the password, text or not, is another secret and is left as it is.
    async fn keep(
        &self,
        entry: &KeyringEntry,
        password: &SecretValue,
    ) -> Result<Kept, KeyringError> {
        let store = Arc::clone(&self.keyring);
        let (service, name) = (entry.service().to_owned(), entry.name().to_owned());
        let password = password.clone();
        // Wrapped where it is read, as a resolved entry is.
        let keeping = tokio::task::spawn_blocking(move || {
            match store.get(&service, &name).map(Zeroizing::new) {
                Ok(held) if held.as_str() == password.expose() => Ok(Kept::AlreadyThere),
                Ok(_) | Err(Failure::NotUnicode) => Err(KeyringError::Taken),
                Err(Failure::NoEntry) => store
                    .set(&service, &name, password.expose())
                    .map(|()| Kept::Added)
                    .map_err(refused),
                Err(failure) => Err(refused(failure)),
            }
        });
        self.within(keeping).await.map_err(unanswered)?
    }

    /// Deletes the entry within the time limit; one that isn't there is
    /// already as asked.
    async fn remove(&self, entry: &KeyringEntry) -> Result<(), KeyringError> {
        let store = Arc::clone(&self.keyring);
        let (service, name) = (entry.service().to_owned(), entry.name().to_owned());
        let removing = tokio::task::spawn_blocking(move || match store.delete(&service, &name) {
            Ok(()) | Err(Failure::NoEntry) => Ok(()),
            Err(failure) => Err(refused(failure)),
        });
        self.within(removing).await.map_err(unanswered)?
    }

    /// Asks the platform within [`ACTIVATION_LIMIT`]; an answer that comes
    /// later, or a question that was lost, is unknown.
    async fn activation(&self) -> KeyringActivation {
        let probe = Arc::clone(&self.activation);
        let asking = tokio::task::spawn_blocking(move || probe());
        match tokio::time::timeout(ACTIVATION_LIMIT, asking).await {
            Ok(Ok(said)) => said,
            // The question panicked or the runtime is ending, or the answer
            // is late: the blocked call is left to end on its own, as the
            // bus's own limit ends it.
            Ok(Err(_)) | Err(_) => KeyringActivation::Unknown,
        }
    }
}

/// Why the store refused to write or delete an entry.
fn refused(failure: Failure) -> KeyringError {
    match failure {
        Failure::NoService(detail) | Failure::Refused(detail) => {
            KeyringError::Unavailable { detail }
        }
        Failure::Unreachable(detail) => KeyringError::Unwritable { detail },
        // An entry that holds what isn't text holds another secret.
        Failure::NotUnicode => KeyringError::Taken,
        // Only a read finds no entry: a write that says so anyway failed,
        // with no words of the platform's.
        Failure::NoEntry => KeyringError::Unwritable {
            detail: String::new(),
        },
        // The entry may be there, unseen in a locked keyring: never written
        // over, never said to be gone.
        Failure::Unconfirmed(detail) => KeyringError::Unavailable { detail },
    }
}

/// A keyring call that gave no answer, as the port reports it.
fn unanswered(unfinished: Unfinished) -> KeyringError {
    match unfinished {
        Unfinished::TimedOut { seconds } => KeyringError::TimedOut { seconds },
        Unfinished::Lost(detail) => KeyringError::Unwritable { detail },
    }
}
