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
    todo!()
}

fn unavailable(error: impl std::fmt::Debug) -> String {
        todo!()
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
        todo!()
    }

    /// The tokens of the admin password kept now, oldest first; those of an
    /// earlier one are revoked on the way.
    pub async fn api_tokens(&self) -> Result<Vec<ApiToken>, ApiTokensRefused> {
        todo!()
    }

    /// Revokes the token `id`; whether there was one.
    pub async fn revoke_api_token(&self, id: &ApiTokenId) -> Result<bool, ApiTokensRefused> {
        todo!()
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
        todo!()
    }

    /// Whether the token `id` is kept still under the admin password kept
    /// now: an events stream opened with it ends once it isn't.
    pub async fn api_token_live(&self, id: &ApiTokenId) -> bool {
        todo!()
    }

    fn not_waiting(&self, client: Client, now: Timestamp) -> Result<(), ApiTokenRefused> {
        todo!()
    }

    /// Counts a failed token for `client`, unless its wait started meanwhile.
    fn failed(&self, client: Client) -> ApiTokenRefused {
        todo!()
    }
}

fn store_refused(error: StoreError) -> ApiTokensRefused {
        todo!()
    }

#[cfg(test)]
mod tests;
