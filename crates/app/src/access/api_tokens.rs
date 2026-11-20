//! API tokens (technical design §10.1, decisions 93 and 127): creating,
//! listing, changing the permissions of and revoking them in a session, and
//! checking the one a program presents. A failed check counts as a failed
//! sign-in for the client, in the same waits; a good one doesn't start them
//! over. Every creation, change and revocation is logged, never a secret.

use std::net::IpAddr;

use api::{PermissionLevelDto, PermissionsDto};
use domain::access::api_token::{
    ApiToken, ApiTokenId, ApiTokenLabel, ApiTokenSecret, ApiTokenText, MAX_API_TOKENS,
    PresentedApiToken, TokenPermissions,
};
use domain::access::permissions::{Level, NoSuchLevel, Permission, Permissions};
use domain::access::{Client, Password};
use domain::{SignedDuration, Timestamp};

use super::{Access, PasswordRefused};
use crate::ports::{Added, StoreError};

/// `permissions` as a log line names them: each row that is on, with its
/// level, as `backups: read`, joined by commas; `no access` for none.
pub fn logged(permissions: Permissions) -> String {
    let rows: Vec<String> = permissions
        .rows()
        .map(|(row, level)| format!("{}: {}", row.as_str(), level.as_str()))
        .collect();
    if rows.is_empty() {
        "no access".to_owned()
    } else {
        rows.join(", ")
    }
}

/// `permissions` on the wire: each permission at its level or `none`.
pub fn permissions_dto(permissions: Permissions) -> PermissionsDto {
    let at = |permission: Permission| {
        if permissions.grants(permission) {
            match permission.level() {
                Level::Read => PermissionLevelDto::Read,
                Level::StartAndCancel => PermissionLevelDto::StartAndCancel,
                Level::Preview => PermissionLevelDto::Preview,
                Level::Propose => PermissionLevelDto::Propose,
                Level::Run => PermissionLevelDto::Run,
            }
        } else {
            PermissionLevelDto::None
        }
    };
    PermissionsDto {
        backups: at(Permission::Backups),
        backup_runs: at(Permission::BackupRuns),
        previews: at(Permission::Previews),
        configuration: at(Permission::Configuration),
        snapshot_contents: at(Permission::SnapshotContents),
        daemon_log: at(Permission::DaemonLog),
        repository_check: at(Permission::RepositoryCheck),
    }
}

/// The permissions a request names; a level its permission doesn't have,
/// as `"backups": "propose"`, is refused.
pub fn permissions_of(dto: PermissionsDto) -> Result<Permissions, NoSuchLevel> {
    let PermissionsDto {
        backups,
        backup_runs,
        previews,
        configuration,
        snapshot_contents,
        daemon_log,
        repository_check,
    } = dto;
    [
        (Permission::Backups, backups),
        (Permission::BackupRuns, backup_runs),
        (Permission::Previews, previews),
        (Permission::Configuration, configuration),
        (Permission::SnapshotContents, snapshot_contents),
        (Permission::DaemonLog, daemon_log),
        (Permission::RepositoryCheck, repository_check),
    ]
    .into_iter()
    .try_fold(Permissions::NONE, |permissions, (permission, level)| {
        let level = match level {
            PermissionLevelDto::None => None,
            PermissionLevelDto::Read => Some(Level::Read),
            PermissionLevelDto::StartAndCancel => Some(Level::StartAndCancel),
            PermissionLevelDto::Preview => Some(Level::Preview),
            PermissionLevelDto::Propose => Some(Level::Propose),
            PermissionLevelDto::Run => Some(Level::Run),
        };
        permissions.with(permission, level)
    })
}

/// A token that has just been created, and its whole text, shown once.
#[derive(Debug)]
pub struct CreatedApiToken {
    pub token: ApiToken,
    pub text: ApiTokenText,
}

/// Why no token was created, listed or revoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiTokensRefused {
    /// [`MAX_API_TOKENS`] are kept already.
    Full,
    /// The admin password or the store could not be read, or no random bytes
    /// drawn: the text is their own message.
    Unavailable(String),
}

/// What a request's token is allowed: who it is and what it may do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiTokenGrant {
    pub id: ApiTokenId,
    pub label: ApiTokenLabel,
    pub permissions: Permissions,
}

/// Why a presented token was not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiTokenRefused {
    /// No such token, a wrong secret, a token of an admin password set since,
    /// or text that is no token: one refusal for all, so none tells which
    /// tokens exist. It counted as a failed sign-in.
    Invalid,
    /// The client's wait after failed sign-ins has `seconds` left, rounded
    /// up; the token was not checked.
    Wait { seconds: u64 },
    /// The admin password or the store could not be read.
    Unavailable(String),
}

/// Why a token's permissions weren't changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiTokenChangeRefused {
    /// The current admin password was wrong, or couldn't be checked; a
    /// wrong one counted as a failed sign-in.
    Password(PasswordRefused),
    /// The store could not be read or written: its own message.
    Unavailable(String),
}

/// What is left of a wait, in whole seconds, rounded up.
pub(super) fn seconds_left(left: SignedDuration) -> u64 {
    let millis = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
    millis.div_ceil(1000)
}

fn unavailable(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

impl Access {
    /// Creates a token with `label` and `permissions` under the admin
    /// password kept now. Tokens of an earlier password are revoked first,
    /// so they never take a place of the [`MAX_API_TOKENS`].
    pub async fn create_api_token(
        &self,
        label: ApiTokenLabel,
        permissions: TokenPermissions,
    ) -> Result<CreatedApiToken, ApiTokensRefused> {
        let under = self
            .password
            .fingerprint()
            .map_err(|error| ApiTokensRefused::Unavailable(unavailable(error)))?;
        self.tokens
            .revoke_all_but(under)
            .await
            .map_err(store_refused)?;
        let draw = || {
            self.random
                .token()
                .map_err(|error| ApiTokensRefused::Unavailable(error.0))
        };
        let (id, secret) = (draw()?, draw()?);
        let id = ApiTokenId::from_bytes(&id);
        let secret = ApiTokenSecret::from_bytes(secret);
        let token = ApiToken::issue(id, label, permissions, &secret, under, self.clock.now());
        match self
            .tokens
            .add(&token, MAX_API_TOKENS)
            .await
            .map_err(store_refused)?
        {
            Added::Kept => {
                self.tokens_written();
                log::info!(
                    "API token {} ({}) created, with {}",
                    token.id.as_str(),
                    token.label.as_str(),
                    logged(token.permissions.get())
                );
                Ok(CreatedApiToken {
                    text: ApiTokenText::new(&token.id, &secret),
                    token,
                })
            }
            Added::Full => Err(ApiTokensRefused::Full),
        }
    }

    /// The tokens of the admin password kept now, oldest first; those of an
    /// earlier one are revoked on the way.
    pub async fn api_tokens(&self) -> Result<Vec<ApiToken>, ApiTokensRefused> {
        let under = self
            .password
            .fingerprint()
            .map_err(|error| ApiTokensRefused::Unavailable(unavailable(error)))?;
        self.tokens
            .revoke_all_but(under)
            .await
            .map_err(store_refused)?;
        self.tokens.all().await.map_err(store_refused)
    }

    /// Revokes the token `id`; whether there was one.
    pub async fn revoke_api_token(&self, id: &ApiTokenId) -> Result<bool, ApiTokensRefused> {
        let revoked = self.tokens.revoke(id).await.map_err(store_refused)?;
        if revoked {
            self.tokens_written();
            log::info!("API token {} revoked", id.as_str());
        }
        Ok(revoked)
    }

    /// Gives the token `id` other `permissions`, only with the `current`
    /// admin password, checked in the waits of the client at `address` as
    /// a sign-in is, so that a stolen session alone can't widen a token;
    /// whether there was one. The route lets only a session ask, never a
    /// token (§10.1).
    pub async fn change_api_token_permissions(
        &self,
        address: IpAddr,
        current: &Password,
        id: &ApiTokenId,
        permissions: TokenPermissions,
    ) -> Result<bool, ApiTokenChangeRefused> {
        self.step_up(address, current)
            .await
            .map_err(ApiTokenChangeRefused::Password)?;
        let changed = self
            .tokens
            .set_permissions(id, permissions)
            .await
            .map_err(|error| ApiTokenChangeRefused::Unavailable(error.to_string()))?;
        if changed {
            self.tokens_written();
            log::info!(
                "API token {} now has {}",
                id.as_str(),
                logged(permissions.get())
            );
        }
        Ok(changed)
    }

    /// Checks the token a request from `address` presents: `None` when the
    /// request's text is no token. During the client's wait it is refused
    /// unchecked; a token that fails counts as a failed sign-in, and one that
    /// passes leaves the waits as they were. Its use is recorded at most once
    /// a minute.
    pub async fn check_api_token(
        &self,
        address: IpAddr,
        presented: Option<&PresentedApiToken>,
    ) -> Result<ApiTokenGrant, ApiTokenRefused> {
        let client = Client::of(address);
        self.not_waiting(client, self.clock.now())?;
        let Some(presented) = presented else {
            return Err(self.failed(client));
        };
        let under = self
            .password
            .fingerprint()
            .map_err(|error| ApiTokenRefused::Unavailable(unavailable(error)))?;
        let kept = self
            .tokens
            .get(presented.id())
            .await
            .map_err(|error| ApiTokenRefused::Unavailable(unavailable(error)))?;
        // Checked and counted under one lock, after a second look at the
        // wait: of two tokens checked at once, the second sees the wait the
        // first one's failure started.
        let now = self.clock.now();
        let token = {
            let mut state = self.lock();
            if let Some(left) = state.waits.waiting(client, now) {
                return Err(ApiTokenRefused::Wait {
                    seconds: seconds_left(left),
                });
            }
            match kept {
                Some(token) if token.accepts(presented, under) => token,
                _ => {
                    // Not waiting, so this counts the failure.
                    let _ = state.waits.attempt(client, now);
                    return Err(ApiTokenRefused::Invalid);
                }
            }
        };
        if token.use_to_record(now) {
            // The last use only informs the admin: a write that fails
            // leaves the earlier one shown and refuses nothing, and is
            // tried again at the token's next use.
            match self.tokens.record_use(&token.id, now).await {
                Ok(()) => self.tokens_written(),
                Err(error) => self.token_use_unkept(&error),
            }
        }
        Ok(ApiTokenGrant {
            id: token.id,
            label: token.label,
            permissions: token.permissions.get(),
        })
    }

    /// The permissions the token `id` holds now, while it is kept still
    /// under the admin password kept now; `None` once it isn't. An events
    /// stream opened with it ends once it is `None` or lacks what the stream
    /// needs, and shows what the permissions it holds now allow.
    pub async fn api_token_permissions(&self, id: &ApiTokenId) -> Option<Permissions> {
        let under = self.password.fingerprint().ok()?;
        match self.tokens.get(id).await {
            Ok(Some(token)) if token.password == under => Some(token.permissions.get()),
            _ => None,
        }
    }

    fn not_waiting(&self, client: Client, now: Timestamp) -> Result<(), ApiTokenRefused> {
        match self.lock().waits.waiting(client, now) {
            Some(left) => Err(ApiTokenRefused::Wait {
                seconds: seconds_left(left),
            }),
            None => Ok(()),
        }
    }

    /// Counts a failed token for `client`, unless its wait started meanwhile.
    fn failed(&self, client: Client) -> ApiTokenRefused {
        let now = self.clock.now();
        match self.lock().waits.attempt(client, now) {
            Ok(()) => ApiTokenRefused::Invalid,
            Err(left) => ApiTokenRefused::Wait {
                seconds: seconds_left(left),
            },
        }
    }

    /// A write to the API-token store changed it: the store writes again,
    /// so the next last use it fails to write is logged. A write that
    /// changes nothing, such as the cleanup of an earlier password's
    /// tokens that the list and a new token start with, may succeed on a
    /// store that can't keep a change, so it doesn't count.
    fn tokens_written(&self) {
        self.lock().token_store_failing = false;
    }

    /// A token's last use the store failed to write with `error`: logged
    /// at its first failure since a write succeeded, by the error's code
    /// alone (I4), as a failed session write is.
    fn token_use_unkept(&self, error: &StoreError) {
        let first = !std::mem::replace(&mut self.lock().token_store_failing, true);
        if first {
            log::warn!(
                "the database could not keep an API token's last use ({}); it is tried again at the token's next use, and further failures of this kind are not logged until a write succeeds",
                error.code()
            );
        }
    }
}

fn store_refused(error: StoreError) -> ApiTokensRefused {
    ApiTokensRefused::Unavailable(error.to_string())
}

#[cfg(test)]
mod tests;
