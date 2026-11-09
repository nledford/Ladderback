//! The announcer (technical design §4.5, "Live updates"; invariant A3): the one
//! place that decides what a screen reader is told about changes the page
//! makes by itself, and when.
//!
//! This is the timing logic alone, over an injected [`Clock`], so a host
//! `cargo test` runs it with a fake one. It owns no DOM: it answers with
//! [`Effect`]s, the writes to the two live regions, and `next_wakeup` says when
//! to ask again. The glue that applies them to the regions is thin and
//! covered by the `@ui` scenarios of `features/ui/accessibility.feature`.
//!
//! - **Write, clear, then set.** A message first clears its region and sets
//!   its text [`SET_DELAY`] later, so a repeated message is announced again.
//! - **Polite messages are merged.** The first opens a [`MERGE_WINDOW`]; every
//!   polite message in it joins one write when it closes, in order, a text
//!   already in the batch said once.
//! - **Assertive messages go straight through**, even while a modal is open.
//! - **While a modal is open polite messages wait,** and are written at once
//!   when the last modal closes. At most [`MAX_HELD`] wait.
//! - **Progress is never announced.**

// Commit 1 of the red-first history: the bodies are `todo!()` stubs, and
// nothing but the tests uses the module yet. Both allowances go with the
// implementation.
#![allow(dead_code, clippy::todo)]

use std::time::Duration;

/// How long a polite message waits for company before it is written.
pub(crate) const MERGE_WINDOW: Duration = Duration::from_secs(2);

/// How long after a region is cleared its text is set.
pub(crate) const SET_DELAY: Duration = Duration::from_millis(50);

/// The most polite messages held for one write. Messages come from the page's
/// own events, but a modal left open must not grow the queue without bound.
pub(crate) const MAX_HELD: usize = 16;

/// Where the announcer reads the time: a duration since any fixed origin that
/// never goes backwards.
pub(crate) trait Clock {
    fn now(&self) -> Duration;
}

/// Which live region a message goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Politeness {
    /// The `role="status"` region: spoken when the reader is idle.
    Polite,
    /// The `role="alert"` region: spoken at once. Only for a failure that
    /// needs the user now.
    Assertive,
}

/// What a message is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Something that happened: an operation finished, an agent came back.
    Event,
    /// A progress update. It is never announced: a screen reader reads the
    /// `<progress>` element itself.
    Progress,
}

/// A message for the screen reader, already in the page's language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Announcement {
    pub(crate) kind: Kind,
    pub(crate) politeness: Politeness,
    pub(crate) text: String,
}

/// A write the glue applies to a live region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Empty the region.
    Clear(Politeness),
    /// Put the text in the region.
    Set(Politeness, String),
}

/// A text waiting for its region to be set.
#[derive(Debug)]
struct PendingSet {
    due: Duration,
    text: String,
}

/// The state of the two regions.
#[derive(Debug)]
pub(crate) struct Announcer<C> {
    clock: C,
    /// Polite texts waiting for one write, in arrival order.
    held: Vec<String>,
    /// When the window that `held` began in closes.
    window_ends: Option<Duration>,
    /// How many modal dialogs are open.
    open_modals: usize,
    polite_set: Option<PendingSet>,
    assertive_set: Option<PendingSet>,
}

impl<C: Clock> Announcer<C> {
    pub(crate) fn new(clock: C) -> Self {
        Self {
            clock,
            held: Vec::new(),
            window_ends: None,
            open_modals: 0,
            polite_set: None,
            assertive_set: None,
        }
    }

    /// Takes in a message. Returns the writes due now.
    pub(crate) fn announce(&mut self, announcement: Announcement) -> Vec<Effect> {
        let _ = announcement;
        todo!("announce")
    }

    /// A modal dialog opened: polite messages wait until the last one closes.
    pub(crate) fn modal_opened(&mut self) {
        todo!("modal_opened")
    }

    /// A modal dialog closed. When it was the last, returns the write of the
    /// polite messages that waited. A close with no modal open is ignored.
    pub(crate) fn modal_closed(&mut self) -> Vec<Effect> {
        todo!("modal_closed")
    }

    /// The writes that have come due since the last call.
    pub(crate) fn poll(&mut self) -> Vec<Effect> {
        todo!("poll")
    }

    /// How long from now the next write falls due, or `None` when nothing is
    /// waiting on the clock (a held batch behind an open modal waits on the
    /// modal).
    pub(crate) fn next_wakeup(&self) -> Option<Duration> {
        todo!("next_wakeup")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::Effect::{Clear, Set};
    use super::Politeness::{Assertive, Polite};
    use super::*;

    /// A clock the test moves.
    #[derive(Clone, Default)]
    struct FakeClock(Rc<Cell<Duration>>);

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }

    struct Page {
        clock: FakeClock,
        announcer: Announcer<FakeClock>,
    }

    impl Page {
        fn new() -> Self {
            let clock = FakeClock::default();
            Self {
                announcer: Announcer::new(clock.clone()),
                clock,
            }
        }

        fn at(&self, millis: u64) {
            self.clock.0.set(Duration::from_millis(millis));
        }

        fn say(&mut self, politeness: Politeness, text: &str) -> Vec<Effect> {
            self.announcer.announce(Announcement {
                kind: Kind::Event,
                politeness,
                text: text.to_owned(),
            })
        }

        fn polite(&mut self, text: &str) -> Vec<Effect> {
            self.say(Polite, text)
        }

        /// Moves the clock to `millis` and returns what falls due.
        fn poll_at(&mut self, millis: u64) -> Vec<Effect> {
            self.at(millis);
            self.announcer.poll()
        }

        /// Polls at every millisecond from `from` to `to` (inclusive) and
        /// returns the writes with the time each came due.
        fn writes_between(&mut self, from: u64, to: u64) -> Vec<(u64, Effect)> {
            let mut writes = Vec::new();
            for millis in from..=to {
                for effect in self.poll_at(millis) {
                    writes.push((millis, effect));
                }
            }
            writes
        }
    }

    fn set(politeness: Politeness, text: &str) -> Effect {
        Set(politeness, text.to_owned())
    }

    #[test]
    fn a_polite_message_is_cleared_when_its_window_closes_and_set_a_tick_later() {
        let mut page = Page::new();
        assert_eq!(page.polite("Backup finished."), []);
        assert_eq!(
            page.writes_between(1, 5_000),
            [
                (2_000, Clear(Polite)),
                (2_050, set(Polite, "Backup finished.")),
            ]
        );
    }

    #[test]
    fn polite_messages_in_one_window_are_merged_in_order() {
        let mut page = Page::new();
        page.polite("Backup finished.");
        page.at(1_500);
        page.polite("Agent back online.");
        assert_eq!(
            page.writes_between(1_501, 5_000),
            [
                (2_000, Clear(Polite)),
                (2_050, set(Polite, "Backup finished. Agent back online.")),
            ]
        );
    }

    #[test]
    fn the_window_runs_from_the_first_message_not_the_last() {
        let mut page = Page::new();
        page.polite("One.");
        page.at(1_999);
        page.polite("Two.");
        assert_eq!(page.poll_at(1_999), []);
        assert_eq!(page.poll_at(2_000), [Clear(Polite)]);
    }

    #[test]
    fn the_same_text_twice_in_one_window_is_said_once() {
        let mut page = Page::new();
        page.polite("Backup finished.");
        page.at(100);
        page.polite("Backup finished.");
        assert_eq!(
            page.writes_between(101, 5_000),
            [
                (2_000, Clear(Polite)),
                (2_050, set(Polite, "Backup finished.")),
            ]
        );
    }

    #[test]
    fn a_message_after_the_window_closed_opens_a_new_one() {
        let mut page = Page::new();
        page.polite("One.");
        page.writes_between(1, 2_500);
        page.polite("Two.");
        assert_eq!(
            page.writes_between(2_501, 5_000),
            [(4_500, Clear(Polite)), (4_550, set(Polite, "Two."))]
        );
    }

    #[test]
    fn the_same_text_five_seconds_apart_is_announced_twice() {
        let mut page = Page::new();
        page.polite("Backup finished.");
        let first = page.writes_between(1, 4_999);
        page.at(5_000);
        page.polite("Backup finished.");
        let second = page.writes_between(5_001, 8_000);
        assert_eq!(
            first,
            [
                (2_000, Clear(Polite)),
                (2_050, set(Polite, "Backup finished.")),
            ]
        );
        assert_eq!(
            second,
            [
                (7_000, Clear(Polite)),
                (7_050, set(Polite, "Backup finished.")),
            ]
        );
    }

    #[test]
    fn an_assertive_message_clears_at_once_and_is_set_a_tick_later() {
        let mut page = Page::new();
        assert_eq!(page.say(Assertive, "Restore failed."), [Clear(Assertive)]);
        assert_eq!(page.poll_at(49), []);
        assert_eq!(page.poll_at(50), [set(Assertive, "Restore failed.")]);
        assert_eq!(page.poll_at(51), []);
    }

    #[test]
    fn an_assertive_message_does_not_wait_for_the_polite_window() {
        let mut page = Page::new();
        page.polite("Backup finished.");
        page.at(100);
        assert_eq!(page.say(Assertive, "Restore failed."), [Clear(Assertive)]);
        assert_eq!(page.poll_at(150), [set(Assertive, "Restore failed.")]);
    }

    #[test]
    fn a_message_for_a_region_still_waiting_to_be_set_is_kept() {
        let mut page = Page::new();
        assert_eq!(page.say(Assertive, "Restore failed."), [Clear(Assertive)]);
        page.at(10);
        assert_eq!(page.say(Assertive, "Prune failed."), []);
        assert_eq!(
            page.poll_at(50),
            [set(Assertive, "Restore failed. Prune failed.")]
        );
    }

    #[test]
    fn progress_is_never_announced() {
        let mut page = Page::new();
        for politeness in [Polite, Assertive] {
            let effects = page.announcer.announce(Announcement {
                kind: Kind::Progress,
                politeness,
                text: "42 %".to_owned(),
            });
            assert_eq!(effects, []);
        }
        assert_eq!(page.writes_between(1, 10_000), []);
        assert_eq!(page.announcer.next_wakeup(), None);
    }

    #[test]
    fn a_blank_message_is_not_announced() {
        let mut page = Page::new();
        assert_eq!(page.say(Assertive, "  "), []);
        page.polite("");
        assert_eq!(page.writes_between(1, 10_000), []);
        assert_eq!(page.announcer.next_wakeup(), None);
    }

    #[test]
    fn polite_messages_wait_while_a_modal_is_open_and_are_written_when_it_closes() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        page.polite("Backup finished.");
        assert_eq!(page.writes_between(1, 10_000), []);
        assert_eq!(page.announcer.next_wakeup(), None);
        assert_eq!(page.announcer.modal_closed(), [Clear(Polite)]);
        assert_eq!(page.poll_at(10_049), []);
        assert_eq!(page.poll_at(10_050), [set(Polite, "Backup finished.")]);
    }

    #[test]
    fn a_modal_opened_inside_a_window_holds_the_batch_and_closing_writes_it_at_once() {
        let mut page = Page::new();
        page.polite("One.");
        page.at(1_000);
        page.announcer.modal_opened();
        assert_eq!(page.poll_at(3_000), []);
        page.at(4_000);
        page.polite("Two.");
        assert_eq!(page.announcer.modal_closed(), [Clear(Polite)]);
        assert_eq!(page.poll_at(4_050), [set(Polite, "One. Two.")]);
    }

    #[test]
    fn an_assertive_message_is_not_held_by_a_modal() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        assert_eq!(page.say(Assertive, "Restore failed."), [Clear(Assertive)]);
        assert_eq!(page.poll_at(50), [set(Assertive, "Restore failed.")]);
    }

    #[test]
    fn closing_a_modal_with_nothing_held_says_nothing() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        assert_eq!(page.announcer.modal_closed(), []);
        assert_eq!(page.announcer.next_wakeup(), None);
    }

    #[test]
    fn nested_modals_hold_messages_until_the_last_one_closes() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        page.announcer.modal_opened();
        page.polite("Backup finished.");
        assert_eq!(page.announcer.modal_closed(), []);
        assert_eq!(page.poll_at(5_000), []);
        assert_eq!(page.announcer.modal_closed(), [Clear(Polite)]);
    }

    #[test]
    fn closing_a_modal_that_was_never_opened_does_not_hold_later_messages() {
        let mut page = Page::new();
        assert_eq!(page.announcer.modal_closed(), []);
        page.polite("Backup finished.");
        assert_eq!(page.poll_at(2_000), [Clear(Polite)]);
    }

    #[test]
    fn a_modal_left_open_holds_at_most_a_bounded_number_of_messages() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        for number in 0..MAX_HELD + 5 {
            page.polite(&format!("Message {number}."));
        }
        assert_eq!(page.announcer.modal_closed(), [Clear(Polite)]);
        let expected = (0..MAX_HELD)
            .map(|number| format!("Message {number}."))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(page.poll_at(50), [set(Polite, &expected)]);
    }

    #[test]
    fn the_next_wakeup_is_the_nearest_write_still_to_come() {
        let mut page = Page::new();
        assert_eq!(page.announcer.next_wakeup(), None);
        page.polite("Backup finished.");
        assert_eq!(page.announcer.next_wakeup(), Some(MERGE_WINDOW));
        page.at(500);
        assert_eq!(
            page.announcer.next_wakeup(),
            Some(Duration::from_millis(1_500))
        );
        page.poll_at(2_000);
        assert_eq!(page.announcer.next_wakeup(), Some(SET_DELAY));
        page.poll_at(2_050);
        assert_eq!(page.announcer.next_wakeup(), None);
    }

    #[test]
    fn a_wakeup_that_is_overdue_is_due_now() {
        let mut page = Page::new();
        page.say(Assertive, "Restore failed.");
        page.at(500);
        assert_eq!(page.announcer.next_wakeup(), Some(Duration::ZERO));
    }
}
