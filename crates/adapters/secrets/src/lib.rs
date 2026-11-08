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

use std::sync::Arc;
use std::time::Duration;

use app::ports::{Resolved, SecretResolver};
use async_trait::async_trait;
use domain::secrets::{SecretError, SecretRef};

use keyring_store::KeyringStore;

/// Reads secrets from the machine: variables of the process environment,
/// files, commands and the OS keyring.
pub struct SystemSecrets {
    keyring: Arc<dyn KeyringStore>,
}

impl SystemSecrets {
    /// Reads the process environment and the OS keyring; a command may run
    /// for a minute.
    pub fn new() -> Self {
        todo!("a resolver of the machine's secrets")
    }

    /// Reads `vars` instead of the process environment, for `Env` secrets and
    /// for `CREDENTIALS_DIRECTORY`. A command still inherits the environment
    /// of the process: it is the user's own helper program.
    pub fn with_environment<K, V>(self, _vars: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        todo!("read the given environment")
    }

    /// How long a command may run before it is killed.
    pub fn with_command_timeout(self, _limit: Duration) -> Self {
        todo!("a time limit for commands")
    }

    #[cfg(test)]
    pub(crate) fn with_keyring(self, _store: Arc<dyn KeyringStore>) -> Self {
        todo!("read the keyring from the given store")
    }
}

impl Default for SystemSecrets {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SystemSecrets {
    // Never the variables: they may hold secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemSecrets").finish_non_exhaustive()
    }
}

#[async_trait]
impl SecretResolver for SystemSecrets {
    async fn resolve(&self, _secret: &SecretRef) -> Result<Resolved, SecretError> {
        todo!("read the secret from where it is kept")
    }
}
