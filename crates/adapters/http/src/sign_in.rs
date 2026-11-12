//! Sign-in (technical design §10.1, decision 70): the session route, and the
//! layer that will ask an API request for a live session.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use api::{ErrorCode, MsgArg, SessionDto, SignInDto, SignInStateDto};
use app::access::{Access, SignInRefused};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{COOKIE, HOST, ORIGIN, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::SignedDuration;
use domain::access::{Password, SESSION_IDLE, SESSION_LIFETIME, Token};

use crate::{refused, refused_with};

/// The session cookie's name.
pub const COOKIE_NAME: &str = "ladderback-session";

/// The header a changed request carries its session's CSRF token in.
pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");

/// The route that tells a page where it stands, signs in and signs out.
pub(crate) const SESSION_PATH: &str = "/api/v1/session";

/// The events stream's route.
pub(crate) const EVENTS_PATH: &str = "/api/v1/events";

/// Whether sign-in is on.
#[derive(Clone, Default)]
pub enum SignIn {
    #[default]
    Off,
    On(Arc<Access>),
}

/// A request that reached its route with a live session.
#[derive(Clone)]
pub(crate) struct SessionOf {
    pub(crate) access: Arc<Access>,
    pub(crate) token: Token,
}

/// Lets every request through, for now.
pub(crate) async fn enforce(
    State(_sign_in): State<SignIn>,
    request: Request,
    next: Next,
) -> Response {
    next.run(request).await
}

pub(crate) async fn current(State(_sign_in): State<SignIn>, _headers: HeaderMap) -> Response {
    todo!()
}

pub(crate) async fn sign_in(State(_sign_in): State<SignIn>, _request: Request) -> Response {
    todo!()
}

pub(crate) async fn sign_out(State(_sign_in): State<SignIn>, _headers: HeaderMap) -> Response {
    todo!()
}

fn over_https(_headers: &HeaderMap) -> bool {
    todo!()
}

#[cfg(test)]
mod tests;
