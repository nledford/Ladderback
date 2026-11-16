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

use api::{
    AdminPasswordDto, ErrorCode, ExposureDto, MsgArg, SessionDto, SetAdminPasswordDto, SignInDto,
    SignInStateDto,
};
use app::access::{
    Access, PasswordChangeRefused, PasswordSet, PasswordWaiting, SetPasswordRefused, SignInRefused,
    set_admin_password,
};
use app::ports::{AdminPassword, SessionStore};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{COOKIE, HOST, ORIGIN, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use domain::SignedDuration;
use domain::access::{CookieLife, PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARS, Password, Token};

use crate::{Exposure, Running, refused, refused_with};

/// The session cookie's name.
pub const COOKIE_NAME: &str = "ladderback-session";

/// The header a changed request carries its session's CSRF token in.
pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");

/// The route that tells a page where it stands, signs in and signs out. It
/// answers without a session: it is how one starts.
pub(crate) const SESSION_PATH: &str = "/api/v1/session";

/// The route that signs out everywhere: every browser's session ends. Only
/// a session reaches it, with its CSRF token, like any other change.
pub(crate) const SESSIONS_PATH: &str = "/api/v1/sessions";

/// The events stream's route. Its stream stays open while a page does, so
/// opening it is never a use of the session.
pub(crate) const EVENTS_PATH: &str = "/api/v1/events";

/// The route that sets and changes the admin password.
pub(crate) const ADMIN_PASSWORD_PATH: &str = "/api/v1/admin-password";

/// The header a request the page sent on its own carries.
const BACKGROUND: HeaderName = HeaderName::from_static(api::BACKGROUND_HEADER);

/// Whether sign-in is on, decided when the daemon starts: on when an admin
/// password is kept, with the sessions it has started; off with where one
/// would be kept, which the session route looks at to say that one set since
/// waits for a restart (decision 70), and the store of the sessions kept
/// from when it was on, which a password set then has forget them.
#[derive(Clone)]
pub enum SignIn {
    Off {
        password: Arc<dyn AdminPassword>,
        sessions: Arc<dyn SessionStore>,
    },
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
    if !is_api(path) {
        return next.run(request).await;
    }
    // A request with a Bearer token is judged by its token alone, its
    // cookie ignored, the session route's included (decision 93).
    if let Some(presented) = crate::api_tokens::bearer(request.headers()) {
        return crate::api_tokens::enforce(access, presented, request, next).await;
    }
    if path == SESSION_PATH {
        return next.run(request).await;
    }
    let Some(token) = session_token(request.headers()) else {
        return refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInRequired);
    };
    let session = if on_its_own(&request) {
        access.peek(&token)
    } else {
        access.session(&token).await
    };
    let Some(session) = session else {
        return refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInRequired);
    };
    // A use that renews the cookie says so on whatever answers it, unless
    // the route sets the cookie itself, as a new admin password forgets it.
    // `Secure` is as the sign-in set it: a same-origin GET has no `Origin`.
    let renewed = session
        .renewed_cookie()
        .and_then(|life| session_cookie(&token, life, session.secure()));
    let mut response = if !reads_only(request.method())
        && !csrf_token(request.headers()).is_some_and(|presented| session.accepts(&presented))
    {
        refused(StatusCode::FORBIDDEN, ErrorCode::CsrfTokenInvalid)
    } else {
        request.extensions_mut().insert(SessionOf { access, token });
        next.run(request).await
    };
    if let Some(cookie) = renewed
        && !response.headers().contains_key(SET_COOKIE)
    {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}

/// The session cookie of `token`, lasting `life`. It can't be read by the
/// page's scripts and is never sent by another site; it is marked `Secure`
/// when `secure`.
fn session_cookie(token: &Token, life: CookieLife, secure: bool) -> Option<HeaderValue> {
    let max_age = match life {
        CookieLife::UntilBrowserCloses => String::new(),
        CookieLife::Lasts(lasts) => format!("; Max-Age={}", whole_seconds(lasts)),
    };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={}; Path=/{max_age}; HttpOnly; SameSite=Strict{}",
        token.to_text(),
        if secure { "; Secure" } else { "" }
    ))
    .ok()
}

/// `GET /api/v1/session`: whether sign-in is on, and whether the request's
/// session is live, with its CSRF token when it is, and the cookie renewed
/// when asking renewed it; a read the page sends on its own only looks, as
/// at every route. A signed-out answer says whether the sign-in form offers
/// Keep me signed in on this device. A request with a `read` token is judged
/// by the token alone, its cookie ignored (decision 93): it has no session,
/// and is told the build, as a signed-in page is.
pub(crate) async fn current(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    State(running): State<Running>,
    api_token: Option<axum::Extension<crate::api_tokens::TokenOf>>,
    headers: HeaderMap,
) -> Response {
    let access = match sign_in {
        SignIn::On(access) => access,
        SignIn::Off { password, .. } => return answer_off(password.as_ref(), exposure, running),
    };
    if api_token.is_some() {
        return answer_told(
            SignInStateDto::SignedOut,
            None,
            None,
            None,
            exposure,
            Some(running),
        );
    }
    let token = session_token(&headers);
    let live = match &token {
        Some(token) if headers.contains_key(&BACKGROUND) => access.peek(token),
        Some(token) => access.session(token).await,
        None => None,
    };
    let (Some(session), Some(token)) = (live, token) else {
        return answer_signed_out(&access, exposure, running);
    };
    let mut response = answer(
        SignInStateDto::SignedIn,
        Some(LiveAnswer {
            csrf: session.csrf().to_text(),
            ends_in: session.ends_in(),
            idle_limit: session.idle_limit(),
        }),
        None,
        None,
        exposure,
        running,
    );
    if let Some(cookie) = session
        .renewed_cookie()
        .and_then(|life| session_cookie(&token, life, session.secure()))
    {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}

/// The session route's answer to a page with no live session, saying
/// whether the sign-in form offers Keep me signed in on this device.
fn answer_signed_out(access: &Access, exposure: Exposure, running: Running) -> Response {
    answer(
        SignInStateDto::SignedOut,
        None,
        None,
        Some(access.offers_keep_signed_in()),
        exposure,
        running,
    )
}

/// `POST /api/v1/session`: signs in with the admin password. A session's
/// cookie can't be read by the page's scripts and is never sent by another
/// site; it is marked `Secure` when the page that sent the request was served
/// over HTTPS, as through a TLS-terminating proxy, and stays so at every
/// renewal.
pub(crate) async fn sign_in(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    State(running): State<Running>,
    request: Request,
) -> Response {
    let access = match sign_in {
        SignIn::On(access) => access,
        SignIn::Off { password, .. } => return answer_off(password.as_ref(), exposure, running),
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
    let Ok(SignInDto {
        password,
        keep_signed_in,
    }) = serde_json::from_slice::<SignInDto>(&body)
    else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    // Empty, or longer than any admin password: not the admin password,
    // and not worth a hash.
    let Ok(password) = Password::parse(password) else {
        return refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInFailed);
    };
    match access
        .sign_in(address, &password, keep_signed_in, secure)
        .await
    {
        Ok(signed_in) => {
            let mut response = answer(
                SignInStateDto::SignedIn,
                Some(LiveAnswer {
                    csrf: signed_in.csrf.to_text(),
                    ends_in: signed_in.ends_in,
                    idle_limit: signed_in.idle_limit,
                }),
                None,
                None,
                exposure,
                running,
            );
            if let Some(cookie) = session_cookie(&signed_in.token, signed_in.cookie, secure) {
                response.headers_mut().insert(SET_COOKIE, cookie);
            }
            response
        }
        Err(SignInRefused::WrongPassword) => {
            refused(StatusCode::UNAUTHORIZED, ErrorCode::SignInFailed)
        }
        Err(SignInRefused::Wait { seconds }) => wait_refused(seconds),
        Err(SignInRefused::Unavailable(detail)) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be checked: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::SignInUnavailable,
            )
        }
        Err(SignInRefused::Store(error)) => sessions_unavailable(&format!(
            "a session could not be kept in the database: {error}"
        )),
    }
}

/// `DELETE /api/v1/session`: signs out, and has the browser forget the
/// cookie. A changed request like any other, it carries the session's CSRF
/// token, so another site can't sign the admin out. A request whose session
/// has already ended has nothing to end, and is answered as signed out.
pub(crate) async fn sign_out(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    State(running): State<Running>,
    headers: HeaderMap,
) -> Response {
    let access = match sign_in {
        SignIn::On(access) => access,
        SignIn::Off { password, .. } => return answer_off(password.as_ref(), exposure, running),
    };
    if let Some(token) = session_token(&headers)
        && let Some(session) = access.peek(&token)
    {
        if !csrf_token(&headers).is_some_and(|presented| session.accepts(&presented)) {
            return refused(StatusCode::FORBIDDEN, ErrorCode::CsrfTokenInvalid);
        }
        if let Err(error) = access.sign_out(&token).await {
            // The session has ended here and the browser forgets its cookie,
            // but the store still holds it until its next write succeeds,
            // and a restart before then would bring it back: say so.
            let mut response = sessions_unavailable(&format!(
                "a signed-out session could not be forgotten in the database: {error}"
            ));
            forget_cookie(&mut response);
            return response;
        }
    }
    let mut response = answer_signed_out(&access, exposure, running);
    forget_cookie(&mut response);
    response
}

/// `DELETE /api/v1/sessions`: Sign out everywhere. Every browser's session
/// ends, the one that asked included, and the store forgets them all; the
/// browser that asked forgets its cookie. API tokens go on. Only a live
/// session with its CSRF token reaches here (`enforce`); with sign-in off
/// there is no session to end.
pub(crate) async fn sign_out_everywhere(
    State(sign_in): State<SignIn>,
    State(exposure): State<Exposure>,
    State(running): State<Running>,
) -> Response {
    let SignIn::On(access) = sign_in else {
        return refused(StatusCode::CONFLICT, ErrorCode::SignInOff);
    };
    let mut response = match access.sign_out_everywhere().await {
        Ok(()) => answer_signed_out(&access, exposure, running),
        Err(error) => {
            // Every session has ended here, but the store still holds them
            // until its next write succeeds, and a restart before then
            // would bring them back: say so.
            sessions_unavailable(&format!(
                "the sessions signed out everywhere could not be forgotten in the database: {error}"
            ))
        }
    };
    forget_cookie(&mut response);
    response
}

/// The answer when the database could not keep or forget sessions: `line`,
/// which names the database, goes to the log for whoever runs the daemon,
/// not to whoever asked.
pub(crate) fn sessions_unavailable(line: &str) -> Response {
    log::warn!("{line}");
    refused(
        StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::SessionsUnavailable,
    )
}

/// Has the browser forget the session cookie.
fn forget_cookie(response: &mut Response) {
    let forget = format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict");
    if let Ok(value) = HeaderValue::from_str(&forget) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
}

/// `POST /api/v1/admin-password`: sets the admin password by the rules
/// `ladderback set-password` keeps (decision 70). With sign-in off it is
/// set, and waits for a restart to turn sign-in on: whoever can reach the
/// page can already run and change everything. With sign-in on, only a
/// session reaches here (an API token is refused before, as for every
/// route only a session may use), and it changes the password only with
/// the current one, checked as a sign-in from the connection's address is:
/// in its waits, a wrong one counted. A changed password ends every
/// session and API token, the caller's included, so the browser is told to
/// forget its cookie. Neither password is ever logged or answered (I4).
pub(crate) async fn set_password(State(sign_in): State<SignIn>, request: Request) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied();
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let body = match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(_) => return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody),
    };
    let Ok(SetAdminPasswordDto {
        current_password,
        new_password,
    }) = serde_json::from_slice::<SetAdminPasswordDto>(&body)
    else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let access = match sign_in {
        SignIn::On(access) => access,
        SignIn::Off { password, sessions } => {
            return match set_admin_password(password.as_ref(), sessions.as_ref(), new_password)
                .await
            {
                Ok(set) => {
                    say_sessions_left(set);
                    StatusCode::NO_CONTENT.into_response()
                }
                Err(refusal) => set_refused(refusal),
            };
        }
    };
    // The current password counts in the waits of the connection's address,
    // as a sign-in does; without it, refuse as `sign_in` does.
    let Some(ConnectInfo(crate::PeerAddr(peer))) = peer else {
        log::error!(
            "a change of the admin password arrived without its connection's address; the server must be served with its connect info"
        );
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::SignInUnavailable,
        );
    };
    // None, empty, or longer than any admin password: not the admin
    // password, and not worth a hash, as with a sign-in.
    let Some(current) = current_password.and_then(|current| Password::parse(current).ok()) else {
        return refused(StatusCode::FORBIDDEN, ErrorCode::CurrentPasswordWrong);
    };
    match access
        .change_password(peer.ip(), &current, new_password)
        .await
    {
        Ok(set) => {
            say_sessions_left(set);
            let mut response = StatusCode::NO_CONTENT.into_response();
            forget_cookie(&mut response);
            response
        }
        Err(PasswordChangeRefused::Current(SignInRefused::WrongPassword)) => {
            refused(StatusCode::FORBIDDEN, ErrorCode::CurrentPasswordWrong)
        }
        Err(PasswordChangeRefused::Current(SignInRefused::Wait { seconds })) => {
            wait_refused(seconds)
        }
        Err(PasswordChangeRefused::Current(SignInRefused::Unavailable(detail))) => {
            log::warn!("the admin password could not be checked: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::AdminPasswordUnavailable,
            )
        }
        // Not met: checking the current password keeps no session. Answered
        // as a sign-in's would be.
        Err(PasswordChangeRefused::Current(SignInRefused::Store(error))) => sessions_unavailable(
            &format!("a session could not be kept in the database: {error}"),
        ),
        Err(PasswordChangeRefused::New(refusal)) => set_refused(refusal),
    }
}

/// Logs a new admin password whose store couldn't forget the sessions kept
/// from before it: they have ended all the same, by the fingerprint, and the
/// next start forgets them.
fn say_sessions_left(set: PasswordSet) {
    if let PasswordSet::SessionsLeft(error) = set {
        log::warn!(
            "the sessions kept from before the new admin password could not be forgotten; they have ended all the same, and the next start forgets them: {error}"
        );
    }
}

/// The answer to a new admin password that was not set.
fn set_refused(refusal: SetPasswordRefused) -> Response {
    match refusal {
        SetPasswordRefused::TooShort => refused_with(
            StatusCode::BAD_REQUEST,
            ErrorCode::AdminPasswordTooShort,
            vec![(
                "min",
                MsgArg::Count(u64::try_from(PASSWORD_MIN_CHARS).unwrap_or(u64::MAX)),
            )],
        ),
        SetPasswordRefused::TooLong => refused_with(
            StatusCode::BAD_REQUEST,
            ErrorCode::AdminPasswordTooLong,
            vec![(
                "max",
                MsgArg::Count(u64::try_from(PASSWORD_MAX_BYTES).unwrap_or(u64::MAX)),
            )],
        ),
        SetPasswordRefused::Unwritable(detail) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be kept: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::AdminPasswordUnavailable,
            )
        }
    }
}

/// `429 sign-in-wait`, with the seconds left in its `Retry-After`.
fn wait_refused(seconds: u64) -> Response {
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

/// The session route's answer with sign-in off: an admin password kept since
/// the daemon started, looked for now, waits for the next start to turn
/// sign-in on (decision 70).
fn answer_off(kept: &dyn AdminPassword, exposure: Exposure, running: Running) -> Response {
    let waiting = PasswordWaiting::of(kept).map(|waiting| match waiting {
        PasswordWaiting::Set => AdminPasswordDto::Set,
        PasswordWaiting::Unreadable => AdminPasswordDto::Unreadable,
    });
    answer(SignInStateDto::Off, None, waiting, None, exposure, running)
}

/// What the session route says of a live session: its CSRF token, how long
/// it has left however it is used, and how long it may go unused; `None`
/// for either when nothing limits it so.
struct LiveAnswer {
    csrf: String,
    ends_in: Option<SignedDuration>,
    idle_limit: Option<SignedDuration>,
}

/// The session route's answer: where the request stands, who can reach the
/// daemon (so the page can say who can use it with sign-in off), for a live
/// session what [`LiveAnswer`] holds, with sign-in off an admin password
/// that waits for a restart, signed out whether the sign-in form offers Keep
/// me signed in on this device, and, for a page that may see the pages,
/// what it is told of the daemon that runs.
fn answer(
    sign_in: SignInStateDto,
    live: Option<LiveAnswer>,
    admin_password: Option<AdminPasswordDto>,
    keep_signed_in_offered: Option<bool>,
    exposure: Exposure,
    running: Running,
) -> Response {
    // A page yet to sign in isn't told the build, as it isn't shown the
    // version (#502), nor the available update.
    let running =
        matches!(sign_in, SignInStateDto::Off | SignInStateDto::SignedIn).then_some(running);
    answer_told(
        sign_in,
        live,
        admin_password,
        keep_signed_in_offered,
        exposure,
        running,
    )
}

/// [`answer`], telling the build and the available update when `running`
/// is some.
fn answer_told(
    sign_in: SignInStateDto,
    live: Option<LiveAnswer>,
    admin_password: Option<AdminPasswordDto>,
    keep_signed_in_offered: Option<bool>,
    exposure: Exposure,
    running: Option<Running>,
) -> Response {
    let (build, available_update) = match running {
        Some(running) => (Some(running.build), running.update),
        None => (None, None),
    };
    let (csrf_token, idle_seconds, ends_in_seconds) = match live {
        Some(live) => (
            Some(live.csrf),
            live.idle_limit.map(whole_seconds),
            live.ends_in.map(whole_seconds),
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
            admin_password,
            keep_signed_in_offered,
            build,
            available_update,
        }),
    )
        .into_response()
}

/// `duration` in whole seconds, rounded down; none when it has passed.
fn whole_seconds(duration: SignedDuration) -> u64 {
    u64::try_from(duration.as_secs()).unwrap_or(0)
}

pub(crate) mod settings;

#[cfg(test)]
mod tests;
