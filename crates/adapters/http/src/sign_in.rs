//! Sign-in (technical design §10.1, decision 70). With an admin password set,
//! every `/api/v1` request but the session route's own carries a live
//! session's cookie, and a changed one carries that session's CSRF token too;
//! the web UI's files are served without one, so the page can ask for the
//! password. With none set, sign-in is off and this layer lets every request
//! through to the request checks' rules alone.
//!
//! The layer runs after the request checks and before the body limit: a
//! request from another site or for another host is refused before its
//! cookie is looked at, and a request with no session never has its body
//! read.

use std::sync::Arc;

use api::{ErrorCode, ExposureDto, MsgArg, SessionDto, SignInDto, SignInStateDto};
use app::access::{Access, SignInRefused};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{COOKIE, HOST, ORIGIN, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::SignedDuration;
use domain::access::{Password, SESSION_IDLE, SESSION_LIFETIME, Token};

use crate::{Exposure, refused, refused_with};

/// The session cookie's name.
pub const COOKIE_NAME: &str = "ladderback-session";

/// The header a changed request carries its session's CSRF token in.
pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");

/// The route that tells a page where it stands, signs in and signs out. It
/// answers without a session: it is how one starts.
pub(crate) const SESSION_PATH: &str = "/api/v1/session";

/// The events stream's route. Its stream stays open while a page does, so
/// opening it is never a use of the session.
pub(crate) const EVENTS_PATH: &str = "/api/v1/events";

/// The header a request the page sent on its own carries.
const BACKGROUND: HeaderName = HeaderName::from_static(api::BACKGROUND_HEADER);

/// Whether sign-in is on, decided when the daemon starts: on when an admin
/// password is kept, with the sessions it has started.
#[derive(Clone, Default)]
pub enum SignIn {
    #[default]
    Off,
    On(Arc<Access>),
}

/// A request that reached its route with a live session: the events stream
/// keeps it, to end once the session does.
#[derive(Clone)]
pub(crate) struct SessionOf {
    pub(crate) access: Arc<Access>,
    pub(crate) token: Token,
}

/// Whether `path` is the API's, `/api/v1` itself and paths no route serves
/// included: the API never answers there without a session.
fn is_api(path: &str) -> bool {
    path.strip_prefix("/api/v1")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn reads_only(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD || method == Method::OPTIONS
}

/// The session cookie's token, if the request carries one that is a token.
fn session_token(headers: &HeaderMap) -> Option<Token> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .and_then(|(_, value)| Token::parse(value.trim()))
}

/// Whether a request is one no person caused: the events stream's handshake,
/// or a request the page marked as sent on its own. Neither uses the
/// session, so a page left open never keeps one alive (§10.1).
fn on_its_own(request: &Request) -> bool {
    request.uri().path() == EVENTS_PATH || request.headers().contains_key(&BACKGROUND)
}

/// The CSRF token of the request's header, when it carries exactly one.
fn csrf_token(headers: &HeaderMap) -> Option<Token> {
    let mut values = headers.get_all(&CSRF_HEADER).iter();
    match (values.next(), values.next()) {
        (Some(value), None) => Token::parse(value.to_str().ok()?),
        _ => None,
    }
}

/// Refuses an API request that has no live session, or a changed one
/// without its session's CSRF token, before any route sees it.
pub(crate) async fn enforce(
    State(sign_in): State<SignIn>,
    mut request: Request,
    next: Next,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return next.run(request).await;
    };
    let path = request.uri().path();
    if !is_api(path) || path == SESSION_PATH {
        return next.run(request).await;
    }
    let Some(token) = session_token(request.headers()) else {
        return refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInRequired);
    };
    let session = if on_its_own(&request) {
        access.peek(&token)
    } else {
        access.session(&token)
    };
    let Some(session) = session else {
        return refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInRequired);
    };
    if !reads_only(request.method())
        && !csrf_token(request.headers()).is_some_and(|presented| session.accepts(&presented))
    {
        return refused(StatusCode::FORBIDDEN, ErrorCode::CsrfTokenInvalid);
    }
    request.extensions_mut().insert(SessionOf { access, token });
    next.run(request).await
}

/// `GET /api/v1/session`: whether sign-in is on, and whether the request's
/// session is live, with its CSRF token when it is.
pub(crate) async fn current(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    headers: HeaderMap,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return answer(SignInStateDto::Off, None, exposure);
    };
    match session_token(&headers).and_then(|token| access.session(&token)) {
        Some(session) => answer(
            SignInStateDto::SignedIn,
            Some((session.csrf().to_text(), session.ends_in())),
            exposure,
        ),
        None => answer(SignInStateDto::SignedOut, None, exposure),
    }
}

/// `POST /api/v1/session`: signs in with the admin password. A session's
/// cookie can't be read by the page's scripts and is never sent by another
/// site; it is marked `Secure` when the page that sent the request was served
/// over HTTPS, as through a TLS-terminating proxy.
pub(crate) async fn sign_in(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    request: Request,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return answer(SignInStateDto::Off, None, exposure);
    };
    // Each client's sign-ins wait on their own, keyed by the connection's
    // address (§10.1). A server that didn't hand it over would put every
    // client in one wait, which one client could keep at its longest for
    // all: refuse, and say so to whoever runs the daemon.
    let Some(ConnectInfo(crate::PeerAddr(peer))) = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied()
    else {
        log::error!(
            "a sign-in arrived without its connection's address; the server must be served with its connect info"
        );
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::SignInUnavailable,
        );
    };
    let address = peer.ip();
    let secure = over_https(request.headers());
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let body = match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(_) => return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody),
    };
    let Ok(SignInDto { password }) = serde_json::from_slice::<SignInDto>(&body) else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    // Empty, or longer than any admin password: not the admin password,
    // and not worth a hash.
    let Ok(password) = Password::parse(password) else {
        return refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInFailed);
    };
    match access.sign_in(address, &password).await {
        Ok(signed_in) => {
            // A session that has just started has all its lifetime left.
            let mut response = answer(
                SignInStateDto::SignedIn,
                Some((signed_in.csrf.to_text(), SESSION_LIFETIME)),
                exposure,
            );
            let cookie = format!(
                "{COOKIE_NAME}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Strict{}",
                signed_in.token.to_text(),
                SESSION_LIFETIME.as_secs(),
                if secure { "; Secure" } else { "" }
            );
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(SET_COOKIE, value);
            }
            response
        }
        Err(SignInRefused::WrongPassword) => {
            refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInFailed)
        }
        Err(SignInRefused::Wait { seconds }) => {
            let mut response = refused_with(
                StatusCode::TOO_MANY_REQUESTS,
                ErrorCode::SignInWait,
                vec![("seconds", MsgArg::Count(seconds))],
            );
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from(seconds));
            response
        }
        Err(SignInRefused::Unavailable(detail)) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be checked: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::SignInUnavailable,
            )
        }
    }
}

/// `DELETE /api/v1/session`: signs out, and has the browser forget the
/// cookie. A changed request like any other, it carries the session's CSRF
/// token, so another site can't sign the admin out. A request whose session
/// has already ended has nothing to end, and is answered as signed out.
pub(crate) async fn sign_out(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    headers: HeaderMap,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return answer(SignInStateDto::Off, None, exposure);
    };
    if let Some(token) = session_token(&headers)
        && let Some(session) = access.peek(&token)
    {
        if !csrf_token(&headers).is_some_and(|presented| session.accepts(&presented)) {
            return refused(StatusCode::FORBIDDEN, ErrorCode::CsrfTokenInvalid);
        }
        access.sign_out(&token);
    }
    let mut response = answer(SignInStateDto::SignedOut, None, exposure);
    let forget = format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict");
    if let Ok(value) = HeaderValue::from_str(&forget) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

/// Whether the request came from a page served over HTTPS: its `Origin` is
/// `https://` and its own `Host`. A browser sets both, and the request checks
/// have already refused an `Origin` of another host; a client that writes
/// them itself only marks its own cookie.
fn over_https(headers: &HeaderMap) -> bool {
    let (Some(origin), Some(host)) = (headers.get(ORIGIN), headers.get(HOST)) else {
        return false;
    };
    let (Ok(origin), Ok(host)) = (origin.to_str(), host.to_str()) else {
        return false;
    };
    origin
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        && origin
            .get(8..)
            .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

/// The session route's answer: where the request stands, who can reach the
/// daemon (so the page can say who can use it with sign-in off) and, for a
/// live session, its CSRF token and how long it has left however it is used.
fn answer(
    sign_in: SignInStateDto,
    live: Option<(String, SignedDuration)>,
    exposure: Exposure,
) -> Response {
    let (csrf_token, idle_seconds, ends_in_seconds) = match live {
        Some((csrf, ends_in)) => (
            Some(csrf),
            Some(whole_seconds(SESSION_IDLE)),
            Some(whole_seconds(ends_in)),
        ),
        None => (None, None, None),
    };
    (
        StatusCode::OK,
        axum::Json(SessionDto {
            sign_in,
            csrf_token,
            idle_seconds,
            ends_in_seconds,
            exposure: match exposure {
                Exposure::Loopback => ExposureDto::Loopback,
                Exposure::Network => ExposureDto::Network,
            },
        }),
    )
        .into_response()
}

/// `duration` in whole seconds, rounded down; none when it has passed.
fn whole_seconds(duration: SignedDuration) -> u64 {
    u64::try_from(duration.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests;
