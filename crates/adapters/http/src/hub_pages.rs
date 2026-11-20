//! The hub's own routes under `/api/v1/hub` (§16, decisions 120 and 121):
//! the key its page shows, and the pairing tokens the page makes and
//! cancels. Only a build with the hub has them (decision 122), and a daemon
//! not serving as a hub answers them as it does any unknown path.
//!
//! Making a token is a high-impact action: a session's only (no API token
//! permission opens it, so a token is refused before it gets here), confirmed
//! by a fresh step-up each time, and refused from beyond loopback while
//! sign-in is off (decision 120, "Who may make a token, approve or re-key").

use axum::Router;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

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

/// The hub key's fingerprint and key ID.
async fn hub(State(state): State<AppState>) -> Response {
    let _ = state;
    todo!()
}

/// A pairing token for a [`api::MakePairingTokenDto`], answered 201 with a
/// [`api::PairingTokenDto`].
async fn make_token(State(state): State<AppState>, request: Request) -> Response {
    let _ = (
        state,
        request.extensions().get::<ConnectInfo<crate::PeerAddr>>(),
    );
    todo!()
}

/// Cancels the unused token `id`: 204, or 404 `pairing-token-unknown` when
/// none is waiting by that ID.
async fn cancel_token(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let _ = (state, id);
    todo!()
}

/// Whether a request came from beyond loopback: its `Host` isn't a loopback
/// name, or its connection's peer isn't a loopback address. A proxy on
/// this machine forwarding a remote browser shows a loopback peer, so the
/// `Host` it forwards counts too.
fn beyond_loopback(headers: &HeaderMap, peer: std::net::IpAddr) -> bool {
    let _ = (headers, peer, StatusCode::OK);
    todo!()
}

#[cfg(test)]
mod tests;
