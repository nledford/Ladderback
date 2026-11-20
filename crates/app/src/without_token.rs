//! What AI agents may do without a token (technical design §10.1, decision
//! 127): the one place the daemon holds a tokenless `ladderback mcp` session,
//! headless or scratch, to the permissions a person set for it, and where a
//! person changes them. They are kept in the configuration's
//! `[without-token]` table, read again for every request, as the rest of it
//! is.

use std::net::IpAddr;
use std::sync::Arc;

use domain::access::Password;
use domain::access::permissions::{Permission, Permissions};

use crate::access::{Access, PasswordRefused, logged};
use crate::ports::{ConfigError, ConfigStore};
use crate::repositories::ConfigWrites;

/// Why a tokenless request wasn't let through.
#[derive(Debug)]
pub enum WithoutTokenRefused {
    /// The permission it needs isn't granted.
    Lacks(Permission),
    /// The configuration couldn't be read, so nothing is let through.
    Unavailable(ConfigError),
}

/// Why the permissions weren't changed.
#[derive(Debug)]
pub enum WithoutTokenChangeRefused {
    /// With sign-in on, the current admin password was missing, wrong, or
    /// couldn't be checked; a wrong one counted as a failed sign-in.
    Password(PasswordRefused),
    /// The configuration couldn't be read or written.
    Unavailable(ConfigError),
}

/// Reads, checks and changes what AI agents may do without a token.
pub struct WithoutToken {
    config: Arc<dyn ConfigStore>,
    writes: Arc<ConfigWrites>,
}

impl WithoutToken {
    pub fn new(config: Arc<dyn ConfigStore>, writes: Arc<ConfigWrites>) -> Self {
        Self { config, writes }
    }

    /// The permissions the configuration holds.
    pub async fn permissions(&self) -> Result<Permissions, ConfigError> {
        todo!()
    }

    /// Lets a tokenless request that needs `permission` through when it is
    /// granted, with every permission the configuration holds: what the
    /// route then shows depends on them too, as a warning's path does on
    /// Snapshot contents.
    pub async fn allow(&self, permission: Permission) -> Result<Permissions, WithoutTokenRefused> {
        todo!()
    }

    /// Keeps `to` as what AI agents may do without a token, and writes the
    /// change to the log. With sign-in on (`access`), only with the
    /// `current` admin password, checked in the waits of the client at
    /// `address` as a sign-in is, so that a stolen session alone can't
    /// widen them; with sign-in off there is no password to ask for.
    #[expect(
        clippy::result_large_err,
        reason = "the configuration store's own error, which the route maps to its code"
    )]
    pub async fn change(
        &self,
        access: Option<&Access>,
        address: IpAddr,
        current: Option<&Password>,
        to: Permissions,
    ) -> Result<Permissions, WithoutTokenChangeRefused> {
        if let Some(access) = access {
            let current = current.ok_or(WithoutTokenChangeRefused::Password(
                PasswordRefused::WrongPassword,
            ))?;
            access
                .step_up(address, current)
                .await
                .map_err(WithoutTokenChangeRefused::Password)?;
        }
        let _writing = self.writes.lock().await;
        let loaded = self
            .config
            .load()
            .await
            .map_err(WithoutTokenChangeRefused::Unavailable)?;
        self.config
            .save(&loaded.with_without_token(to))
            .await
            .map_err(WithoutTokenChangeRefused::Unavailable)?;
        log::info!("AI agents without a token now have {}", logged(to));
        Ok(to)
    }
}

#[cfg(test)]
mod tests;
