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
    let mut credentials = headers
        .get_all(AUTHORIZATION)
        .iter()
        .filter_map(|value| bearer_credentials(value.as_bytes()));
    let first = credentials.next()?;
    if credentials.next().is_some() {
        return Some(None);
    }
    Some(
        std::str::from_utf8(first)
            .ok()
            .and_then(PresentedApiToken::parse),
    )
}

/// What follows the `Bearer` scheme in one header's value, if that is its
/// scheme.
fn bearer_credentials(value: &[u8]) -> Option<&[u8]> {
    const SCHEME: &[u8] = b"bearer";
    let scheme = value.get(..SCHEME.len())?;
    let rest = value.get(SCHEME.len()..)?;
    if !scheme.eq_ignore_ascii_case(SCHEME) || !(rest.is_empty() || rest.starts_with(b" ")) {
        return None;
    }
    Some(rest.trim_ascii())
}

/// The scope table (decision 93): the scope a token needs for `method` on
/// `path`, or `None` for a route only a session may use, which is every
/// route the table doesn't name.
pub(crate) fn required_scope(method: &Method, path: &str) -> Option<Scope> {
    let rest = path.strip_prefix("/api/v1/")?;
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    let reads = method == Method::GET || method == Method::HEAD;
    match segments.as_slice() {
        ["events" | "operations" | "plans" | "repositories" | "session"]
        | ["operations", _]
        | ["repositories", _, "snapshots" | "groups"]
        | ["plans", _, "profile"]
            if reads =>
        {
            Some(Scope::Read)
        }
        ["operations"] | ["operations", _, "cancel"] | ["repositories", _, "test"]
            if method == Method::POST =>
        {
            Some(Scope::Run)
        }
        ["repositories", _, "maintenance", "preview"] | ["profiles", "preview"]
            if method == Method::POST =>
        {
            Some(Scope::Preview)
        }
        _ => None,
    }
}

/// `403 api-token-scope`, naming the scope the route needs.
pub(crate) fn scope_refused(scope: &str) -> Response {
    refused_with(
        StatusCode::FORBIDDEN,
        ErrorCode::ApiTokenScope,
        vec![("scope", MsgArg::Text(scope.to_owned()))],
    )
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
    // As a sign-in, by the connection's address: without it every client
    // would share one wait (§10.1).
    let Some(ConnectInfo(crate::PeerAddr(peer))) = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied()
    else {
        log::error!(
            "an API token arrived without its connection's address; the server must be served with its connect info"
        );
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::SignInUnavailable,
        );
    };
    let grant = match access.check_api_token(peer.ip(), presented.as_ref()).await {
        Ok(grant) => grant,
        Err(ApiTokenRefused::Invalid) => {
            return refused(StatusCode::UNAUTHORIZED, ErrorCode::ApiTokenInvalid);
        }
        Err(ApiTokenRefused::Wait { seconds }) => {
            let mut response = refused_with(
                StatusCode::TOO_MANY_REQUESTS,
                ErrorCode::SignInWait,
                vec![("seconds", MsgArg::Count(seconds))],
            );
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from(seconds));
            return response;
        }
        Err(ApiTokenRefused::Unavailable(detail)) => return unavailable(&detail),
    };
    match required_scope(request.method(), request.uri().path()) {
        None => return scope_refused(SESSION_SCOPE),
        Some(scope) if !grant.scopes.allows(scope) => return scope_refused(scope.as_str()),
        Some(_) => {}
    }
    request.extensions_mut().insert(TokenOf { access, grant });
    next.run(request).await
}

/// `503 api-tokens-unavailable`. The detail names files of the data folder
/// or the database: the log keeps it for whoever runs the daemon.
fn unavailable(detail: &str) -> Response {
    log::warn!("the API tokens could not be read or kept: {detail}");
    refused(
        StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::ApiTokensUnavailable,
    )
}

fn sign_in_off() -> Response {
    refused(StatusCode::CONFLICT, ErrorCode::SignInOff)
}

fn refused_as(refusal: ApiTokensRefused) -> Response {
    match refusal {
        ApiTokensRefused::Full => refused_with(
            StatusCode::CONFLICT,
            ErrorCode::ApiTokenLimit,
            vec![("limit", MsgArg::Count(count(MAX_API_TOKENS)))],
        ),
        ApiTokensRefused::Unavailable(detail) => unavailable(&detail),
    }
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn scope_dto(scope: Scope) -> ApiTokenScopeDto {
    match scope {
        Scope::Read => ApiTokenScopeDto::Read,
        Scope::Run => ApiTokenScopeDto::Run,
        Scope::Preview => ApiTokenScopeDto::Preview,
        Scope::Propose => ApiTokenScopeDto::Propose,
    }
}

fn scope_of(dto: ApiTokenScopeDto) -> Scope {
    match dto {
        ApiTokenScopeDto::Read => Scope::Read,
        ApiTokenScopeDto::Run => Scope::Run,
        ApiTokenScopeDto::Preview => Scope::Preview,
        ApiTokenScopeDto::Propose => Scope::Propose,
    }
}

/// A token as it is listed: never its secret, which the daemon doesn't keep.
fn listed(token: &ApiToken) -> ApiTokenDto {
    ApiTokenDto {
        id: token.id.as_str().to_owned(),
        label: token.label.as_str().to_owned(),
        scopes: token.scopes.iter().map(scope_dto).collect(),
        created_at: token.created_at.to_string(),
        last_used_at: token.last_used_at.map(|at| at.to_string()),
    }
}

/// `GET /api/v1/tokens`: the tokens kept, oldest first.
pub(crate) async fn list(State(sign_in): State<SignIn>) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    match access.api_tokens().await {
        Ok(tokens) => (
            StatusCode::OK,
            axum::Json(ApiTokensDto {
                tokens: tokens.iter().map(listed).collect(),
            }),
        )
            .into_response(),
        Err(refusal) => refused_as(refusal),
    }
}

/// `POST /api/v1/tokens`: creates a token with the body's label and scopes,
/// and answers with its whole text, this once.
pub(crate) async fn create(
    State(sign_in): State<SignIn>,
    body: Result<axum::Json<CreateApiTokenDto>, JsonRejection>,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    let Ok(axum::Json(CreateApiTokenDto { label, scopes })) = body else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let label = match ApiTokenLabel::new(label) {
        Ok(label) => label,
        Err(_) => {
            return refused_with(
                StatusCode::BAD_REQUEST,
                ErrorCode::ApiTokenLabel,
                vec![("max", MsgArg::Count(count(LABEL_MAX_CHARS)))],
            );
        }
    };
    let scopes = match Scopes::new(scopes.into_iter().map(scope_of)) {
        Ok(scopes) => scopes,
        Err(_) => {
            return refused(StatusCode::BAD_REQUEST, ErrorCode::ApiTokenNoScope);
        }
    };
    match access.create_api_token(label, scopes).await {
        Ok(created) => (
            StatusCode::CREATED,
            axum::Json(CreatedApiTokenDto {
                token: listed(&created.token),
                secret: created.text.expose().to_owned(),
            }),
        )
            .into_response(),
        Err(refusal) => refused_as(refusal),
    }
}

/// `DELETE /api/v1/tokens/{id}`: revokes the token; an events stream opened
/// with it ends within the recheck.
pub(crate) async fn revoke(State(sign_in): State<SignIn>, Path(id): Path<String>) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    let unknown = |id: String| {
        refused_with(
            StatusCode::NOT_FOUND,
            ErrorCode::ApiTokenUnknown,
            vec![("token", MsgArg::Text(id))],
        )
    };
    let Some(token) = ApiTokenId::parse(&id) else {
        return unknown(id);
    };
    match access.revoke_api_token(&token).await {
        Ok(true) => (StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
        Ok(false) => unknown(id),
        Err(refusal) => refused_as(refusal),
    }
}

#[cfg(test)]
mod tests;
