//! API tokens over HTTP (technical design §10.1, decision 93): a program
//! other than a browser presents one as `Authorization: Bearer <token>`, and
//! is let through to the routes its scopes open; the admin creates, lists
//! and revokes them in a session.
//!
//! A token request is judged by its token alone: its cookie is ignored, and
//! it carries no CSRF token, which guards only what a browser sends on its
//! own. The request checks (host, origin, JSON content type) have refused it
//! already when they would, and the body limit still applies after.

use std::sync::Arc;

use api::{
    ApiTokenDto, ApiTokenScopeDto, ApiTokensDto, CreateApiTokenDto, CreatedApiTokenDto, ErrorCode,
    MsgArg,
};
use app::access::{Access, ApiTokenGrant, ApiTokenRefused, ApiTokensRefused};
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::header::{AUTHORIZATION, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::access::api_token::{
    ApiToken, ApiTokenId, ApiTokenLabel, LABEL_MAX_CHARS, MAX_API_TOKENS, PresentedApiToken, Scope,
    Scopes,
};

use crate::sign_in::SignIn;
use crate::{refused, refused_with};

/// The token routes' own path.
pub(crate) const TOKENS_PATH: &str = "/api/v1/tokens";

/// The scope a refusal names for a route only a session may use.
pub(crate) const SESSION_SCOPE: &str = "session";

/// A request that reached its route with a good API token: who it is and
/// what it may do. The events stream keeps it, to end once the token does.
#[derive(Clone)]
pub(crate) struct TokenOf {
    pub(crate) access: Arc<Access>,
    pub(crate) grant: ApiTokenGrant,
}

/// The token a request presents, when it has an `Authorization` header with
/// the `Bearer` scheme (in any case): `Some(None)` when its text is no
/// token, or the request has two of them. A request with no such header is
/// `None`, its other schemes ignored: it is judged by its session.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<Option<PresentedApiToken>> {
        todo!()
    }

/// What follows the `Bearer` scheme in one header's value, if that is its
/// scheme.
fn bearer_credentials(value: &[u8]) -> Option<&[u8]> {
        todo!()
    }

/// The scope table (decision 93): the scope a token needs for `method` on
/// `path`, or `None` for a route only a session may use, which is every
/// route the table doesn't name.
pub(crate) fn required_scope(method: &Method, path: &str) -> Option<Scope> {
        todo!()
    }

/// `403 api-token-scope`, naming the scope the route needs.
pub(crate) fn scope_refused(scope: &str) -> Response {
        todo!()
    }

/// Lets a request with an API token through to its route when the token is
/// good and its scopes open the route. A token is checked as a sign-in is,
/// in the waits of the connection's address: during a wait it is refused
/// unchecked, and a failed one starts the next wait.
pub(crate) async fn enforce(
    access: Arc<Access>,
    presented: Option<PresentedApiToken>,
    mut request: Request,
    next: Next,
) -> Response {
        todo!()
    }

/// `503 api-tokens-unavailable`. The detail names files of the data folder
/// or the database: the log keeps it for whoever runs the daemon.
fn unavailable(detail: &str) -> Response {
        todo!()
    }

fn sign_in_off() -> Response {
        todo!()
    }

fn refused_as(refusal: ApiTokensRefused) -> Response {
        todo!()
    }

fn count(value: usize) -> u64 {
        todo!()
    }

fn scope_dto(scope: Scope) -> ApiTokenScopeDto {
        todo!()
    }

fn scope_of(dto: ApiTokenScopeDto) -> Scope {
        todo!()
    }

/// A token as it is listed: never its secret, which the daemon doesn't keep.
fn listed(token: &ApiToken) -> ApiTokenDto {
        todo!()
    }

/// `GET /api/v1/tokens`: the tokens kept, oldest first.
pub(crate) async fn list(State(sign_in): State<SignIn>) -> Response {
        todo!()
    }

/// `POST /api/v1/tokens`: creates a token with the body's label and scopes,
/// and answers with its whole text, this once.
pub(crate) async fn create(
    State(sign_in): State<SignIn>,
    body: Result<axum::Json<CreateApiTokenDto>, JsonRejection>,
) -> Response {
        todo!()
    }

/// `DELETE /api/v1/tokens/{id}`: revokes the token; an events stream opened
/// with it ends within the recheck.
pub(crate) async fn revoke(State(sign_in): State<SignIn>, Path(id): Path<String>) -> Response {
        todo!()
    }

#[cfg(test)]
mod tests;
