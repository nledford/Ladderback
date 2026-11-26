//! What the hub's pairing dialog and its section decide (§16, decision 121,
//! "The hub's dialog"), host-tested: whether the daemon is a hub, the
//! certificate the form offers first, whether the address agents will use
//! is warned of or shows a certificate, what a token is asked for, how long
//! it has left on the page's own clock and how its lapse reads, the life
//! typed, which token the dialog and a cancel are about, and what the
//! section says of a cancel once the dialog is closed.
//! `settings/agents.rs` is its glue.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::time::Duration;

use api::{ErrorCode, MsgArg, PAIRING_TOKENS_PATH, TokenCertificateDto};
use l10n::{Localizer, MessageId, msg};

use crate::routes::{instant, text_with};
use crate::session::Standing;
use crate::snapshots::viewer_offset;
use crate::ws::Posted;

/// The certificate choice's two values: the hub's own, pinned, and a
/// certificate authority's.
pub(crate) const PINNED: &str = "pinned";
pub(crate) const CA: &str = "ca";

/// The scheme of the address agents will use, as typed: a plain `http://`
/// address shows no certificate, so the form offers no choice of one and
/// the token names none (#1343).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    Https,
    Http,
}

impl Scheme {
    /// The scheme of `address`, as typed, in any case and with spaces
    /// around it, or `None` for one that is neither `http://` nor
    /// `https://`, which the hub refuses in its own words.
    pub(crate) fn of(address: &str) -> Option<Self> {
        split(address).map(|(scheme, _)| scheme)
    }

    /// Whether `address`, as typed, is a plain `http://` one, which shows
    /// no certificate, so the form offers no choice of one (#1343).
    pub(crate) fn plain(address: &str) -> bool {
        Self::of(address) == Some(Self::Http)
    }
}

/// The scheme of `address`, as typed, and what follows its `://`.
fn split(address: &str) -> Option<(Scheme, &str)> {
    let (scheme, rest) = address.trim().split_once("://")?;
    let scheme = if scheme.eq_ignore_ascii_case("http") {
        Scheme::Http
    } else if scheme.eq_ignore_ascii_case("https") {
        Scheme::Https
    } else {
        return None;
    };
    Some((scheme, rest))
}

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
        let (scheme, rest) = split(address)?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if is_loopback(host_of(authority)) {
            Some(Self::Loopback)
        } else {
            (scheme == Scheme::Http).then_some(Self::Plain)
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

/// Whether the daemon is a hub, from the page's `read` of its key: the
/// standing it was read in, and whether a key came. A read answers once it
/// was made in the standing the page is in `now`, or at once where there is
/// no standing to compare; `None` until then.
pub(crate) fn is_hub(read: Option<(Standing, bool)>, now: Option<Standing>) -> Option<bool> {
    match read {
        Some((when, key)) if now.is_none_or(|now| now == when) => Some(key),
        _ => None,
    }
}

/// The certificate the form offers first, from the one the hub's key
/// `named`: the hub's own, pinned, while it `serves_own` (the page came in
/// over it); a certificate authority's otherwise.
pub(crate) fn first_choice(named: Option<TokenCertificateDto>, serves_own: bool) -> &'static str {
    match named {
        Some(TokenCertificateDto::Pinned) if serves_own => PINNED,
        _ => CA,
    }
}

/// What a token is asked for: the address as typed, without the spaces
/// around it, and the certificate chosen; but a plain `http://` address
/// shows no certificate, so its token pins none, even when the hub's own was
/// chosen before the address was typed (#1343).
pub(crate) fn asked(typed: &str, chosen: &str) -> (String, TokenCertificateDto) {
    let typed = typed.trim();
    let certificate = match Scheme::of(typed) {
        Some(Scheme::Http) => TokenCertificateDto::Ca,
        _ if chosen == PINNED => TokenCertificateDto::Pinned,
        _ => TokenCertificateDto::Ca,
    };
    (typed.to_owned(), certificate)
}

/// When the token lapses, in the viewer's `zone` with its offset named,
/// and in how many minutes, rounded up. Without a zone the time is UTC's,
/// as `instant` writes it, and so is the offset named.
pub(crate) fn lapses(
    localizer: &Localizer,
    zone: Option<&api::IanaZone>,
    expires_at: &str,
    left: u64,
) -> String {
    let offset = jiff::Timestamp::from_str(expires_at)
        .map(|at| zone.map_or(jiff::tz::Offset::UTC, |_| viewer_offset(at)))
        .map(|offset| api::ClockOffset::new(offset).to_string())
        .unwrap_or_default();
    text_with(
        localizer,
        msg::UI_PAIRING_LAPSES,
        &[
            ("when", MsgArg::Text(instant(localizer, expires_at, zone))),
            ("offset", MsgArg::Text(offset)),
            ("minutes", MsgArg::Count(minutes(left))),
        ],
    )
}

/// What the dialog says once the token lapsed, its time in the viewer's
/// `zone`.
pub(crate) fn lapsed(
    localizer: &Localizer,
    zone: Option<&api::IanaZone>,
    expires_at: &str,
) -> String {
    text_with(
        localizer,
        msg::UI_PAIRING_LAPSED,
        &[("when", MsgArg::Text(instant(localizer, expires_at, zone)))],
    )
}

/// The address the token `id` is canceled at, on the daemon at `origin`.
pub(crate) fn cancel_url(origin: &str, id: &str) -> String {
    format!("{origin}{PAIRING_TOKENS_PATH}/{id}")
}

/// Whether `held` and `other`, IDs the page holds, name the same token:
/// the last token the dialog showed, the one it shows, or the one a cancel
/// is on its way for. Holding none is naming no token.
pub(crate) fn same_token(held: Option<&str>, other: Option<&str>) -> bool {
    held.is_some() && held == other
}

/// Whether the dialog, `open` and showing the token `shown`, shows the
/// token `id`.
pub(crate) fn shows(open: bool, shown: Option<&str>, id: &str) -> bool {
    open && same_token(shown, Some(id))
}

/// Whether the token `id` is still counted down: the dialog shows it
/// (`shown`), and it hasn't `ended`, canceled or lapsed.
pub(crate) fn counts_down(shown: Option<&str>, id: &str, ended: bool) -> bool {
    !ended && same_token(shown, Some(id))
}

/// Whether Cancel this token, or leaving the dialog, cancels the token
/// `id`: it hasn't `ended`, and no cancel of it is on its way
/// (`canceling`), whose answer is what the page says. A second cancel
/// would find none.
pub(crate) fn cancelable(ended: bool, canceling: Option<&str>, id: &str) -> bool {
    !ended && !same_token(canceling, Some(id))
}

/// Whether the answer of a cancel that was on its way ends the wait for
/// it: the cancel on its way now (`canceling`) is still the token `id`'s,
/// not a later token's.
pub(crate) fn cancel_on_its_way(canceling: Option<&str>, id: &str) -> bool {
    same_token(canceling, Some(id))
}

/// Whether the dialog's Cancel this token is busy: a cancel of the token it
/// shows (`shown`) is on its way (`canceling`).
pub(crate) fn canceling_shown(canceling: Option<&str>, shown: Option<&str>) -> bool {
    same_token(canceling, shown)
}

/// Whether the dialog was just left: it was open (`was_open`, `None`
/// before the first look) and no longer is.
pub(crate) fn left(was_open: Option<bool>, open: bool) -> bool {
    was_open == Some(true) && !open
}

/// What the section says of the cancel of a token whose dialog is closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClosedCancel {
    /// Of the last token the dialog showed: its answer in full, or nothing,
    /// in place of what was said of the one before.
    Last(Option<MessageId>),
    /// Of an earlier token, only that it still works, so its answer is
    /// never read as the later token's.
    Earlier(MessageId),
}

/// What the section says of the cancel `answer` of a closed dialog's
/// token, the `last` the dialog showed or an earlier one; `None` for an
/// earlier token that no longer works.
pub(crate) fn closed_cancel(last: bool, answer: Option<&Posted>) -> Option<ClosedCancel> {
    if last {
        Some(ClosedCancel::Last(said_once_closed(answer)))
    } else if still_works(answer) {
        Some(ClosedCancel::Earlier(msg::UI_PAIRING_EARLIER_NOT_CANCELED))
    } else {
        None
    }
}

/// Whether a token still works once its cancel is answered: the hub
/// refused the cancel, other than as a token it no longer holds, or didn't
/// answer.
fn still_works(answer: Option<&Posted>) -> bool {
    !matches!(
        answer,
        Some(Posted::Done(_) | Posted::Refused(ErrorCode::PairingTokenUnknown, ..))
    )
}

/// What the section says of a cancel answered once its dialog is closed:
/// that the token was canceled, or that the hub couldn't cancel it, so it
/// still works until it lapses. A token the hub no longer holds was used or
/// lapsed already, and nothing is said.
fn said_once_closed(answer: Option<&Posted>) -> Option<MessageId> {
    match answer {
        Some(Posted::Done(_)) => Some(msg::UI_PAIRING_CLOSED_CANCELED),
        Some(Posted::Refused(ErrorCode::PairingTokenUnknown, ..)) => None,
        Some(Posted::Refused(..)) | None => Some(msg::UI_PAIRING_CLOSED_NOT_CANCELED),
    }
}

#[cfg(test)]
mod tests;
