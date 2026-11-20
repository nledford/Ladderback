//! What the hub's pairing dialog decides (§16, decision 121, "The hub's
//! dialog"), host-tested: whether the address agents will use is warned of,
//! how long a token has left on the page's own clock, and the life typed.
//! `settings/agents.rs` is its glue.

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
        let _ = address;
        todo!()
    }
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
        let _ = (now, self.seconds_left, self.from);
        todo!()
    }
}

/// The minutes the dialog says are left for `seconds` left, rounded up, so
/// a token is never said to have none left while it has some.
pub(crate) fn minutes(seconds: u64) -> u64 {
    let _ = seconds;
    todo!()
}

/// The life typed, in whole minutes, or `None` for anything else. The hub
/// checks the range (`pairing-token-life`).
pub(crate) fn life(typed: &str) -> Option<u32> {
    let _ = typed;
    todo!()
}

#[cfg(test)]
mod tests;
