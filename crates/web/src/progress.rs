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
use l10n::{MessageId, msg};

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
pub(crate) struct Pace {
    /// The first report of this measure the page saw, and the latest: a
    /// new step, a new total or a count that went back is a new measure.
    measure: Option<(Seen, Seen)>,
}

/// A report, and when the page saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    step: Option<OperationStep>,
    total: Option<u64>,
    current: u64,
    at: Duration,
}

impl Pace {
    /// Takes the report `progress`, seen at `now`.
    pub(crate) fn observe(&mut self, progress: &Progress, now: Duration) {
        let seen = Seen {
            step: progress.step,
            total: progress.total,
            current: progress.current,
            at: now,
        };
        let first = match self.measure {
            Some((first, latest))
                if latest.step == seen.step
                    && latest.total == seen.total
                    && latest.current <= seen.current =>
            {
                first
            }
            _ => seen,
        };
        self.measure = Some((first, seen));
    }

    /// The time left, once there is a pace to tell it by: the rest of the
    /// total at the average pace since the measure was first seen, once it
    /// has been watched for [`PACE_AFTER`] and has done a hundredth of its
    /// total since.
    pub(crate) fn left(&self) -> Option<Left> {
        const MINUTE_MS: u128 = 60_000;
        const HOUR_MS: u128 = 60 * MINUTE_MS;
        let (first, latest) = self.measure?;
        let total = latest.total.filter(|total| *total > latest.current)?;
        let watched = latest.at.checked_sub(first.at)?;
        let done = latest.current.checked_sub(first.current)?;
        if watched < PACE_AFTER || done == 0 || u128::from(done) * 100 < u128::from(total) {
            return None;
        }
        let rest = u128::from(total - latest.current);
        let left_ms = rest * watched.as_millis() / u128::from(done);
        let whole = |unit: u128| u64::try_from((left_ms + unit / 2) / unit).unwrap_or(u64::MAX);
        Some(if left_ms < MINUTE_MS {
            Left::Soon
        } else if left_ms + MINUTE_MS / 2 < HOUR_MS {
            Left::Minutes(whole(MINUTE_MS))
        } else {
            Left::Hours(whole(HOUR_MS))
        })
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
pub(crate) struct Teller {
    pace: Pace,
    /// What the bar exposes, and since when.
    told: Option<(Told, Duration)>,
    /// The latest value, held back until [`HOLD`] after the last told.
    held: Option<Told>,
}

impl Teller {
    /// The report `progress` arrives at `now`.
    pub(crate) fn report(&mut self, progress: Progress, now: Duration) -> Next {
        self.pace.observe(&progress, now);
        let value = told_of(&progress, self.pace.left());
        let Some((exposed, since)) = self.told else {
            return self.tell(value, now);
        };
        if value == exposed {
            self.held = None;
            return Next::Keep;
        }
        let due = since.saturating_add(HOLD);
        if now >= due || at_once(&exposed, &value) {
            return self.tell(value, now);
        }
        self.held = Some(value);
        Next::WaitUntil(due)
    }

    /// The wait the last answer asked for is over at `now`.
    pub(crate) fn wake(&mut self, now: Duration) -> Next {
        let (Some(held), Some((_, since))) = (self.held, self.told) else {
            return Next::Keep;
        };
        let due = since.saturating_add(HOLD);
        if now >= due {
            self.tell(held, now)
        } else {
            Next::WaitUntil(due)
        }
    }

    fn tell(&mut self, value: Told, now: Duration) -> Next {
        self.told = Some((value, now));
        self.held = None;
        Next::Tell(value)
    }
}

/// How long a timer set at `now` waits for the instant `due`: rounded up to
/// whole milliseconds, the browser timer's unit, so it never fires before
/// the hold is over only to be set again; nothing once `due` has passed.
pub(crate) fn wait_for(due: Duration, now: Duration) -> Duration {
    let wait = due.saturating_sub(now);
    match wait.subsec_nanos() % 1_000_000 {
        0 => wait,
        part => wait.saturating_add(Duration::from_nanos(u64::from(1_000_000 - part))),
    }
}

/// What `progress` tells, with the time `left` when it has a total.
fn told_of(progress: &Progress, left: Option<Left>) -> Told {
    Told {
        step: progress.step,
        measure: progress
            .total
            .zip(percent(progress.current, progress.total))
            .map(|(total, percent)| Measure {
                percent,
                current: progress.current,
                total,
                unit: progress.unit,
                left,
            }),
    }
}

/// Whether `value` is told at once, whatever the hold: a new step, a total
/// learnt or lost, or the total reached.
fn at_once(exposed: &Told, value: &Told) -> bool {
    let reached = |told: &Told| {
        told.measure
            .is_some_and(|measure| measure.current >= measure.total)
    };
    exposed.step != value.step
        || exposed.measure.is_some() != value.measure.is_some()
        || (reached(value) && !reached(exposed))
}

/// The value text of `told`, worded by `word`: the step and the working
/// text when there is no total, otherwise the step, the percentage, the
/// amounts and the time left once known. The step is its own message, a
/// whole clause, which the value's message places: the code joins nothing.
pub(crate) fn value_text(
    told: &Told,
    word: impl Fn(MessageId, &[(&str, MsgArg)]) -> String,
) -> String {
    let step = told.step.and_then(step_message);
    let Some(measure) = told.measure else {
        return word(step.unwrap_or(msg::UI_PROGRESS_WORKING), &[]);
    };
    let amount = |amount| match measure.unit {
        ProgressUnit::Bytes => MsgArg::Bytes(amount),
        ProgressUnit::Count | ProgressUnit::Unknown => MsgArg::Count(amount),
    };
    let (left, minutes, hours) = match measure.left {
        None => ("none", 0, 0),
        Some(Left::Soon) => ("soon", 0, 0),
        Some(Left::Minutes(minutes)) => ("minutes", minutes, 0),
        Some(Left::Hours(hours)) => ("hours", 0, hours),
    };
    word(
        msg::UI_PROGRESS_VALUE,
        &[
            (
                "step",
                MsgArg::Text(step.map_or_else(|| "none".to_owned(), |step| word(step, &[]))),
            ),
            ("percent", MsgArg::Count(u64::from(measure.percent))),
            ("done", amount(measure.current)),
            ("total", amount(measure.total)),
            ("left", MsgArg::Text(left.to_owned())),
            ("minutes", MsgArg::Count(minutes)),
            ("hours", MsgArg::Count(hours)),
        ],
    )
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
        if let Some(before) = self.pending.replace(handle) {
            before.clear();
        }
    }

    /// Clears the timer waited on, if any: the bar told what it held, or
    /// is going.
    pub(crate) fn clear(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.clear();
        }
    }
}

/// A run's progress as a whole percent of its total. A total of nothing or
/// zero cannot be a share, so there is none rather than a wrong one. A count
/// past its total is all of it: both come from a worker's report, so the
/// count is held to the total first, and the product is taken in `u128`,
/// where a `u64` times 100 can't overflow.
pub(crate) fn percent(current: u64, total: Option<u64>) -> Option<u8> {
    total.filter(|total| *total > 0).map(|total| {
        let share = u128::from(current.min(total)) * 100 / u128::from(total);
        u8::try_from(share).unwrap_or(100)
    })
}

/// What a bar's Status cell says of its run's progress besides the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProgressText {
    /// The step's message above the bar, while the bar shows a percentage.
    pub(crate) above: Option<MessageId>,
    /// What stands beside the bar while it shows no percentage: the step's
    /// message, or that the run is working when it names none this build
    /// knows.
    pub(crate) unknown: MessageId,
}

/// What `progress` says of the step its run is in (§4.4; §16, decision 82).
pub(crate) fn progress_text(progress: &Progress) -> ProgressText {
    let step = progress.step.and_then(step_message);
    let with_percent = percent(progress.current, progress.total).is_some();
    ProgressText {
        above: step.filter(|_| with_percent),
        unknown: step.unwrap_or(msg::UI_PROGRESS_WORKING),
    }
}

/// The message of `step`; `None` for one this build doesn't know.
pub(crate) fn step_message(step: api::OperationStep) -> Option<MessageId> {
    use api::OperationStep as S;
    Some(match step {
        S::ReadingIndex => msg::UI_STEP_READING_INDEX,
        S::GettingSnapshots => msg::UI_STEP_GETTING_SNAPSHOTS,
        S::BackingUp => msg::UI_STEP_BACKING_UP,
        S::ListingPacks => msg::UI_STEP_LISTING_PACKS,
        S::CheckingCache => msg::UI_STEP_CHECKING_CACHE,
        S::CheckingHotRepository => msg::UI_STEP_CHECKING_HOT_REPOSITORY,
        S::CheckingTrees => msg::UI_STEP_CHECKING_TREES,
        S::ReadingPackData => msg::UI_STEP_READING_PACK_DATA,
        S::FindingUsedData => msg::UI_STEP_FINDING_USED_DATA,
        S::MarkingUnusedPacks => msg::UI_STEP_MARKING_UNUSED_PACKS,
        S::Repacking => msg::UI_STEP_REPACKING,
        S::RebuildingIndex => msg::UI_STEP_REBUILDING_INDEX,
        S::DeletingUnusedData => msg::UI_STEP_DELETING_UNUSED_DATA,
        S::DeletingSnapshots => msg::UI_STEP_DELETING_SNAPSHOTS,
        S::SavingSnapshots => msg::UI_STEP_SAVING_SNAPSHOTS,
        S::Unknown => return None,
    })
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

    /// A run says the step it is in: beside its bar while the bar shows no
    /// percentage, and above it while it shows one, so the percentage is
    /// read as that step's. A step it doesn't know, or none, is working.
    #[test]
    fn a_row_says_its_step_beside_a_bar_with_no_percentage_and_above_one_with_one() {
        use api::OperationStep as S;
        let at = |step, total| {
            progress_text(&Progress {
                step,
                unit: ProgressUnit::Count,
                current: 3,
                total,
            })
        };
        assert_eq!(
            at(Some(S::BackingUp), None),
            ProgressText {
                above: None,
                unknown: msg::UI_STEP_BACKING_UP
            }
        );
        assert_eq!(
            at(Some(S::ReadingIndex), Some(10)),
            ProgressText {
                above: Some(msg::UI_STEP_READING_INDEX),
                unknown: msg::UI_STEP_READING_INDEX
            }
        );
        for step in [None, Some(S::Unknown)] {
            for total in [None, Some(10)] {
                assert_eq!(
                    at(step, total),
                    ProgressText {
                        above: None,
                        unknown: msg::UI_PROGRESS_WORKING
                    },
                    "{step:?} of {total:?}"
                );
            }
        }
    }

    #[test]
    fn each_step_has_a_message_of_its_own() {
        use api::OperationStep as S;
        let messages = [
            (S::ReadingIndex, msg::UI_STEP_READING_INDEX),
            (S::GettingSnapshots, msg::UI_STEP_GETTING_SNAPSHOTS),
            (S::BackingUp, msg::UI_STEP_BACKING_UP),
            (S::ListingPacks, msg::UI_STEP_LISTING_PACKS),
            (S::CheckingCache, msg::UI_STEP_CHECKING_CACHE),
            (
                S::CheckingHotRepository,
                msg::UI_STEP_CHECKING_HOT_REPOSITORY,
            ),
            (S::CheckingTrees, msg::UI_STEP_CHECKING_TREES),
            (S::ReadingPackData, msg::UI_STEP_READING_PACK_DATA),
            (S::FindingUsedData, msg::UI_STEP_FINDING_USED_DATA),
            (S::MarkingUnusedPacks, msg::UI_STEP_MARKING_UNUSED_PACKS),
            (S::Repacking, msg::UI_STEP_REPACKING),
            (S::RebuildingIndex, msg::UI_STEP_REBUILDING_INDEX),
            (S::DeletingUnusedData, msg::UI_STEP_DELETING_UNUSED_DATA),
            (S::DeletingSnapshots, msg::UI_STEP_DELETING_SNAPSHOTS),
            (S::SavingSnapshots, msg::UI_STEP_SAVING_SNAPSHOTS),
        ];
        for (step, message) in messages {
            assert_eq!(step_message(step), Some(message), "{step:?}");
        }
        assert_eq!(step_message(S::Unknown), None);
    }

    #[test]
    fn progress_is_a_whole_percent_of_a_known_total_and_none_without_one() {
        assert_eq!(percent(3, Some(4)), Some(75));
        assert_eq!(percent(1, Some(3)), Some(33));
        assert_eq!(
            percent(0, Some(3)),
            Some(0),
            "nothing of a known total is 0%"
        );
        assert_eq!(percent(5, Some(0)), None, "zero of nothing is no share");
        assert_eq!(percent(9, None), None, "a total not known yet is no share");
    }

    // Both numbers come from a worker's report, outside the process, and
    // nothing keeps the count at or under the total: the share stops at 100%
    // rather than wrapping round to a small number, and no product overflows.
    #[test]
    fn progress_is_never_past_a_hundred_percent_nor_wrapped() {
        assert_eq!(
            percent(3, Some(1)),
            Some(100),
            "three of one is all of it, not 44%"
        );
        assert_eq!(
            percent(101, Some(100)),
            Some(100),
            "a count just past its total is all of it, not 101%"
        );
        assert_eq!(
            percent(u64::MAX, Some(u64::MAX)),
            Some(100),
            "all of the largest total is all of it"
        );
        assert_eq!(percent(1, Some(3)), Some(33));
    }
}
