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

use app::ports::{Resolved, SecretResolver};
use async_trait::async_trait;
use domain::secrets::{
    FileRead, KeyringEntry, Problem, SecretError, SecretRef, SecretValue, SourceName, decode,
};

use keyring_store::{Failure, KeyringStore};

/// How long a command may run unless told otherwise.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Reads secrets from the machine: variables of the process environment,
/// files, commands and the OS keyring.
pub struct SystemSecrets {
    /// `None`: the process environment.
    environment: Option<BTreeMap<String, String>>,
    command_timeout: Duration,
    keyring: Arc<dyn KeyringStore>,
}

impl SystemSecrets {
    /// Reads the process environment and the OS keyring; a command may run
    /// for a minute.
    pub fn new() -> Self {
        Self {
            environment: None,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            keyring: keyring_store::os_store(),
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
    pub(crate) fn with_file_reader(self, _reader: FileReader) -> Self {
        todo!()
    }
}

/// Reads the file at a path, given how it is read and `$CREDENTIALS_DIRECTORY`.
#[cfg(test)]
pub(crate) type FileReader =
    Arc<dyn Fn(&str, FileRead, Option<&str>) -> Result<Resolved, SecretError> + Send + Sync>;

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
        let reading =
            tokio::task::spawn_blocking(move || file::read(&path, read, credentials.as_deref()));
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
        let reading = tokio::task::spawn_blocking(move || store.get(&service, &name));
        let got = self.limited(&source, reading).await?;
        match got {
            Ok(secret) => Ok(resolved(decode(&source, secret.as_bytes(), |text| text)?)),
            Err(Failure::NoEntry) => Err(SecretError::missing(source.clone())),
            Err(Failure::NotUnicode) => Err(fail(Problem::NotUnicode)),
            Err(Failure::Unreachable(detail)) => Err(fail(Problem::Unreachable { detail })),
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
        match tokio::time::timeout(self.command_timeout, reading).await {
            Ok(Ok(read)) => Ok(read),
            Ok(Err(error)) => Err(unfinished(source.clone(), &error)),
            Err(_elapsed) => Err(SecretError {
                source: source.clone(),
                problem: Problem::TimedOut {
                    seconds: command::whole_seconds_up(self.command_timeout),
                },
            }),
        }
    }
}

/// A blocking read that was lost: it panicked or the runtime is shutting down.
fn unfinished(source: SourceName, error: &tokio::task::JoinError) -> SecretError {
    SecretError {
        source,
        problem: Problem::Unreachable {
            detail: error.to_string(),
        },
    }
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
