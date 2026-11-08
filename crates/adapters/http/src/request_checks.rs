//! Request checks (technical design §10.1), always on, with or without sign-in.
//!
//! The UI can set hooks and `password-command`, so an API that a web page can
//! reach would let any site run commands on this machine, through DNS
//! rebinding or a cross-site POST. Every request passes these checks before a
//! route's handler runs:
//!
//! 1. the `Host` header names a loopback address or a configured host name;
//! 2. a request that carries an `Origin` header comes from the UI's own origin;
//! 3. such a request, when it mutates, uses `Content-Type: application/json`.
//!
//! A request without `Origin` (curl, scripts, the `steading` CLI) is subject
//! to the first check only. Each refusal is a typed [`ErrorCode`]; `/sync/v1`
//! is exempt from all three, since agents authenticate there with a signed
//! nonce (§7.1).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use api::ErrorCode;
use axum::Json;
use axum::extract::{Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderName, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// The host names the UI answers to, besides loopback addresses.
#[derive(Clone, Debug, Default)]
pub struct RequestChecks {
    /// Without a port; compared regardless of case.
    allowed_hosts: Arc<[String]>,
}

impl RequestChecks {
    /// `hosts` are the configured names, as [`validate_host_name`] accepts them.
    pub fn new(hosts: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_hosts: hosts.into_iter().collect(),
        }
    }

    fn check(&self, method: &Method, uri: &Uri, headers: &HeaderMap) -> Result<(), ErrorCode> {
        let _ = (method, uri, headers, &self.allowed_hosts);
        todo!("request checks")
    }

    /// Whether `host` (the host part of an authority) is a loopback address
    /// or a configured name.
    fn allows(&self, host: &str) -> bool {
        is_loopback_host(host)
            || self
                .allowed_hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(host))
    }
}

/// The agent routes authenticate with a signed nonce (§7.1), are reached by
/// whatever name the hub has, and aren't browsers.
fn is_agent_route(path: &str) -> bool {
    path.strip_prefix("/sync/v1")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// What a header that must appear at most once holds.
enum Header<'a> {
    Absent,
    Text(&'a str),
    /// Repeated, or not visible ASCII. Either way it can't be trusted.
    Invalid,
}

fn one<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Header<'a> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (None, _) => Header::Absent,
        (Some(value), None) => value.to_str().map_or(Header::Invalid, Header::Text),
        (Some(_), Some(_)) => Header::Invalid,
    }
}

/// The host of an authority (a `Host` header or an origin without its
/// scheme), once the whole text is known to be well formed.
struct Authority<'a> {
    /// A name, an IPv4 address or an IPv6 address in brackets.
    host: &'a str,
}

/// Parses `name`, `name:port`, `1.2.3.4[:port]` or `[v6][:port]` and nothing
/// else: no user info, path, spaces or malformed port.
fn parse_authority(text: &str) -> Option<Authority<'_>> {
    let (host, port) = if let Some(rest) = text.strip_prefix('[') {
        let (inner, after) = rest.split_once(']')?;
        inner.parse::<Ipv6Addr>().ok()?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':')?),
        };
        (text.strip_suffix(after)?, port)
    } else {
        match text.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (text, None),
        }
    };
    let name_like = !host.is_empty()
        && (host.starts_with('[')
            || host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_')));
    let port_ok = port.is_none_or(|port| {
        !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
    });
    (name_like && port_ok).then_some(Authority { host })
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6.parse::<Ipv6Addr>().is_ok_and(|addr| addr.is_loopback());
    }
    host.parse::<Ipv4Addr>()
        .is_ok_and(|addr| addr.is_loopback())
}

/// The UI's own origin: the request's `Host`, behind `http://` or, behind a
/// TLS-terminating proxy, `https://`. `host` is already checked.
fn is_own_origin(origin: &str, host: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .find_map(|scheme| origin.strip_prefix(scheme))
        .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

/// Whether a `Content-Type` is `application/json`, with or without
/// parameters. Other JSON types (`+json`, `-seq`) aren't.
fn is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

/// Refuses a request that fails a check, before any route sees it.
pub(crate) async fn enforce(
    State(checks): State<RequestChecks>,
    request: Request,
    next: Next,
) -> Response {
    match checks.check(request.method(), request.uri(), request.headers()) {
        Ok(()) => next.run(request).await,
        Err(code) => refusal(code),
    }
}

/// The body of a refusal: the code alone, for the UI to render (§8.4).
#[derive(Serialize)]
struct Refusal {
    code: ErrorCode,
}

fn refusal(code: ErrorCode) -> Response {
    let status = match code {
        ErrorCode::JsonRequired => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        _ => StatusCode::FORBIDDEN,
    };
    (status, Json(Refusal { code })).into_response()
}

/// The startup rule of §10.1. Sign-in doesn't exist yet and there is no
/// trusted-proxy mode, so the daemon may only listen on a loopback address.
pub fn require_loopback(addr: SocketAddr) -> Result<(), NonLoopbackBind> {
    let _ = addr;
    todo!("startup rule")
}

/// The daemon was asked to listen beyond loopback with sign-in off.
#[derive(Debug, PartialEq, Eq)]
pub struct NonLoopbackBind(pub SocketAddr);

impl std::fmt::Display for NonLoopbackBind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "refusing to listen on {}: sign-in is off, so Steading only listens on loopback addresses (127.0.0.1, ::1)",
            self.0
        )
    }
}

impl std::error::Error for NonLoopbackBind {}

/// Checks a host name for `--allowed-host`: a name or an address without a
/// port, such as `nas.example.com`, `192.168.1.5` or `[fd00::1]`. A value the
/// `Host` header could never match would silently lock the UI out.
pub fn validate_host_name(name: &str) -> Result<String, InvalidHostName> {
    let _ = name;
    todo!("host name validation")
}

/// A configured host name that isn't a name or an address without a port.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidHostName(pub String);

impl std::fmt::Display for InvalidHostName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` isn't a host name or address without a port, such as nas.example.com",
            self.0
        )
    }
}

impl std::error::Error for InvalidHostName {}

#[cfg(test)]
mod tests;
