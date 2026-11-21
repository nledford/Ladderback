//! The hub's own routes under `/api/v1/hub` (§16, decisions 120 and 121):
//! the key its page shows, and the pairing tokens the page makes and
//! cancels. Only a build with the hub has them (decision 122), and a daemon
//! not serving as a hub answers them as it does any unknown path.
//!
//! Making a token is a high-impact action: a session's only (no API token
//! permission opens it, so a token is refused before it gets here), confirmed
//! by a fresh step-up each time, and refused from beyond loopback while
//! sign-in is off (decision 120, "Who may make a token, approve or re-key").

use std::sync::Arc;

use api::ErrorCode;
use app::pairing::{HubPairing, HubPairingError, MadeToken, TokenAsk, TokenAskRefused};
use axum::Router;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use domain::SignedDuration;
use domain::access::step_up::StepUpConfirmation;
use domain::fleet::{Fingerprint, HubAddress, KeyId, TokenId, TokenTls};

use crate::AppState;

/// The path of one pairing token the page cancels: `DELETE`.
const PAIRING_TOKEN_PATH: &str = "/api/v1/hub/pairing-tokens/{id}";

/// `routes` with the hub's own.
pub(crate) fn routes(routes: Router<AppState>) -> Router<AppState> {
    routes
        .route(api::HUB_PATH, axum::routing::get(hub))
        .route(api::PAIRING_TOKENS_PATH, axum::routing::post(make_token))
        .route(PAIRING_TOKEN_PATH, axum::routing::delete(cancel_token))
}

/// The hub's pairing, or none when this daemon doesn't serve as a hub.
fn pairing_of(state: &AppState) -> Option<Arc<HubPairing>> {
    state.sync.pairing().cloned()
}

/// The answer a daemon not serving as a hub gives the hub's routes: the
/// one an unknown path gets.
fn not_a_hub() -> Response {
    crate::refused(StatusCode::NOT_FOUND, ErrorCode::RouteUnknown)
}

/// The hub key's fingerprint and key ID, and the certificate a token is
/// offered first: the hub's own when the page came over its built-in TLS,
/// a certificate authority's when it came in plain, as through a proxy.
async fn hub(State(state): State<AppState>, request: Request) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let own = state.sync.certificate();
    let over_tls = request.extensions().get::<crate::BuiltInTls>().is_some();
    let certificate = if over_tls && own.is_some() {
        api::TokenCertificateDto::Pinned
    } else {
        api::TokenCertificateDto::Ca
    };
    match pairing.hub_key().await {
        Ok(key) => axum::Json(api::HubDto {
            fingerprint: Fingerprint::of_key(&key).to_string(),
            key_id: KeyId::of_key(&key).to_string(),
            certificate,
            own_certificate: own.map(|fingerprint| fingerprint.full_hex()),
        })
        .into_response(),
        Err(error) => hub_refusal(&error),
    }
}

/// A pairing token for a [`api::MakePairingTokenDto`], answered 201 with a
/// [`api::PairingTokenDto`].
async fn make_token(State(state): State<AppState>, request: Request) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    // A wrong password counts in the waits of the connection's address, as
    // a sign-in does; without it, refuse as a sign-in does.
    let Some(ConnectInfo(crate::PeerAddr(peer))) = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied()
    else {
        log::error!(
            "a pairing token was asked for without its connection's address; the server must be served with its connect info"
        );
        return crate::unavailable(ErrorCode::SignInUnavailable, Vec::new(), None);
    };
    let beyond = beyond_loopback(request.headers(), peer.ip());
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let Ok(api::MakePairingTokenDto {
        address,
        certificate,
        life_minutes,
        confirmation,
    }) = serde_json::from_slice(&body)
    else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    // A pinned token names the certificate the hub serves: one that
    // serves none has nothing to pin, so a pinned ask is no ask it reads.
    let tls = match certificate {
        api::TokenCertificateDto::Ca => TokenTls::Ca,
        api::TokenCertificateDto::Pinned => match state.sync.certificate() {
            Some(fingerprint) => TokenTls::Pinned(fingerprint),
            None => return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody),
        },
    };
    let Ok(address) = HubAddress::parse(&address) else {
        return crate::refused(StatusCode::BAD_REQUEST, ErrorCode::HubAddressInvalid);
    };
    let ask = TokenAsk {
        address,
        tls,
        life: life_minutes.map(|minutes| SignedDuration::from_mins(i64::from(minutes))),
        peer: peer.ip(),
        beyond_loopback: beyond,
        // Checked after the sign-in rule: beyond loopback with sign-in off,
        // no confirmation helps.
        confirmation: StepUpConfirmation::parse(confirmation).ok(),
    };
    match pairing.make_token_asked(ask).await {
        Ok(made) => (StatusCode::CREATED, axum::Json(token_dto(&made))).into_response(),
        Err(TokenAskRefused::NeedsSignIn) => {
            crate::refused(StatusCode::FORBIDDEN, ErrorCode::PairingNeedsSignIn)
        }
        Err(TokenAskRefused::StepUpFailed) => {
            crate::refused(StatusCode::FORBIDDEN, ErrorCode::StepUpFailed)
        }
        Err(TokenAskRefused::StepUpWait { seconds }) => crate::sign_in::wait_refused(seconds),
        Err(TokenAskRefused::StepUpUnavailable(detail)) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be checked: {detail}");
            crate::unavailable(ErrorCode::SignInUnavailable, Vec::new(), None)
        }
        Err(TokenAskRefused::Hub(error)) => hub_refusal(&error),
    }
}

/// The token as the page shows it, once. Its life left is the hub's own
/// count, which the page counts down on its monotonic clock, so a browser
/// whose clock is wrong still shows it right.
fn token_dto(made: &MadeToken) -> api::PairingTokenDto {
    api::PairingTokenDto {
        token: made.token.to_text().expose().to_owned(),
        token_id: made.id.to_string(),
        fingerprint: Fingerprint::of_key(&made.hub_key).to_string(),
        address: made.token.address().as_str().to_owned(),
        expires_at: made.expires_at.to_string(),
        seconds_left: u64::try_from(made.left.as_secs()).unwrap_or(0),
        certificate: match made.token.tls() {
            TokenTls::Pinned(_) => api::TokenCertificateDto::Pinned,
            TokenTls::Ca => api::TokenCertificateDto::Ca,
        },
    }
}

/// Cancels the unused token `id`: 204, or 404 `pairing-token-unknown` when
/// none is waiting by that ID.
async fn cancel_token(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(pairing) = pairing_of(&state) else {
        return not_a_hub();
    };
    let unknown = || crate::refused(StatusCode::NOT_FOUND, ErrorCode::PairingTokenUnknown);
    let Some(id) = TokenId::parse(&id) else {
        return unknown();
    };
    match pairing.revoke_token(id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => unknown(),
        Err(error) => hub_refusal(&error),
    }
}

/// What the hub couldn't do, by its kind; what it couldn't read or keep is
/// told in the daemon's log, not to whoever asked.
fn hub_refusal(error: &HubPairingError) -> Response {
    match error {
        HubPairingError::TokenLife => {
            crate::refused(StatusCode::BAD_REQUEST, ErrorCode::PairingTokenLife)
        }
        HubPairingError::TooManyUnusedTokens => {
            crate::refused(StatusCode::CONFLICT, ErrorCode::PairingTokenLimit)
        }
        other => {
            log::error!("the hub could not make or cancel a pairing token: {other:?}");
            crate::unavailable(ErrorCode::HubUnavailable, Vec::new(), None)
        }
    }
}

/// Whether a request came from beyond loopback: its `Host` isn't a loopback
/// name, or its connection's peer isn't a loopback address. A proxy on
/// this machine forwarding a remote browser shows a loopback peer, so the
/// `Host` it forwards counts too.
fn beyond_loopback(headers: &HeaderMap, peer: std::net::IpAddr) -> bool {
    !(peer.is_loopback() && crate::request_checks::host_is_loopback(headers))
}

#[cfg(test)]
mod tests;
