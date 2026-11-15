//! The sign-in settings over HTTP (technical design §10.1): **Stay signed in
//! for**, the session length. Only a session reads or changes them: the
//! scope table names no scope for the route, so an API token is refused, and
//! a change carries the session's CSRF token like any other (`enforce`).

use api::{ErrorCode, SessionLengthDto, SignInSettingsDto};
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::access::SessionLength;

use super::SignIn;
use crate::refused;

/// The sign-in settings' route.
pub(crate) const SETTINGS_PATH: &str = "/api/v1/settings/sign-in";

/// `GET /api/v1/settings/sign-in`: the session length.
pub(crate) async fn read(State(sign_in): State<SignIn>) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    answer(access.session_length())
}

/// `PUT /api/v1/settings/sign-in`: sets the session length, which ends at
/// once every session older than it. One the store can't keep changes
/// nothing.
pub(crate) async fn change(
    State(sign_in): State<SignIn>,
    body: Result<axum::Json<SignInSettingsDto>, JsonRejection>,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return sign_in_off();
    };
    let Ok(axum::Json(SignInSettingsDto { session_length })) = body else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let length = length_of(session_length);
    match access.set_session_length(length).await {
        Ok(()) => answer(length),
        Err(error) => super::sessions_unavailable(&format!(
            "the session length could not be kept in the database: {error}"
        )),
    }
}

fn sign_in_off() -> Response {
    refused(StatusCode::CONFLICT, ErrorCode::SignInOff)
}

fn answer(length: SessionLength) -> Response {
    (
        StatusCode::OK,
        axum::Json(SignInSettingsDto {
            session_length: dto_of(length),
        }),
    )
        .into_response()
}

fn length_of(dto: SessionLengthDto) -> SessionLength {
    match dto {
        SessionLengthDto::UntilBrowserCloses => SessionLength::UntilBrowserCloses,
        SessionLengthDto::SevenDays => SessionLength::SevenDays,
        SessionLengthDto::ThirtyDays => SessionLength::ThirtyDays,
        SessionLengthDto::NinetyDays => SessionLength::NinetyDays,
        SessionLengthDto::OneYear => SessionLength::OneYear,
        SessionLengthDto::UntilSignOut => SessionLength::UntilSignOut,
    }
}

fn dto_of(length: SessionLength) -> SessionLengthDto {
    match length {
        SessionLength::UntilBrowserCloses => SessionLengthDto::UntilBrowserCloses,
        SessionLength::SevenDays => SessionLengthDto::SevenDays,
        SessionLength::ThirtyDays => SessionLengthDto::ThirtyDays,
        SessionLength::NinetyDays => SessionLengthDto::NinetyDays,
        SessionLength::OneYear => SessionLengthDto::OneYear,
        SessionLength::UntilSignOut => SessionLengthDto::UntilSignOut,
    }
}
