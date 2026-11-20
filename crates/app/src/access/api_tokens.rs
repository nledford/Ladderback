//! API tokens (technical design §10.1, decision 93): creating, listing and
//! revoking them in a session, and checking the one a program presents. A
//! failed check counts as a failed sign-in for the client, in the same waits;
//! a good one doesn't start them over.

use std::net::IpAddr;

use domain::access::Client;
use domain::access::api_token::{
    ApiToken, ApiTokenId, ApiTokenLabel, ApiTokenSecret, ApiTokenText, MAX_API_TOKENS,
    PresentedApiToken, Scopes,
};
use domain::{SignedDuration, Timestamp};

use super::Access;
use crate::ports::{Added, StoreError};

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
    pub scopes: Scopes,
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

/// What is left of a wait, in whole seconds, rounded up.
pub(super) fn seconds_left(left: SignedDuration) -> u64 {
    let millis = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
    millis.div_ceil(1000)
}

fn unavailable(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

impl Access {
    /// Creates a token with `label` and `scopes` under the admin password
    /// kept now. Tokens of an earlier password are revoked first, so they
    /// never take a place of the [`MAX_API_TOKENS`].
    pub async fn create_api_token(
        &self,
        label: ApiTokenLabel,
        scopes: Scopes,
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
        let token = ApiToken::issue(id, label, scopes, &secret, under, self.clock.now());
        match self
            .tokens
            .add(&token, MAX_API_TOKENS)
            .await
            .map_err(store_refused)?
        {
            Added::Kept => {
                self.tokens_written();
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
        }
        Ok(revoked)
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
            scopes: token.scopes,
        })
    }

    /// Whether the token `id` is kept still under the admin password kept
    /// now: an events stream opened with it ends once it isn't.
    pub async fn api_token_live(&self, id: &ApiTokenId) -> bool {
        let Ok(under) = self.password.fingerprint() else {
            return false;
        };
        matches!(self.tokens.get(id).await, Ok(Some(token)) if token.password == under)
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
