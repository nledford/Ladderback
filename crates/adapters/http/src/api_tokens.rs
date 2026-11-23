//! API tokens over HTTP (technical design §10.1, decisions 93 and 127): a
//! program other than a browser presents one as `Authorization: Bearer
//! <token>`, and is let through to the routes its permissions open; the
//! admin creates, lists, changes the permissions of and revokes them in a
//! session.
//!
//! A token request is judged by its token alone: its cookie is ignored, and
//! it carries no CSRF token, which guards only what a browser sends on its
//! own. The request checks (host, origin, JSON content type) have refused it
//! already when they would, and the body limit still applies after.

use std::sync::Arc;

use api::{
    ApiTokenDto, ApiTokensDto, ChangeApiTokenPermissionsDto, ChangeWithoutTokenDto,
    CreateApiTokenDto, CreatedApiTokenDto, ErrorCode, MsgArg,
};
use app::access::{
    Access, ApiTokenChangeRefused, ApiTokenGrant, ApiTokenRefused, ApiTokensRefused,
    PasswordRefused, permissions_dto, permissions_of,
};
use app::ports::config_unusable;
use app::without_token::{WithoutToken, WithoutTokenChangeRefused, WithoutTokenRefused};
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::header::{AUTHORIZATION, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::access::Password;
use domain::access::api_token::{
    ApiToken, ApiTokenId, ApiTokenLabel, LABEL_MAX_CHARS, MAX_API_TOKENS, PresentedApiToken,
    TokenPermissions,
};
use domain::access::permissions::{Permission, Permissions};

use crate::sign_in::SignIn;
use crate::{refused, refused_detailed, refused_with};

/// The token routes' own path.
pub(crate) const TOKENS_PATH: &str = "/api/v1/tokens";

/// What a program may do: any good token reads it, and `ladderback mcp`
/// lists only the tools it may call.
pub(crate) const PERMISSIONS_PATH: &str = "/api/v1/permissions";

/// What a refusal names for a route only a session may use.
pub(crate) const SESSION_ONLY: &str = "session";

/// A request that reached its route with a good API token: who it is and
/// what it may do. The events stream keeps it, to end once the token does.
#[derive(Clone)]
pub(crate) struct TokenOf {
    pub(crate) access: Arc<Access>,
    pub(crate) grant: ApiTokenGrant,
}

/// What the program asking may do, for a request that reached its route
/// with a good API token, or, with sign-in off, marked as an AI agent's: the
/// token's permissions, or what AI agents may do without a token. A request
/// without it is a person's, a session's or an unmarked one with sign-in
/// off, and may see everything. A route that shows more to some programs
/// than to others reads it, as a warning's path needs Snapshot contents.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Granted(pub(crate) Permissions);

/// Every permission: what a person may do.
pub(crate) fn every() -> Permissions {
    Permission::ALL
        .into_iter()
        .fold(Permissions::NONE, |all, permission| {
            all.with(permission, Some(permission.level()))
                .unwrap_or(all)
        })
}

/// What `permissions` may see of `warning`: a program without Snapshot
/// contents sees neither its item nor the engine's own words, either of
/// which can name a file (decision 127).
pub fn warning_shown(
    permissions: Permissions,
    warning: api::OperationWarningDto,
) -> api::OperationWarningDto {
    if permissions.grants(Permission::SnapshotContents) {
        return warning;
    }
    api::OperationWarningDto {
        item: None,
        detail: String::new(),
        ..warning
    }
}

/// What `permissions` may see of an operation's `summary`: a program
/// without Snapshot contents gets its failure's code only, neither the
/// failure's arguments nor the engine's own words, and neither the snapshot
/// entry a restore read nor the folder it wrote into, any of which can name
/// a file (decision 127).
pub fn summary_shown(
    permissions: Permissions,
    summary: api::OperationSummaryDto,
) -> api::OperationSummaryDto {
    if permissions.grants(Permission::SnapshotContents) {
        return summary;
    }
    api::OperationSummaryDto {
        failure: summary
            .failure
            .map(|failure| api::ErrorDto::new(failure.code, [], None)),
        entry: None,
        restored_to: None,
        ..summary
    }
}

/// What `permissions` may see of the status summary: each operation in it
/// as [`summary_shown`].
pub fn status_shown(permissions: Permissions, status: api::StatusDto) -> api::StatusDto {
    api::StatusDto {
        plans: status
            .plans
            .into_iter()
            .map(|plan| api::PlanStatusDto {
                last: plan.last.map(|last| summary_shown(permissions, last)),
                ..plan
            })
            .collect(),
        running: status
            .running
            .into_iter()
            .map(|summary| summary_shown(permissions, summary))
            .collect(),
        unreadable_repositories: status
            .unreadable_repositories
            .into_iter()
            .map(|unreadable| api::UnreadableRepositoryDto {
                operation: summary_shown(permissions, unreadable.operation),
                ..unreadable
            })
            .collect(),
        ..status
    }
}

/// What `permissions` may see of `event`: a warning's arguments, its item
/// and the engine's own words, as [`warning_shown`], and a failure's
/// arguments and the engine's own words, as [`summary_shown`], only with
/// Snapshot contents. The hub's machines, as its routes, only `machines`:
/// for a session, or the browser with sign-in off (§16, decision 121),
/// never a token or an AI agent, whatever it holds.
pub(crate) fn event_shown(
    permissions: Permissions,
    machines: bool,
    event: api::UiEvent,
) -> Option<api::UiEvent> {
    let _ = machines;
    Some(contents_shown(permissions, event))
}

/// What `permissions` may see of `event`'s contents.
fn contents_shown(permissions: Permissions, event: api::UiEvent) -> api::UiEvent {
    if permissions.grants(Permission::SnapshotContents) {
        return event;
    }
    match event {
        api::UiEvent::OperationWarning {
            operation, code, ..
        } => api::UiEvent::OperationWarning {
            operation,
            code,
            args: Vec::new(),
        },
        api::UiEvent::OperationFailed {
            operation,
            code,
            finished_at,
            ..
        } => api::UiEvent::OperationFailed {
            operation,
            code,
            args: Vec::new(),
            finished_at,
        },
        event => event,
    }
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

/// What a program needs for a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Needs {
    /// One permission.
    Permission(Permission),
    /// Only a good token, whatever its permissions: what it may do.
    AnyToken,
    /// A signed-in browser's session: no token opens the route.
    Session,
}

/// The permission table (decision 127): what a program needs for `method`
/// on `path`. Every route the table doesn't name is a session's only.
pub(crate) fn required_permission(method: &Method, path: &str) -> Needs {
    let Some(rest) = path.strip_prefix("/api/v1/") else {
        return Needs::Session;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Needs::Session;
    }
    let reads = method == Method::GET || method == Method::HEAD;
    let permission = match segments.as_slice() {
        ["permissions"] if reads => return Needs::AnyToken,
        ["log"] if reads => Permission::DaemonLog,
        // A search names what a folder's page names (decision 132).
        ["repositories", _, "snapshots", _, "folder"] | ["repositories", _, "find"] if reads => {
            Permission::SnapshotContents
        }
        ["events" | "operations" | "pairing" | "plans" | "repositories" | "session" | "status"]
        | ["operations", _]
        | ["repositories", _, "snapshots" | "groups" | "stats"]
        | ["plans", _, "profile"]
            if reads =>
        {
            Permission::Backups
        }
        // Reading a repository's stats now opens the repository, as a test
        // does, and keeps the reading (§16, decision 104).
        ["operations"] | ["operations", _, "cancel"] | ["repositories", _, "test" | "stats"]
            if method == Method::POST =>
        {
            Permission::BackupRuns
        }
        ["repositories", _, "maintenance", "preview"] | ["profiles", "preview"]
            if method == Method::POST =>
        {
            Permission::Previews
        }
        // A token sees and withdraws only the proposals it made (§16,
        // decision 89).
        ["proposals"] | ["proposals", _] if reads => Permission::Configuration,
        ["proposals"] | ["proposals", _, "withdraw"] | ["dry-runs"] if method == Method::POST => {
            Permission::Configuration
        }
        _ => return Needs::Session,
    };
    Needs::Permission(permission)
}

/// `403 api-token-permission`, naming what the route needs.
pub(crate) fn permission_refused(needs: &str) -> Response {
    refused_with(
        StatusCode::FORBIDDEN,
        ErrorCode::ApiTokenPermission,
        vec![("permission", MsgArg::Text(needs.to_owned()))],
    )
}

/// With sign-in off, holds a request `ladderback mcp` marked as an AI
/// agent's to what AI agents may do without a token (§10.5): a route that
/// needs a permission the configuration's `[without-token]` table doesn't
/// grant is refused `403 without-token-permission`, naming it, and a route
/// no token may use is refused naming `session`. What it may do goes with
/// the request as [`Granted`].
pub(crate) async fn enforce_without_token(
    without_token: &WithoutToken,
    mut request: Request,
    next: Next,
) -> Response {
    let held = match required_permission(request.method(), request.uri().path()) {
        Needs::Session => return without_token_refused(SESSION_ONLY),
        Needs::AnyToken => without_token
            .permissions()
            .await
            .map_err(WithoutTokenRefused::Unavailable),
        Needs::Permission(permission) => without_token.allow(permission).await,
    };
    match held {
        Ok(permissions) => {
            request.extensions_mut().insert(Granted(permissions));
            next.run(request).await
        }
        Err(WithoutTokenRefused::Lacks(permission)) => without_token_refused(permission.as_str()),
        Err(WithoutTokenRefused::Unavailable(error)) => {
            log::warn!("what AI agents may do without a token could not be read: {error}");
            config_refused(error)
        }
    }
}

/// `403 without-token-permission`, naming what the route needs.
fn without_token_refused(needs: &str) -> Response {
    refused_with(
        StatusCode::FORBIDDEN,
        ErrorCode::WithoutTokenPermission,
        vec![("permission", MsgArg::Text(needs.to_owned()))],
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
    match required_permission(request.method(), request.uri().path()) {
        Needs::Session => return permission_refused(SESSION_ONLY),
        Needs::Permission(permission) if !grant.permissions.grants(permission) => {
            return permission_refused(permission.as_str());
        }
        Needs::Permission(_) | Needs::AnyToken => {}
    }
    request.extensions_mut().insert(Granted(grant.permissions));
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

/// A token as it is listed: never its secret, which the daemon doesn't keep.
fn listed(token: &ApiToken) -> ApiTokenDto {
    ApiTokenDto {
        id: token.id.as_str().to_owned(),
        label: token.label.as_str().to_owned(),
        permissions: permissions_dto(token.permissions.get()),
        created_at: token.created_at.to_string(),
        last_used_at: token.last_used_at.map(|at| at.to_string()),
    }
}

/// The permissions a body names, at least one, or the code of its refusal.
fn token_permissions(dto: api::PermissionsDto) -> Result<TokenPermissions, ErrorCode> {
    let permissions = permissions_of(dto).map_err(|_| ErrorCode::InvalidBody)?;
    TokenPermissions::new(permissions).map_err(|_| ErrorCode::ApiTokenNoPermission)
}

/// `GET /api/v1/permissions`: what the request may do. A token's
/// permissions; with sign-in off, what AI agents may do without a token for
/// a request `ladderback mcp` marked; everything for a session, and for an
/// unmarked request with sign-in off. The mark counts only with sign-in
/// off: with sign-in on, a session is a person's whatever it carries.
pub(crate) async fn permissions(granted: Option<axum::Extension<Granted>>) -> Response {
    let permissions = granted.map_or_else(every, |axum::Extension(Granted(held))| held);
    (StatusCode::OK, axum::Json(permissions_dto(permissions))).into_response()
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

/// `POST /api/v1/tokens`: creates a token with the body's label and
/// permissions, and answers with its whole text, this once.
pub(crate) async fn create(
    State(sign_in): State<SignIn>,
    body: Result<axum::Json<CreateApiTokenDto>, JsonRejection>,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    let Ok(axum::Json(CreateApiTokenDto { label, permissions })) = body else {
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
    let permissions = match token_permissions(permissions) {
        Ok(permissions) => permissions,
        Err(code) => return refused(StatusCode::BAD_REQUEST, code),
    };
    match access.create_api_token(label, permissions).await {
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

/// `PUT /api/v1/tokens/{id}/permissions`: keeps the body's permissions as
/// the token's from now on, with the current admin password, checked in the
/// waits of the connection's address as a sign-in is. A session's only: no
/// token changes its own or another's permissions.
pub(crate) async fn change_permissions(
    State(sign_in): State<SignIn>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    let peer = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied();
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(ChangeApiTokenPermissionsDto {
        permissions,
        current_password,
    }) = serde_json::from_slice(&body)
    else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let permissions = match token_permissions(permissions) {
        Ok(permissions) => permissions,
        Err(code) => return refused(StatusCode::BAD_REQUEST, code),
    };
    let Some(ConnectInfo(crate::PeerAddr(peer))) = peer else {
        log::error!(
            "a change of an API token's permissions arrived without its connection's address; the server must be served with its connect info"
        );
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::SignInUnavailable,
        );
    };
    // Empty, or longer than any admin password: not the admin password.
    let Ok(current) = Password::parse(current_password) else {
        return refused(StatusCode::FORBIDDEN, ErrorCode::CurrentPasswordWrong);
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
    match access
        .change_api_token_permissions(peer.ip(), &current, &token, permissions)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            axum::Json(permissions_dto(permissions.get())),
        )
            .into_response(),
        Ok(false) => unknown(id),
        Err(ApiTokenChangeRefused::Password(refusal)) => password_refused(refusal),
        Err(ApiTokenChangeRefused::Unavailable(detail)) => unavailable(&detail),
    }
}

/// The route of what AI agents may do without a token (decision 127).
pub(crate) const WITHOUT_TOKEN_PATH: &str = "/api/v1/settings/without-token";

/// `GET /api/v1/settings/without-token`: what AI agents may do without a
/// token, every permission at its level or `none`.
pub(crate) async fn without_token(State(without_token): State<Arc<WithoutToken>>) -> Response {
    match without_token.permissions().await {
        Ok(permissions) => {
            (StatusCode::OK, axum::Json(permissions_dto(permissions))).into_response()
        }
        Err(error) => config_refused(error),
    }
}

/// The configuration's refusal, `503` with its `config-*` code.
fn config_refused(error: app::ports::ConfigError) -> Response {
    config_unusable(error, |code, args, detail| {
        refused_detailed(StatusCode::SERVICE_UNAVAILABLE, code, args, detail)
    })
}

/// `PUT /api/v1/settings/without-token`: keeps what AI agents may do
/// without a token and answers it. With sign-in on it takes the current
/// admin password, checked in the waits of the connection's address as a
/// token's change is; with sign-in off there is none to ask for.
pub(crate) async fn change_without_token(
    State(sign_in): State<SignIn>,
    State(without_token): State<Arc<WithoutToken>>,
    request: Request,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied();
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(ChangeWithoutTokenDto {
        permissions,
        current_password,
    }) = serde_json::from_slice(&body)
    else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(permissions) = permissions_of(permissions) else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Some(ConnectInfo(crate::PeerAddr(peer))) = peer else {
        log::error!(
            "a change of what AI agents may do without a token arrived without its connection's address; the server must be served with its connect info"
        );
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::SignInUnavailable,
        );
    };
    let access = match &sign_in {
        SignIn::On(access) => Some(access.as_ref()),
        SignIn::Off { .. } => None,
    };
    let current = match (access, current_password) {
        (None, _) => None,
        // Missing, empty, or longer than any admin password: not the admin
        // password.
        (Some(_), None) => return refused(StatusCode::FORBIDDEN, ErrorCode::CurrentPasswordWrong),
        (Some(_), Some(text)) => match Password::parse(text) {
            Ok(current) => Some(current),
            Err(_) => return refused(StatusCode::FORBIDDEN, ErrorCode::CurrentPasswordWrong),
        },
    };
    match without_token
        .change(access, peer.ip(), current.as_ref(), permissions)
        .await
    {
        Ok(kept) => (StatusCode::OK, axum::Json(permissions_dto(kept))).into_response(),
        Err(WithoutTokenChangeRefused::Password(refusal)) => password_refused(refusal),
        Err(WithoutTokenChangeRefused::Unavailable(error)) => config_refused(error),
    }
}

/// A current admin password that was wrong, met a wait, or couldn't be
/// checked.
fn password_refused(refusal: PasswordRefused) -> Response {
    match refusal {
        PasswordRefused::WrongPassword => {
            refused(StatusCode::FORBIDDEN, ErrorCode::CurrentPasswordWrong)
        }
        PasswordRefused::Wait { seconds } => crate::sign_in::wait_refused(seconds),
        PasswordRefused::Unavailable(detail) => {
            log::warn!("the admin password could not be checked: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::AdminPasswordUnavailable,
            )
        }
    }
}

#[cfg(test)]
mod tests;
