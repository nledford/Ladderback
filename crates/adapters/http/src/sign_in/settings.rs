//! The sign-in settings over HTTP (technical design §10.1): **Stay signed in
//! for**, the session length. Only a session reads or changes them: the
//! scope table names no scope for the route, so an API token is refused, and
//! a change carries the session's CSRF token like any other (`enforce`).

use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::response::Response;

use api::SignInSettingsDto;

use super::SignIn;

/// The sign-in settings' route.
pub(crate) const SETTINGS_PATH: &str = "/api/v1/settings/sign-in";

/// `GET /api/v1/settings/sign-in`: the session length.
pub(crate) async fn read(State(sign_in): State<SignIn>) -> Response {
    let _ = sign_in;
    todo!()
}

/// `PUT /api/v1/settings/sign-in`: sets the session length, which ends at
/// once every session older than it.
pub(crate) async fn change(
    State(sign_in): State<SignIn>,
    body: Result<axum::Json<SignInSettingsDto>, JsonRejection>,
) -> Response {
    let _ = (sign_in, body);
    todo!()
}
