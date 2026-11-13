//! What a progress bar tells a screen reader, and when (technical design
//! §4.5; §16, decision 83). The bar is drawn from every report, so it moves
//! smoothly; what it exposes, its `aria-valuenow` and `aria-valuetext`, is
//! told at most every [`HOLD`], and at once when the step changes, when the
//! bar learns or loses its total, and when it reaches its total. The time
//! left is told once the step has a pace worth telling.
//!
//! Pure: the clock is the caller's, a [`Duration`] since any fixed instant
//! (the page's `performance.now()`), so `cargo nextest run -p web` tests it
//! on the host. The glue that holds the one timer and writes the attributes
//! is `components.rs`'s `Progress`.

use std::time::Duration;

use api::{MsgArg, OperationStep, ProgressUnit};
use l10n::MessageId;

use crate::backend::Progress;

/// The least time between two values told to a screen reader, while the
/// step stays the same.
pub(crate) const HOLD: Duration = Duration::from_secs(30);

/// How long a step is watched before its pace tells the time left.
pub(crate) const PACE_AFTER: Duration = Duration::from_secs(30);

/// How long is left, as the value text says it: under a minute, or about so
/// many whole minutes or hours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Left {
    Soon,
    Minutes(u64),
    Hours(u64),
}

/// How fast the step a bar measures goes, from the first report of it the
/// page saw to the latest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pace;

impl Pace {
    /// Takes the report `progress`, seen at `now`.
    pub(crate) fn observe(&mut self, progress: &Progress, now: Duration) {
        let _ = (progress, now);
        todo!()
    }

    /// The time left, once there is a pace to tell it by.
    pub(crate) fn left(&self) -> Option<Left> {
        todo!()
    }
}

/// What a bar tells a screen reader: its step, and how far it has come
/// when it knows its total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) step: Option<OperationStep>,
    pub(crate) measure: Option<Measure>,
}

/// How far a bar with a total has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Measure {
    pub(crate) percent: u8,
    pub(crate) current: u64,
    pub(crate) total: u64,
    pub(crate) unit: ProgressUnit,
    pub(crate) left: Option<Left>,
}

/// What the bar does with what it exposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Exposes this, now.
    Tell(Told),
    /// Keeps what it exposes until this instant, then asks again.
    WaitUntil(Duration),
    /// Keeps what it exposes: nothing new to tell.
    Keep,
}

/// What a bar exposes, and when it may change it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Teller;

impl Teller {
    /// The report `progress` arrives at `now`.
    pub(crate) fn report(&mut self, progress: Progress, now: Duration) -> Next {
        let _ = (progress, now);
        todo!()
    }

    /// The wait the last answer asked for is over at `now`.
    pub(crate) fn wake(&mut self, now: Duration) -> Next {
        let _ = now;
        todo!()
    }
}

/// How long a timer set at `now` waits for the instant `due`.
pub(crate) fn wait_for(due: Duration, now: Duration) -> Duration {
    let _ = (due, now);
    todo!()
}

/// The value text of `told`, worded by `word`: the step and the working
/// text when there is no total, otherwise the step, the percentage, the
/// amounts and the time left once known.
pub(crate) fn value_text(
    told: &Told,
    word: impl Fn(MessageId, &[(&str, MsgArg)]) -> String,
) -> String {
    let _ = (told, word);
    todo!()
}

/// What clears a timer the page set: the handle it gave.
pub(crate) trait Clear {
    fn clear(self);
}

/// The one timer a bar waits on for its next value: a newer one replaces
/// it, and the bar's going clears it, so a bar that is gone leaves no timer
/// to fire.
#[derive(Debug)]
pub(crate) struct Wake<H: Clear> {
    pending: Option<H>,
}

impl<H: Clear> Default for Wake<H> {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl<H: Clear> Wake<H> {
    /// Waits on `handle`'s timer, clearing the one waited on before.
    pub(crate) fn set(&mut self, handle: H) {
        let _ = handle;
        todo!()
    }

    /// Clears the timer waited on, if any: the bar told what it held, or
    /// is going.
    pub(crate) fn clear(&mut self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use api::{OperationStep, ProgressUnit};
    use l10n::Localizer;

    use super::*;
    use crate::routes::text_with;

    const GIB: u64 = 1 << 30;

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn report(step: Option<OperationStep>, current: u64, total: Option<u64>) -> Progress {
        Progress {
            step,
            unit: ProgressUnit::Count,
            current,
            total,
        }
    }

    fn backing_up(current: u64, total: u64) -> Progress {
        report(Some(OperationStep::BackingUp), current, Some(total))
    }

    /// The pace after `first` at 0 s and `then` at `seconds`.
    fn pace(first: Progress, then: Progress, seconds: u64) -> Option<Left> {
        let mut pace = Pace::default();
        pace.observe(&first, at(0));
        pace.observe(&then, at(seconds));
        pace.left()
    }

    /// 2 GiB in a minute, 16 GiB to go: about 8 minutes, from the average
    /// pace since the step was first seen, not the last report's.
    #[test]
    fn the_time_left_is_the_rest_at_the_average_pace_since_the_step_was_first_seen() {
        let mut pace = Pace::default();
        pace.observe(&backing_up(2 * GIB, 20 * GIB), at(0));
        pace.observe(&backing_up(3 * GIB, 20 * GIB), at(50));
        pace.observe(&backing_up(4 * GIB, 20 * GIB), at(60));
        assert_eq!(pace.left(), Some(Left::Minutes(8)));
    }

    #[test]
    fn the_time_left_is_rounded_to_the_nearest_minute_and_under_one_is_soon() {
        // 1 of 100 in 30 s leaves 99 × 30 s, 49.5 minutes.
        assert_eq!(
            pace(backing_up(0, 100), backing_up(1, 100), 30),
            Some(Left::Minutes(50))
        );
        // 89.5 s rounds to one minute; 90 s to two.
        assert_eq!(
            pace(backing_up(0, 1000), backing_up(800, 1000), 358),
            Some(Left::Minutes(1))
        );
        assert_eq!(
            pace(backing_up(0, 1000), backing_up(800, 1000), 360),
            Some(Left::Minutes(2))
        );
        // A minute is a minute; anything under is soon.
        assert_eq!(
            pace(backing_up(0, 100), backing_up(50, 100), 60),
            Some(Left::Minutes(1))
        );
        assert_eq!(
            pace(backing_up(0, 100), backing_up(50, 100), 59),
            Some(Left::Soon)
        );
        assert_eq!(
            pace(backing_up(0, 100), backing_up(75, 100), 100),
            Some(Left::Soon)
        );
    }

    #[test]
    fn an_hour_or_more_left_is_told_in_whole_hours() {
        // 2 of 121 a minute leaves 119 × 30 s, 59 min 30 s: about an hour.
        assert_eq!(
            pace(backing_up(0, 121), backing_up(2, 121), 60),
            Some(Left::Hours(1))
        );
        // 1 of 100 a minute: 99 minutes, about 2 hours.
        assert_eq!(
            pace(backing_up(0, 100), backing_up(1, 100), 60),
            Some(Left::Hours(2))
        );
        // 1 h 29 min: about 1 hour.
        assert_eq!(
            pace(backing_up(0, 90), backing_up(1, 90), 60),
            Some(Left::Hours(1))
        );
        // 59 min, short of the half minute that rounds to an hour: minutes.
        assert_eq!(
            pace(backing_up(0, 60), backing_up(1, 60), 60),
            Some(Left::Minutes(59))
        );
    }

    #[test]
    fn no_time_left_is_told_before_the_step_has_gone_on_for_long_enough() {
        assert_eq!(pace(backing_up(0, 100), backing_up(50, 100), 29), None);
        let mut pace = Pace::default();
        pace.observe(&backing_up(0, 100), at(0));
        assert_eq!(pace.left(), None);
    }

    #[test]
    fn no_time_left_is_told_before_the_step_has_done_a_hundredth_of_its_total() {
        assert_eq!(pace(backing_up(0, 1000), backing_up(9, 1000), 600), None);
        assert_eq!(
            pace(backing_up(0, 1000), backing_up(10, 1000), 600),
            Some(Left::Hours(17))
        );
        assert_eq!(pace(backing_up(5, 100), backing_up(5, 100), 600), None);
    }

    #[test]
    fn no_time_left_is_told_without_a_total_or_once_the_total_is_reached() {
        assert_eq!(
            pace(
                report(Some(OperationStep::BackingUp), 0, None),
                report(Some(OperationStep::BackingUp), 50, None),
                60
            ),
            None
        );
        assert_eq!(pace(backing_up(0, 0), backing_up(0, 0), 60), None);
        assert_eq!(pace(backing_up(0, 100), backing_up(100, 100), 60), None);
        assert_eq!(pace(backing_up(0, 100), backing_up(150, 100), 60), None);
    }

    /// A new step, a new total or a count that goes back is a new measure:
    /// the pace starts again from it.
    #[test]
    fn the_pace_starts_again_with_a_new_step_a_new_total_or_a_count_that_goes_back() {
        let reading = report(Some(OperationStep::ReadingIndex), 0, Some(100));
        let mut new_step = Pace::default();
        new_step.observe(&reading, at(0));
        new_step.observe(&backing_up(10, 100), at(60));
        assert_eq!(new_step.left(), None);
        new_step.observe(&backing_up(20, 100), at(90));
        assert_eq!(new_step.left(), Some(Left::Minutes(4)));

        let mut new_total = Pace::default();
        new_total.observe(&backing_up(0, 100), at(0));
        new_total.observe(&backing_up(10, 200), at(60));
        assert_eq!(new_total.left(), None);

        let mut back = Pace::default();
        back.observe(&backing_up(50, 100), at(0));
        back.observe(&backing_up(10, 100), at(60));
        assert_eq!(back.left(), None);
        back.observe(&backing_up(20, 100), at(90));
        assert_eq!(back.left(), Some(Left::Minutes(4)));
    }

    fn measured(progress: Progress, percent: u8, left: Option<Left>) -> Next {
        Next::Tell(Told {
            step: progress.step,
            measure: Some(Measure {
                percent,
                current: progress.current,
                total: progress.total.unwrap_or_default(),
                unit: progress.unit,
                left,
            }),
        })
    }

    #[test]
    fn the_first_report_is_told_at_once() {
        let mut teller = Teller::default();
        assert_eq!(
            teller.report(backing_up(2, 10), at(100)),
            measured(backing_up(2, 10), 20, None)
        );
    }

    /// While the step stays the same, a change waits until 30 s after the
    /// value last told, and then the latest report is told.
    #[test]
    fn a_change_within_thirty_seconds_of_the_last_value_told_waits_for_them() {
        let mut teller = Teller::default();
        teller.report(backing_up(2, 10), at(100));
        assert_eq!(
            teller.report(backing_up(3, 10), at(110)),
            Next::WaitUntil(at(130))
        );
        assert_eq!(
            teller.report(backing_up(4, 10), at(129)),
            Next::WaitUntil(at(130))
        );
        assert_eq!(teller.wake(at(129)), Next::WaitUntil(at(130)));
        assert_eq!(teller.wake(at(130)), measured(backing_up(4, 10), 40, None));
        // The next 30 s run from that value.
        assert_eq!(
            teller.report(backing_up(5, 10), at(131)),
            Next::WaitUntil(at(160))
        );
    }

    /// A report back at the value the bar exposes drops the value held
    /// before it, so the end of the hold tells nothing stale.
    #[test]
    fn a_report_back_at_the_value_told_drops_the_value_held_before_it() {
        let mut teller = Teller::default();
        teller.report(backing_up(2, 10), at(100));
        assert_eq!(
            teller.report(backing_up(3, 10), at(110)),
            Next::WaitUntil(at(130))
        );
        assert_eq!(teller.report(backing_up(2, 10), at(115)), Next::Keep);
        assert_eq!(teller.wake(at(130)), Next::Keep);
    }

    /// A browser's timer counts whole milliseconds, so the wait is rounded
    /// up: a timer that fired before the hold is over would only be set again.
    #[test]
    fn a_wait_lasts_until_the_hold_is_over_in_whole_milliseconds() {
        let now = Duration::from_millis(10_000);
        assert_eq!(
            wait_for(now + Duration::from_micros(19_999_700), now),
            Duration::from_millis(20_000)
        );
        assert_eq!(
            wait_for(now + Duration::from_millis(20_000), now),
            Duration::from_millis(20_000)
        );
        assert_eq!(wait_for(now, now + Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn a_change_thirty_seconds_or_more_after_the_last_value_told_is_told_at_once() {
        let mut teller = Teller::default();
        teller.report(backing_up(2, 10), at(100));
        // 1 in 30 s, 7 to go: 3.5 minutes.
        assert_eq!(
            teller.report(backing_up(3, 10), at(130)),
            measured(backing_up(3, 10), 30, Some(Left::Minutes(4)))
        );
        // 2 in 400 s, 6 to go: 20 minutes.
        assert_eq!(
            teller.report(backing_up(4, 10), at(500)),
            measured(backing_up(4, 10), 40, Some(Left::Minutes(20)))
        );
    }

    /// Nothing new to tell, nothing to wait for: an unknown size counting on
    /// says the same thing, and so does the same report again.
    #[test]
    fn a_report_that_tells_nothing_new_keeps_what_is_told() {
        let mut teller = Teller::default();
        let working = |current| report(Some(OperationStep::BackingUp), current, None);
        assert_eq!(
            teller.report(working(8), at(100)),
            Next::Tell(Told {
                step: Some(OperationStep::BackingUp),
                measure: None,
            })
        );
        assert_eq!(teller.report(working(4096), at(101)), Next::Keep);
        assert_eq!(teller.wake(at(200)), Next::Keep);
        let mut same = Teller::default();
        same.report(backing_up(2, 10), at(100));
        assert_eq!(same.report(backing_up(2, 10), at(101)), Next::Keep);
    }

    #[test]
    fn a_new_step_is_told_at_once() {
        let mut teller = Teller::default();
        teller.report(
            report(Some(OperationStep::ReadingIndex), 3, Some(10)),
            at(100),
        );
        assert_eq!(
            teller.report(backing_up(1, 10), at(101)),
            measured(backing_up(1, 10), 10, None)
        );
        let none = report(None, 2, Some(10));
        assert_eq!(teller.report(none, at(102)), measured(none, 20, None));
    }

    /// A total learnt or lost changes what the bar is: a percentage, or the
    /// working text in its place.
    #[test]
    fn a_total_learnt_or_lost_is_told_at_once() {
        let mut teller = Teller::default();
        teller.report(report(Some(OperationStep::BackingUp), 8, None), at(100));
        assert_eq!(
            teller.report(backing_up(1, 4), at(101)),
            measured(backing_up(1, 4), 25, None)
        );
        assert_eq!(
            teller.report(report(Some(OperationStep::BackingUp), 9, None), at(102)),
            Next::Tell(Told {
                step: Some(OperationStep::BackingUp),
                measure: None,
            })
        );
    }

    #[test]
    fn the_end_of_a_step_is_told_at_once() {
        let mut teller = Teller::default();
        teller.report(backing_up(1, 4), at(100));
        assert_eq!(
            teller.report(backing_up(4, 4), at(101)),
            measured(backing_up(4, 4), 100, None)
        );
    }

    /// The time left joins the value told once the pace can say it, and
    /// waits for the hold as any other change does.
    #[test]
    fn the_time_left_is_told_with_the_value_once_known() {
        let mut teller = Teller::default();
        teller.report(backing_up(2 * GIB, 20 * GIB), at(0));
        assert_eq!(
            teller.report(backing_up(4 * GIB, 20 * GIB), at(60)),
            measured(backing_up(4 * GIB, 20 * GIB), 20, Some(Left::Minutes(8)))
        );
    }

    /// What `told` reads in English, without the isolation marks a reader
    /// never hears.
    fn english(told: &Told) -> String {
        let localizer = Localizer::new(&[]);
        value_text(told, |id, args| text_with(&localizer, id, args))
            .replace(['\u{2068}', '\u{2069}'], "")
    }

    fn told(
        step: Option<OperationStep>,
        unit: ProgressUnit,
        (percent, current, total): (u8, u64, u64),
        left: Option<Left>,
    ) -> Told {
        Told {
            step,
            measure: Some(Measure {
                percent,
                current,
                total,
                unit,
                left,
            }),
        }
    }

    #[test]
    fn the_value_text_says_the_step_the_percentage_the_amounts_and_the_time_left() {
        let backing_up = Some(OperationStep::BackingUp);
        assert_eq!(
            english(&told(
                backing_up,
                ProgressUnit::Bytes,
                (42, 3_328_599_655, 7_945_689_498),
                Some(Left::Minutes(6))
            )),
            "Backing up… 42%, 3.1 GiB of 7.4 GiB, about 6 minutes left"
        );
        assert_eq!(
            english(&told(
                Some(OperationStep::ReadingIndex),
                ProgressUnit::Count,
                (30, 3, 10),
                None
            )),
            "Reading the index… 30%, 3 of 10"
        );
        assert_eq!(
            english(&told(
                backing_up,
                ProgressUnit::Bytes,
                (99, 99, 100),
                Some(Left::Soon)
            )),
            "Backing up… 99%, 99 B of 100 B, less than a minute left"
        );
        assert_eq!(
            english(&told(
                backing_up,
                ProgressUnit::Count,
                (1, 1, 100),
                Some(Left::Minutes(1))
            )),
            "Backing up… 1%, 1 of 100, about 1 minute left"
        );
        assert_eq!(
            english(&told(
                backing_up,
                ProgressUnit::Count,
                (1, 1, 100),
                Some(Left::Hours(1))
            )),
            "Backing up… 1%, 1 of 100, about 1 hour left"
        );
        assert_eq!(
            english(&told(
                backing_up,
                ProgressUnit::Count,
                (1, 1, 100),
                Some(Left::Hours(3))
            )),
            "Backing up… 1%, 1 of 100, about 3 hours left"
        );
    }

    /// No step this build knows: the value text starts at the percentage. A
    /// unit it doesn't know is a plain number.
    #[test]
    fn the_value_text_without_a_step_or_a_unit_this_build_knows() {
        assert_eq!(
            english(&told(None, ProgressUnit::Count, (25, 1, 4), None)),
            "25%, 1 of 4"
        );
        assert_eq!(
            english(&told(
                Some(OperationStep::Unknown),
                ProgressUnit::Unknown,
                (25, 100, 400),
                None
            )),
            "25%, 100 of 400"
        );
    }

    /// Without a total the value text is the step, or that the operation is
    /// working: never a percentage (#422).
    #[test]
    fn the_value_text_without_a_total_is_the_step_or_working() {
        assert_eq!(
            english(&Told {
                step: Some(OperationStep::ReadingIndex),
                measure: None,
            }),
            "Reading the index…"
        );
        assert_eq!(
            english(&Told {
                step: None,
                measure: None,
            }),
            "Working…"
        );
    }

    /// A timer the page set, as its handle: what clears it marks it so.
    #[derive(Debug)]
    struct Timer(std::rc::Rc<std::cell::Cell<bool>>);

    impl Clear for Timer {
        fn clear(self) {
            self.0.set(true);
        }
    }

    fn timer() -> (Timer, std::rc::Rc<std::cell::Cell<bool>>) {
        let cleared = std::rc::Rc::new(std::cell::Cell::new(false));
        (Timer(std::rc::Rc::clone(&cleared)), cleared)
    }

    /// A bar waits on one timer at a time: a new wait clears the one before.
    #[test]
    fn a_new_wait_clears_the_timer_waited_on_before() {
        let (first, first_cleared) = timer();
        let (second, second_cleared) = timer();
        let mut wake = Wake::default();
        wake.set(first);
        assert!(!first_cleared.get());
        wake.set(second);
        assert!(first_cleared.get());
        assert!(!second_cleared.get());
    }

    /// A row that goes away, or a bar that tells at once what it held, leaves
    /// no timer to fire; clearing again clears nothing more.
    #[test]
    fn a_bar_that_goes_away_leaves_no_timer_to_fire() {
        let (waited, cleared) = timer();
        let mut wake = Wake::default();
        wake.set(waited);
        wake.clear();
        assert!(cleared.get());
        wake.clear();
        let (next, next_cleared) = timer();
        wake.set(next);
        assert!(!next_cleared.get());
    }
}
