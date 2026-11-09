//! The toast's arrival announcement and lifetime (technical design §4.5): a
//! toast is announced politely once — assertive is only for destructive
//! results — stays on screen for [`TOAST_MS`] and goes unannounced. The
//! decisions are host-decidable, so they are tested here; `components.rs`
//! applies them to the DOM.

use std::cell::Cell;
use std::time::Duration;

use crate::announcer::Politeness;

/// How long a toast stays on screen before it goes, unannounced.
pub(crate) const TOAST_MS: u64 = 3_000;

/// A toast's arrival: how it is announced and how long it stays.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Arrival {
    /// Toasts are polite; only destructive results are assertive (§4.5).
    pub politeness: Politeness,
    /// How long the toast stays on screen before it goes, unannounced.
    pub shown_for: Duration,
}

/// How a toast's arrival is announced and how long it stays.
pub(crate) fn arrival() -> Arrival {
    Arrival {
        politeness: Politeness::Polite,
        shown_for: Duration::from_millis(TOAST_MS),
    }
}

/// Counts toast arrivals, so an older toast's expiry can no longer clear the
/// text a newer toast is showing: a toast that arrives while another is on
/// screen still gets its whole lifetime.
#[derive(Debug, Default)]
pub(crate) struct ToastTimers {
    generation: Cell<u64>,
}

impl ToastTimers {
    /// Marks a toast's arrival and returns its generation.
    pub(crate) fn arrive(&self) -> u64 {
        let next = self.generation.get().checked_add(1).unwrap_or(0);
        self.generation.set(next);
        next
    }

    /// Whether the toast of `generation` is still the newest one shown.
    pub(crate) fn still_current(&self, generation: u64) -> bool {
        self.generation.get() == generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toasts_arrival_is_announced_politely_and_shown_for_three_seconds() {
        let plan = arrival();
        assert_eq!(plan.politeness, Politeness::Polite);
        assert_eq!(plan.shown_for, Duration::from_millis(3_000));
    }

    #[test]
    fn an_older_toasts_expiry_no_longer_clears_the_text() {
        let timers = ToastTimers::default();
        let first = timers.arrive();
        let second = timers.arrive();
        assert!(!timers.still_current(first));
        assert!(timers.still_current(second));
    }
}
