//! What the hub's pairing dialog decides (§16, decision 121, "The hub's
//! dialog"), host-tested: whether the address agents will use is warned of,
//! how long a token has left on the page's own clock, and the life typed.
//! `settings/agents.rs` is its glue.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

/// Why the address agents will use is warned of. Making a token for it
/// stays possible, for an agent on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AddressWarning {
    /// `localhost`, `127.0.0.0/8` or `[::1]`: another machine can't reach it.
    Loopback,
    /// Plain `http://` to a host that isn't loopback: an agent refuses it
    /// (decision 120, "Binding to the TLS session").
    Plain,
}

impl AddressWarning {
    /// The warning `address` gets, as typed, or `None` for an `https://`
    /// address beyond loopback, and for one that is neither `http://` nor
    /// `https://`, which the hub refuses in its own words.
    pub(crate) fn of(address: &str) -> Option<Self> {
        let address = address.trim();
        let scheme_end = address.find("://")?;
        let plain = match address.get(..scheme_end)?.to_ascii_lowercase().as_str() {
            "http" => true,
            "https" => false,
            _ => return None,
        };
        let rest = address.get(scheme_end.saturating_add(3)..)?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if is_loopback(host_of(authority)) {
            Some(Self::Loopback)
        } else {
            plain.then_some(Self::Plain)
        }
    }
}

/// The host of `authority` (`host:port`, `[v6]:port`), without its port.
fn host_of(authority: &str) -> &str {
    if authority.starts_with('[') {
        return authority
            .find(']')
            .and_then(|end| authority.get(..=end))
            .unwrap_or(authority);
    }
    authority
        .rsplit_once(':')
        .map_or(authority, |(host, _port)| host)
}

/// Whether `host` names this machine alone: `localhost`, an address in
/// `127.0.0.0/8`, or `[::1]`.
fn is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Some(v6) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        return v6.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback());
    }
    host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

/// A token's life as the page counts it down: the seconds the hub said it
/// had left, from the moment its answer came, on the page's monotonic clock.
/// The browser's wall clock is never read, so a wrong one can't show a live
/// token as lapsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Countdown {
    seconds_left: u64,
    from: Duration,
}

impl Countdown {
    /// The hub's `seconds_left`, counted from `from` on the monotonic clock.
    pub(crate) const fn new(seconds_left: u64, from: Duration) -> Self {
        Self { seconds_left, from }
    }

    /// The whole seconds left at `now`: none once the life is spent.
    pub(crate) fn left(self, now: Duration) -> u64 {
        let spent = now.saturating_sub(self.from).as_secs();
        self.seconds_left.saturating_sub(spent)
    }
}

/// The minutes the dialog says are left for `seconds` left, rounded up, so
/// a token is never said to have none left while it has some.
pub(crate) fn minutes(seconds: u64) -> u64 {
    seconds.div_ceil(60)
}

/// The life typed, in whole minutes, or `None` for anything else. The hub
/// checks the range (`pairing-token-life`).
pub(crate) fn life(typed: &str) -> Option<u32> {
    let typed = typed.trim();
    if typed.is_empty() || !typed.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    typed.parse().ok()
}

#[cfg(test)]
mod tests;
