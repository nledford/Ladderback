//! Where an agent connects for a token (§16, decision 120, "Pairing", and
//! decision 121, "Errors and their next step"): the host and port of the
//! hub's address, and how the session is protected. A token's address is
//! `https://host[:port]` or, to this computer only, `http://host[:port]`; a
//! plain address off loopback would show the token to the network, so it is
//! refused before anything is sent.

use std::net::{IpAddr, Ipv6Addr};

use super::fingerprint::Fingerprint;
use super::token::{HubAddress, TokenTls};

/// The port an `https://` address without one means.
const HTTPS_PORT: u16 = 443;
/// The port an `http://` address without one means.
const HTTP_PORT: u16 = 80;

/// How an agent reaches its hub for a token's address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubRoute {
    /// The host as the address names it: a name, or an IP address without
    /// brackets.
    pub host: String,
    pub port: u16,
    pub session: RouteSession,
}

/// What protects the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSession {
    /// No TLS: an `http://` address on this computer.
    Plain,
    /// TLS checked against a certificate authority, as a proxy's is.
    Ca,
    /// TLS with the hub's own certificate, pinned by its fingerprint.
    Pinned(Fingerprint),
}

/// Why an address can't be connected to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRefused {
    /// A plain `http://` address off loopback, which would show the token to
    /// the network.
    PlainOffLoopback,
    /// Not `scheme://host[:port]` with an optional `/`: a user, a path, a
    /// port out of range, or an `http://` address with a pinned certificate.
    Malformed,
}

impl HubRoute {
    /// The route for `address` under the token's `tls`.
    pub fn of(address: &HubAddress, tls: TokenTls) -> Result<Self, RouteRefused> {
        let text = address.as_str();
        let (plain, rest) = match text.strip_prefix("https://") {
            Some(rest) => (false, rest),
            None => (
                true,
                text.strip_prefix("http://")
                    .ok_or(RouteRefused::Malformed)?,
            ),
        };
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        if authority.contains(['/', '@', '?', '#']) {
            return Err(RouteRefused::Malformed);
        }
        let (host, port) = host_and_port(authority)?;
        let port = match port {
            None if plain => HTTP_PORT,
            None => HTTPS_PORT,
            Some(port) => port_number(port)?,
        };
        let session = match (plain, tls) {
            (true, TokenTls::Pinned(_)) => return Err(RouteRefused::Malformed),
            (true, TokenTls::Ca) if !is_loopback(host) => {
                return Err(RouteRefused::PlainOffLoopback);
            }
            (true, TokenTls::Ca) => RouteSession::Plain,
            (false, TokenTls::Ca) => RouteSession::Ca,
            (false, TokenTls::Pinned(fingerprint)) => RouteSession::Pinned(fingerprint),
        };
        Ok(Self {
            host: host.to_owned(),
            port,
            session,
        })
    }
}

/// The host and, when there is one, the port's text of `authority`: an
/// IPv6 address stands in brackets, which the host loses.
fn host_and_port(authority: &str) -> Result<(&str, Option<&str>), RouteRefused> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (inside, after) = rest.split_once(']').ok_or(RouteRefused::Malformed)?;
        inside
            .parse::<Ipv6Addr>()
            .map_err(|_| RouteRefused::Malformed)?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or(RouteRefused::Malformed)?),
        };
        (inside, port)
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(RouteRefused::Malformed);
    }
    Ok((host, port))
}

/// A port written as 1 to 65535 in decimal digits only.
fn port_number(text: &str) -> Result<u16, RouteRefused> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RouteRefused::Malformed);
    }
    match text.parse::<u16>() {
        Ok(port) if port != 0 => Ok(port),
        _ => Err(RouteRefused::Malformed),
    }
}

/// Whether `host` names this computer: `localhost`, or a loopback address.
pub fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests;
