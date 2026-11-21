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

/// The host of `authority` (`host:port`, `[v6]:port`), without its port or
/// an IPv6 address's brackets.
fn host_of(authority: &str) -> &str {
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split_once(']').map_or(v6, |(host, _port)| host);
    }
    authority
        .rsplit_once(':')
        .map_or(authority, |(host, _port)| host)
}

/// Whether `host` names this computer: `localhost`, the IPv6 loopback, or
/// an IPv4 address in `127.0.0.0/8` in a form every resolver reads alike
/// (`ipv4_read_alike`), as `127.1`, which the agent's connection resolves
/// there. Its twin, `crates/domain/src/fleet/route.rs::is_loopback`, routes
/// the agent by the same hosts; the two are tested with one table, so keep
/// them alike.
fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback())
        || ipv4_read_alike(host).is_some_and(|ip| ip.is_loopback())
}

/// An IPv4 address in a form every resolver reads alike: one to four
/// decimal parts split by dots, the last filling the bytes the others
/// leave, as `127.1` is `127.0.0.1` and `2130706433` is too. A part with a
/// leading `0` or `0x` is none: glibc reads it as octal or hex and macOS,
/// in four parts, as decimal, so `0177.0.0.1` is `127.0.0.1` on one and
/// `177.0.0.1` on the other.
fn ipv4_read_alike(host: &str) -> Option<Ipv4Addr> {
    let parts = host.split('.').map(number).collect::<Option<Vec<u64>>>()?;
    let (&last, leading) = parts.split_last()?;
    let room = 4_u32.checked_sub(u32::try_from(leading.len()).ok()?)?;
    let span = 256_u64.checked_pow(room)?;
    if room == 0 || last >= span {
        return None;
    }
    let mut address = 0_u64;
    for &part in leading {
        if part > 255 {
            return None;
        }
        address = address.checked_mul(256)?.checked_add(part)?;
    }
    let address = address.checked_mul(span)?.checked_add(last)?;
    u32::try_from(address).ok().map(Ipv4Addr::from)
}

/// One part of an IPv4 address in decimal digits, with no leading `0`
/// unless it is `0` itself.
fn number(part: &str) -> Option<u64> {
    let decimal = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !decimal || (part.len() > 1 && part.starts_with('0')) {
        return None;
    }
    part.parse().ok()
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
