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
//! - **Assertive messages skip the merge window:** one is written at once,
//!   clear then set, and a second for the same region joins the set still waiting.
//! - **While a modal is open every message waits,** polite and assertive: the
//!   modal makes the regions inert, and some screen readers (VoiceOver, JAWS)
//!   say nothing for a live region in inert content. They are written at once
//!   when the last modal closes, assertive first. At most [`MAX_HELD`] wait
//!   per region.
//! - **While live updates are paused every event waits** (WCAG 2.2.2): what
//!   happened by itself is held until the reader resumes, and then said in
//!   one write after the page's own summary. What answers the reader's own
//!   press is said as usual.
//! - **Progress is never announced.**

use std::time::Duration;

/// How long a polite message waits for company before it is written.
pub(crate) const MERGE_WINDOW: Duration = Duration::from_secs(2);

/// How long after a region is cleared its text is set.
pub(crate) const SET_DELAY: Duration = Duration::from_millis(50);

/// The most messages held for one write, per region. Messages come from the
/// page's own events, but a modal left open must not grow the queue without
/// bound.
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
    /// Something that happened by itself: an operation finished, an agent
    /// came back. It waits while live updates are paused.
    Event,
    /// The page's answer to what the reader just did: a sort, a start, a
    /// form's result. It is said whether or not live updates are paused.
    Reply,
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
    /// Assertive texts waiting for the last modal to close, in arrival order.
    held_assertive: Vec<String>,
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
            held_assertive: Vec::new(),
            window_ends: None,
            open_modals: 0,
            polite_set: None,
            assertive_set: None,
        }
    }

    /// Takes in a message. Returns the writes due now.
    pub(crate) fn announce(&mut self, announcement: Announcement) -> Vec<Effect> {
        let Announcement {
            kind,
            politeness,
            text,
        } = announcement;
        if kind == Kind::Progress || text.trim().is_empty() {
            return Vec::new();
        }
        match politeness {
            Politeness::Assertive if self.open_modals == 0 => {
                self.schedule_set(Politeness::Assertive, text)
            }
            Politeness::Assertive => {
                hold(&mut self.held_assertive, text);
                Vec::new()
            }
            Politeness::Polite => {
                hold(&mut self.held, text);
                if self.open_modals == 0 && self.window_ends.is_none() && !self.held.is_empty() {
                    self.window_ends = Some(self.clock.now() + MERGE_WINDOW);
                }
                Vec::new()
            }
        }
    }

    /// A modal dialog opened: messages wait until the last one closes.
    pub(crate) fn modal_opened(&mut self) {
        self.open_modals = self.open_modals.saturating_add(1);
    }

    /// A modal dialog closed. When it was the last, returns the writes of the
    /// messages that waited. A close with no modal open is ignored.
    pub(crate) fn modal_closed(&mut self) -> Vec<Effect> {
        if self.open_modals == 0 {
            return Vec::new();
        }
        self.open_modals -= 1;
        if self.open_modals > 0 {
            return Vec::new();
        }
        let mut effects = self.flush_held_assertive();
        effects.extend(self.flush_held());
        effects
    }

    /// The reader paused live updates: every [`Kind::Event`] waits for
    /// [`Self::resume`]. `said`, the page's answer to the press, is written at
    /// once rather than after a merge window, so a resume soon after never
    /// joins it. Returns the writes due now.
    pub(crate) fn pause(&mut self, said: String) -> Vec<Effect> {
        let _ = said;
        todo!("pause the announcer")
    }

    /// The reader resumed live updates: `summary`, what changed meanwhile,
    /// is said first, then every message that waited, in one write per
    /// region at once, assertive first. Behind an open modal they wait for it
    /// to close instead. A resume while not paused is ignored.
    pub(crate) fn resume(&mut self, summary: Option<String>) -> Vec<Effect> {
        let _ = summary;
        todo!("resume the announcer")
    }

    /// The writes that have come due since the last call. While a modal is
    /// open nothing is written: the regions are inert.
    pub(crate) fn poll(&mut self) -> Vec<Effect> {
        if self.open_modals > 0 {
            return Vec::new();
        }
        let now = self.clock.now();
        let mut effects = Vec::new();
        for politeness in [Politeness::Assertive, Politeness::Polite] {
            let slot = self.pending_set(politeness);
            if slot.as_ref().is_some_and(|pending| pending.due <= now)
                && let Some(pending) = slot.take()
            {
                effects.push(Effect::Set(politeness, pending.text));
            }
        }
        if self.window_ends.is_some_and(|ends| ends <= now) {
            effects.extend(self.flush_held());
        }
        effects
    }

    /// How long from now the next write falls due, or `None` when nothing is
    /// waiting on the clock (everything behind an open modal waits on the
    /// modal).
    pub(crate) fn next_wakeup(&self) -> Option<Duration> {
        if self.open_modals > 0 {
            return None;
        }
        [
            self.polite_set.as_ref().map(|pending| pending.due),
            self.assertive_set.as_ref().map(|pending| pending.due),
            self.window_ends,
        ]
        .into_iter()
        .flatten()
        .min()
        .map(|due| due.saturating_sub(self.clock.now()))
    }

    fn pending_set(&mut self, politeness: Politeness) -> &mut Option<PendingSet> {
        match politeness {
            Politeness::Polite => &mut self.polite_set,
            Politeness::Assertive => &mut self.assertive_set,
        }
    }

    /// Ends the window: the held texts become one write.
    fn flush_held(&mut self) -> Vec<Effect> {
        self.window_ends = None;
        if self.held.is_empty() {
            return Vec::new();
        }
        let text = std::mem::take(&mut self.held).join(" ");
        self.schedule_set(Politeness::Polite, text)
    }

    /// Writes the assertive texts that waited for a modal to close.
    fn flush_held_assertive(&mut self) -> Vec<Effect> {
        if self.held_assertive.is_empty() {
            return Vec::new();
        }
        let text = std::mem::take(&mut self.held_assertive).join(" ");
        self.schedule_set(Politeness::Assertive, text)
    }

    /// Clears the region now and sets `text` a tick later. A region still
    /// waiting to be set takes the text into that set instead, so no text is
    /// lost.
    fn schedule_set(&mut self, politeness: Politeness, text: String) -> Vec<Effect> {
        let due = self.clock.now() + SET_DELAY;
        match self.pending_set(politeness) {
            Some(pending) => {
                if pending.text != text {
                    pending.text.push(' ');
                    pending.text.push_str(&text);
                }
                Vec::new()
            }
            slot @ None => {
                *slot = Some(PendingSet { due, text });
                vec![Effect::Clear(politeness)]
            }
        }
    }
}

/// Holds `text` for the next write: in arrival order, a text already held
/// once, and no more than [`MAX_HELD`].
fn hold(held: &mut Vec<String>, text: String) {
    if held.len() < MAX_HELD && !held.contains(&text) {
        held.push(text);
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

        fn reply(&mut self, text: &str) -> Vec<Effect> {
            self.announcer.announce(Announcement {
                kind: Kind::Reply,
                politeness: Polite,
                text: text.to_owned(),
            })
        }

        fn pause(&mut self) -> Vec<Effect> {
            self.announcer.pause("Live updates are paused.".to_owned())
        }

        fn resume(&mut self, summary: &str) -> Vec<Effect> {
            self.announcer.resume(Some(summary.to_owned()))
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
    fn an_assertive_message_waits_for_a_modal_to_close_like_any_other() {
        // The modal makes the regions inert, and VoiceOver and JAWS say
        // nothing for a live region in inert content: the message would be
        // lost, and it is the urgent one.
        let mut page = Page::new();
        page.announcer.modal_opened();
        assert_eq!(page.say(Assertive, "Restore failed."), []);
        assert_eq!(page.writes_between(1, 10_000), []);
        assert_eq!(page.announcer.next_wakeup(), None);
        page.at(10_000);
        assert_eq!(page.announcer.modal_closed(), [Clear(Assertive)]);
        assert_eq!(page.poll_at(10_049), []);
        assert_eq!(page.poll_at(10_050), [set(Assertive, "Restore failed.")]);
    }

    #[test]
    fn held_assertive_messages_are_merged_and_written_before_the_polite_ones() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        page.polite("Backup finished.");
        page.say(Assertive, "Restore failed.");
        page.say(Assertive, "Prune failed.");
        page.say(Assertive, "Restore failed.");
        assert_eq!(
            page.announcer.modal_closed(),
            [Clear(Assertive), Clear(Polite)]
        );
        assert_eq!(
            page.poll_at(50),
            [
                set(Assertive, "Restore failed. Prune failed."),
                set(Polite, "Backup finished.")
            ]
        );
    }

    #[test]
    fn assertive_messages_wait_for_the_last_of_nested_modals() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        page.announcer.modal_opened();
        page.say(Assertive, "Restore failed.");
        assert_eq!(page.announcer.modal_closed(), []);
        assert_eq!(page.poll_at(5_000), []);
        assert_eq!(page.announcer.modal_closed(), [Clear(Assertive)]);
    }

    #[test]
    fn a_modal_left_open_holds_at_most_a_bounded_number_of_assertive_messages() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        for number in 0..MAX_HELD + 5 {
            page.say(Assertive, &format!("Failure {number}."));
        }
        assert_eq!(page.announcer.modal_closed(), [Clear(Assertive)]);
        let expected = (0..MAX_HELD)
            .map(|number| format!("Failure {number}."))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(page.poll_at(50), [set(Assertive, &expected)]);
    }

    #[test]
    fn a_write_waiting_when_a_modal_opens_is_made_after_it_closes() {
        let mut page = Page::new();
        assert_eq!(page.say(Assertive, "Restore failed."), [Clear(Assertive)]);
        page.at(10);
        page.announcer.modal_opened();
        assert_eq!(page.writes_between(11, 5_000), []);
        assert_eq!(page.announcer.next_wakeup(), None);
        page.at(6_000);
        assert_eq!(page.announcer.modal_closed(), []);
        assert_eq!(page.announcer.next_wakeup(), Some(Duration::ZERO));
        assert_eq!(page.poll_at(6_000), [set(Assertive, "Restore failed.")]);
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

    #[test]
    fn pausing_says_so_at_once_without_a_merge_window() {
        let mut page = Page::new();
        assert_eq!(page.pause(), [Clear(Polite)]);
        assert_eq!(
            page.writes_between(1, 5_000),
            [(50, set(Polite, "Live updates are paused."))]
        );
    }

    #[test]
    fn an_event_while_paused_is_not_said() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        assert_eq!(page.polite("Backup finished."), []);
        assert_eq!(page.say(Assertive, "Backup failed."), []);
        assert_eq!(page.writes_between(101, 10_000), []);
        assert_eq!(page.announcer.next_wakeup(), None);
    }

    #[test]
    fn a_reply_while_paused_is_said_as_usual() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        page.reply("Sorted by started, newest first.");
        assert_eq!(
            page.writes_between(101, 5_000),
            [
                (2_100, Clear(Polite)),
                (2_150, set(Polite, "Sorted by started, newest first.")),
            ]
        );
    }

    #[test]
    fn resuming_says_the_summary_then_what_waited_in_one_write_at_once() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        page.polite("Still waiting.");
        page.polite("Agent back online.");
        page.at(5_000);
        assert_eq!(page.resume("Live updates are on again."), [Clear(Polite)]);
        assert_eq!(
            page.writes_between(5_001, 10_000),
            [(
                5_050,
                set(
                    Polite,
                    "Live updates are on again. Still waiting. Agent back online."
                )
            )]
        );
    }

    #[test]
    fn resuming_says_what_waited_assertively_first() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        page.say(Assertive, "Restore failed.");
        page.at(5_000);
        assert_eq!(
            page.resume("Live updates are on again."),
            [Clear(Assertive), Clear(Polite)]
        );
        assert_eq!(
            page.poll_at(5_050),
            [
                set(Assertive, "Restore failed."),
                set(Polite, "Live updates are on again."),
            ]
        );
    }

    #[test]
    fn after_a_resume_events_are_said_again() {
        let mut page = Page::new();
        page.pause();
        page.at(1_000);
        page.resume("Live updates are on again.");
        page.writes_between(1_001, 1_100);
        page.polite("Backup finished.");
        assert_eq!(
            page.writes_between(1_101, 5_000),
            [
                (3_100, Clear(Polite)),
                (3_150, set(Polite, "Backup finished.")),
            ]
        );
    }

    #[test]
    fn a_reply_still_in_its_window_joins_the_resumes_write() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        page.reply("Sorted by started, newest first.");
        page.at(500);
        assert_eq!(page.resume("Live updates are on again."), [Clear(Polite)]);
        assert_eq!(
            page.writes_between(501, 5_000),
            [(
                550,
                set(
                    Polite,
                    "Live updates are on again. Sorted by started, newest first."
                )
            )]
        );
    }

    #[test]
    fn a_resume_while_not_paused_is_ignored() {
        let mut page = Page::new();
        assert_eq!(page.resume("Live updates are on again."), []);
        page.polite("Backup finished.");
        assert_eq!(page.poll_at(1_999), []);
        assert_eq!(page.poll_at(2_000), [Clear(Polite)]);
        assert_eq!(page.poll_at(2_050), [set(Polite, "Backup finished.")]);
    }

    #[test]
    fn a_resume_with_nothing_to_say_writes_nothing() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        assert_eq!(page.announcer.resume(None), []);
        assert_eq!(page.writes_between(101, 5_000), []);
    }

    #[test]
    fn a_resume_behind_an_open_modal_waits_for_it_to_close() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        page.polite("Backup finished.");
        page.announcer.modal_opened();
        assert_eq!(page.resume("Live updates are on again."), []);
        assert_eq!(page.writes_between(101, 5_000), []);
        assert_eq!(page.announcer.modal_closed(), [Clear(Polite)]);
        assert_eq!(
            page.poll_at(5_050),
            [set(Polite, "Live updates are on again. Backup finished.")]
        );
    }

    #[test]
    fn what_waits_while_paused_is_said_once_and_bounded() {
        let mut page = Page::new();
        page.pause();
        page.writes_between(1, 100);
        page.polite("Backup finished.");
        for number in 0..MAX_HELD + 5 {
            page.polite(&format!("Message {number}."));
            page.say(Assertive, &format!("Failure {number}."));
        }
        page.at(1_000);
        assert_eq!(
            page.resume("Live updates are on again."),
            [Clear(Assertive), Clear(Polite)]
        );
        let polite = ["Live updates are on again.", "Backup finished."]
            .into_iter()
            .map(str::to_owned)
            .chain((0..MAX_HELD - 2).map(|number| format!("Message {number}.")))
            .collect::<Vec<_>>()
            .join(" ");
        let assertive = (0..MAX_HELD)
            .map(|number| format!("Failure {number}."))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            page.poll_at(1_050),
            [set(Assertive, &assertive), set(Polite, &polite)]
        );
    }

    #[test]
    fn a_pause_behind_an_open_modal_says_so_when_it_closes() {
        let mut page = Page::new();
        page.announcer.modal_opened();
        assert_eq!(page.pause(), []);
        assert_eq!(page.announcer.modal_closed(), [Clear(Polite)]);
        assert_eq!(page.poll_at(50), [set(Polite, "Live updates are paused.")]);
    }

    #[test]
    fn a_resume_before_a_pause_joined_to_an_event_is_written_drops_only_the_pause() {
        let mut page = Page::new();
        assert!(page.polite("Backup finished.").is_empty());
        page.at(1_000);
        assert_eq!(page.pause(), [Clear(Polite)]);
        page.at(1_020);
        assert!(page.resume("Live updates are on again.").is_empty());
        assert_eq!(
            page.writes_between(1_021, 5_000),
            [(
                1_050,
                set(Polite, "Backup finished. Live updates are on again.")
            )]
        );
    }
}
