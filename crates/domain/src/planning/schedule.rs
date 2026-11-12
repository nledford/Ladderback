//! When a plan runs (technical design §5, §6.1): a cron expression or an
//! interval, and what an agent does when the moment has passed or isn't
//! convenient.

use croner::parser::{CronParser, Seconds, Year};
use jiff::SignedDuration;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::duration::within_limits;
use super::{InvalidValue, MAX_DURATION, ValueKind};

/// The most bytes a cron expression takes. Every value of every field, listed
/// one by one, fits in 360; the cap bounds what the parser is handed.
pub const MAX_CRON_BYTES: usize = 512;

/// A cron expression in `croner`'s syntax, as it was written: five fields
/// (minute, hour, day of month, month, day of week), or one of the nicknames
/// such as `@daily`. A field for seconds or for a year is refused: a backup
/// has no use for either, and refusing now leaves them to be allowed later
/// without breaking a file that was valid. So is an expression that never
/// fires, such as `0 0 31 2 *`: a plan that can't run is a mistake, and the
/// scheduler would otherwise search for its next moment in vain.
///
/// Which instants it matches, and in which time zone, is for the scheduler
/// (§5); this holds only that the expression can be read, and fires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronExpression(String);

impl CronExpression {
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let refused = || InvalidValue::new(ValueKind::Cron, text);
        if text.len() > MAX_CRON_BYTES {
            return Err(refused());
        }
        let cron = CronParser::builder()
            .seconds(Seconds::Disallowed)
            .year(Year::Disallowed)
            .build()
            .parse(text)
            .map_err(|_| refused())?;
        // A leap year, so that `0 0 29 2 *` has a next moment; the search
        // gives up at a year of croner's choosing, and that is a refusal.
        let from = jiff::civil::date(2024, 1, 1).at(0, 0, 0, 0);
        cron.find_next_occurrence(&from, true)
            .map(|_| Self(text.to_owned()))
            .map_err(|_| refused())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The expression's first moment strictly after `after`, read on the
    /// wall clock of `zone` (§5). A wall-clock time the zone skips, as when
    /// the clocks go forward, runs at the instant the clock jumps, and every
    /// moment in the skipped stretch runs then, once. A wall-clock time the
    /// zone repeats, as when the clocks go back, runs once, the first time
    /// round.
    pub fn next_after(&self, after: Timestamp, zone: &TimeZone) -> Result<Timestamp, NoNextRun> {
        let _ = (after, zone);
        todo!()
    }
}

/// The shortest interval a plan may run at: 1 minute, a cron expression's
/// finest step. A shorter one is a typo: a backup every few seconds would
/// only queue behind itself.
pub const MIN_INTERVAL: SignedDuration = SignedDuration::from_mins(1);

/// How long after a run starts the next one is due, for a plan that runs
/// "every 6 h" rather than on a cron expression. Real time: a change of the
/// clocks for daylight saving time neither shortens nor lengthens it. At
/// least [`MIN_INTERVAL`] and at most [`MAX_DURATION`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval(SignedDuration);

impl Interval {
    pub fn new(interval: SignedDuration) -> Result<Self, InvalidValue> {
        let _ = (interval, MAX_DURATION);
        todo!()
    }

    pub fn get(self) -> SignedDuration {
        self.0
    }
}

/// What a schedule's moments are: those of a cron expression, or one
/// interval after each run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cadence {
    Cron(CronExpression),
    Every(Interval),
}

impl From<CronExpression> for Cadence {
    fn from(cron: CronExpression) -> Self {
        Self::Cron(cron)
    }
}

impl From<Interval> for Cadence {
    fn from(interval: Interval) -> Self {
        Self::Every(interval)
    }
}

impl Cadence {
    /// The first moment strictly after `after`: a cron expression's next
    /// moment on the wall clock of `zone`, or the interval after `after`.
    pub fn moment_after(&self, after: Timestamp, zone: &TimeZone) -> Result<Timestamp, NoNextRun> {
        let _ = (after, zone);
        todo!()
    }
}

/// A schedule has no moment left that an instant can hold: its next one is
/// past the year 5000, where the search for one gives up, or past the last
/// instant `jiff` knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoNextRun;

impl std::fmt::Display for NoNextRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the schedule has no next run")
    }
}

impl std::error::Error for NoNextRun {}

/// When a plan runs, and how it behaves when the moment is missed or isn't
/// convenient. Built from its cron expression or interval, with the defaults
/// of §6.1 for the rest: no jitter, a run that was missed is made up for
/// once, and any power source will do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    cadence: Cadence,
    jitter: Option<SignedDuration>,
    catch_up: bool,
    on_ac_power: bool,
}

impl Schedule {
    pub fn new(cadence: impl Into<Cadence>) -> Self {
        Self {
            cadence: cadence.into(),
            jitter: None,
            catch_up: true,
            on_ac_power: false,
        }
    }

    /// Spreads runs over up to `jitter` after the expression's moment, so
    /// that machines on the same expression don't all start at once. Not below
    /// zero and at most [`MAX_DURATION`](super::MAX_DURATION).
    pub fn with_jitter(self, jitter: SignedDuration) -> Result<Self, InvalidValue> {
        let jitter = within_limits(jitter)?;
        Ok(Self {
            jitter: Some(jitter),
            ..self
        })
    }

    /// Whether a run missed while the agent was off or asleep is made up for
    /// once the agent is back (§5).
    pub fn with_catch_up(self, catch_up: bool) -> Self {
        Self { catch_up, ..self }
    }

    /// Whether a run waits for the machine to be on AC power (§5).
    pub fn with_on_ac_power(self, on_ac_power: bool) -> Self {
        Self {
            on_ac_power,
            ..self
        }
    }

    pub fn cadence(&self) -> &Cadence {
        &self.cadence
    }

    /// The cron expression, for a schedule that has one.
    pub fn cron(&self) -> Option<&CronExpression> {
        match &self.cadence {
            Cadence::Cron(cron) => Some(cron),
            Cadence::Every(_) => None,
        }
    }

    pub fn jitter(&self) -> Option<SignedDuration> {
        self.jitter
    }

    pub fn catch_up(&self) -> bool {
        self.catch_up
    }

    pub fn on_ac_power(&self) -> bool {
        self.on_ac_power
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cron(text: &str) -> CronExpression {
        CronExpression::parse(text).unwrap()
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn zone(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    /// The moment of `expression` after `after`, on the wall clock of `zone`.
    fn next(expression: &str, after: &str, zone_name: &str) -> Timestamp {
        cron(expression)
            .next_after(at(after), &zone(zone_name))
            .unwrap()
    }

    #[test]
    fn a_cron_expression_has_five_fields_or_is_a_nickname() {
        for text in [
            "0 * * * *",
            "*/5 * * * *",
            "0 3 * * 0",
            "30 2 1 1 *",
            "0 9-17 * * MON-FRI",
            "15,45 */6 1-7 jan,jul sun",
            "0 0 L * *",
            // Rare is not never: this one fires once every four years.
            "0 0 29 2 *",
            "@hourly",
            "@daily",
            "@weekly",
            "@monthly",
            "@yearly",
        ] {
            assert_eq!(CronExpression::parse(text).unwrap().as_str(), text);
        }
    }

    #[test]
    fn anything_else_is_not_a_cron_expression() {
        for text in [
            "",
            "   ",
            "soon",
            "* * * *",
            "61 * * * *",
            "0 24 * * *",
            "0 * 32 * *",
            "0 * * 13 *",
            "0 * * * 8",
            "*/0 * * * *",
            "@reboot",
            // A field for seconds, and one for a year, are not allowed.
            "0 0 * * * *",
            "0 0 0 * * * 2030",
            // Well-formed, and never fires: no February has a 30th or 31st,
            // and no April, June, September or November a 31st.
            "0 0 30 2 *",
            "0 0 31 2 *",
            "0 0 31 4 *",
            "0 0 31 6,9,11 *",
        ] {
            assert_eq!(
                CronExpression::parse(text),
                Err(InvalidValue::new(ValueKind::Cron, text)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_cron_expression_is_at_most_512_bytes() {
        let padded = |bytes: usize| format!("{:<bytes$}", "0 * * * *");
        let widest = padded(512);
        let one_over = padded(513);
        assert_eq!(widest.len(), 512);
        assert_eq!(
            CronExpression::parse(&widest).unwrap().as_str(),
            widest.as_str()
        );
        assert_eq!(
            CronExpression::parse(&one_over),
            Err(InvalidValue::new(ValueKind::Cron, one_over.as_str()))
        );
    }

    #[test]
    fn a_schedule_that_says_nothing_else_catches_up_and_runs_on_any_power() {
        let schedule = Schedule::new(cron("0 * * * *"));
        assert_eq!(
            schedule.cron().map(CronExpression::as_str),
            Some("0 * * * *")
        );
        assert_eq!(schedule.jitter(), None);
        assert!(schedule.catch_up());
        assert!(!schedule.on_ac_power());
    }

    #[test]
    fn a_schedule_keeps_what_it_was_told() {
        let schedule = Schedule::new(cron("0 3 * * 0"))
            .with_jitter(SignedDuration::from_mins(5))
            .unwrap()
            .with_catch_up(false)
            .with_on_ac_power(true);
        assert_eq!(
            schedule.cron().map(CronExpression::as_str),
            Some("0 3 * * 0")
        );
        assert_eq!(schedule.jitter(), Some(SignedDuration::from_mins(5)));
        assert!(!schedule.catch_up());
        assert!(schedule.on_ac_power());

        let back_again = schedule.with_catch_up(true).with_on_ac_power(false);
        assert!(back_again.catch_up());
        assert!(!back_again.on_ac_power());
    }

    #[test]
    fn a_jitter_is_zero_or_more_and_at_most_366_days() {
        let second = SignedDuration::from_secs(1);
        for fine in [SignedDuration::ZERO, second, MAX_DURATION] {
            let schedule = Schedule::new(cron("@daily")).with_jitter(fine).unwrap();
            assert_eq!(schedule.jitter(), Some(fine));
        }
        for refused in [-second, MAX_DURATION + second] {
            assert!(matches!(
                Schedule::new(cron("@daily")).with_jitter(refused),
                Err(InvalidValue {
                    kind: ValueKind::Duration,
                    ..
                })
            ));
        }
    }

    #[test]
    fn a_schedule_runs_on_a_cron_expression_or_an_interval() {
        let on_cron = Schedule::new(cron("@daily"));
        assert_eq!(on_cron.cadence(), &Cadence::Cron(cron("@daily")));
        assert_eq!(on_cron.cron(), Some(&cron("@daily")));

        let every = Interval::new(SignedDuration::from_hours(6)).unwrap();
        let on_interval = Schedule::new(every);
        assert_eq!(on_interval.cadence(), &Cadence::Every(every));
        assert_eq!(on_interval.cron(), None);
        assert!(on_interval.catch_up());
    }

    #[test]
    fn an_interval_is_one_minute_to_366_days() {
        for fine in [MIN_INTERVAL, SignedDuration::from_hours(6), MAX_DURATION] {
            assert_eq!(Interval::new(fine).unwrap().get(), fine);
        }
        let nanosecond = SignedDuration::from_nanos(1);
        for refused in [
            SignedDuration::ZERO,
            -MIN_INTERVAL,
            MIN_INTERVAL - nanosecond,
            MAX_DURATION + nanosecond,
        ] {
            assert_eq!(
                Interval::new(refused),
                Err(InvalidValue::new(
                    ValueKind::Interval,
                    super::super::format_duration(refused)
                )),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn an_interval_counts_real_time_across_a_change_of_the_clocks() {
        let every = Cadence::Every(Interval::new(SignedDuration::from_hours(6)).unwrap());
        let berlin = zone("Europe/Berlin");
        // 00:30 in winter time, then the clocks go forward an hour at 02:00.
        assert_eq!(
            every.moment_after(at("2026-03-29T00:30+01:00"), &berlin),
            Ok(at("2026-03-29T07:30+02:00"))
        );
        assert_eq!(
            every.moment_after(at("2026-10-25T00:30+02:00"), &berlin),
            Ok(at("2026-10-25T05:30+01:00"))
        );
        assert_eq!(every.moment_after(Timestamp::MAX, &berlin), Err(NoNextRun));
    }

    #[test]
    fn a_cron_cadence_runs_at_the_expression_s_moments() {
        let daily = Cadence::from(cron("0 3 * * *"));
        assert_eq!(
            daily.moment_after(at("2026-06-10T03:00+02:00"), &zone("Europe/Berlin")),
            Ok(at("2026-06-11T03:00+02:00"))
        );
    }

    #[test]
    fn the_next_moment_is_strictly_after_the_instant_given() {
        let berlin = "Europe/Berlin";
        assert_eq!(
            next("0 3 * * *", "2026-06-10T03:00+02:00", berlin),
            at("2026-06-11T03:00+02:00")
        );
        assert_eq!(
            next("0 3 * * *", "2026-06-10T02:59:59.999999999+02:00", berlin),
            at("2026-06-10T03:00+02:00")
        );
        assert_eq!(
            next("0 3 * * *", "2026-06-10T03:00:00.001+02:00", berlin),
            at("2026-06-11T03:00+02:00")
        );
    }

    #[test]
    fn midnight_and_new_year_roll_over_to_the_next_day() {
        assert_eq!(
            next("0 0 * * *", "2026-06-10T23:59:59Z", "UTC"),
            at("2026-06-11T00:00Z")
        );
        assert_eq!(
            next("0 0 * * *", "2026-06-11T00:00Z", "UTC"),
            at("2026-06-12T00:00Z")
        );
        assert_eq!(
            next("0 0 1 1 *", "2026-12-31T23:59:59Z", "UTC"),
            at("2027-01-01T00:00Z")
        );
        assert_eq!(
            next("59 23 31 12 *", "2026-12-31T23:59Z", "UTC"),
            at("2027-12-31T23:59Z")
        );
    }

    #[test]
    fn a_day_a_month_lacks_waits_for_a_month_that_has_it() {
        assert_eq!(
            next("0 0 31 * *", "2026-04-01T00:00Z", "UTC"),
            at("2026-05-31T00:00Z")
        );
        assert_eq!(
            next("0 0 30 * *", "2026-01-31T00:00Z", "UTC"),
            at("2026-03-30T00:00Z")
        );
        assert_eq!(
            next("0 0 L * *", "2026-02-01T00:00Z", "UTC"),
            at("2026-02-28T00:00Z")
        );
        assert_eq!(
            next("0 0 L * *", "2028-02-01T00:00Z", "UTC"),
            at("2028-02-29T00:00Z")
        );
    }

    #[test]
    fn the_leap_day_comes_once_in_four_years() {
        assert_eq!(
            next("0 0 29 2 *", "2026-03-01T00:00Z", "UTC"),
            at("2028-02-29T00:00Z")
        );
        assert_eq!(
            next("0 0 29 2 *", "2028-02-29T00:00Z", "UTC"),
            at("2032-02-29T00:00Z")
        );
    }

    #[test]
    fn a_time_the_clocks_skip_runs_when_they_jump() {
        // Berlin goes from 02:00 winter time to 03:00 summer time.
        let berlin = "Europe/Berlin";
        assert_eq!(
            next("30 2 * * *", "2026-03-28T02:30+01:00", berlin),
            at("2026-03-29T03:00+02:00")
        );
        assert_eq!(
            next("30 2 * * *", "2026-03-29T03:00+02:00", berlin),
            at("2026-03-30T02:30+02:00")
        );
        // Sydney goes from 02:00 standard time to 03:00 summer time in October.
        assert_eq!(
            next("30 2 * * *", "2026-10-03T02:30+10:00", "Australia/Sydney"),
            at("2026-10-04T03:00+11:00")
        );
    }

    #[test]
    fn the_moments_the_clocks_skip_run_once_when_they_jump() {
        let berlin = "Europe/Berlin";
        assert_eq!(
            next("*/15 * * * *", "2026-03-29T01:45+01:00", berlin),
            at("2026-03-29T03:00+02:00")
        );
        assert_eq!(
            next("*/15 * * * *", "2026-03-29T03:00+02:00", berlin),
            at("2026-03-29T03:15+02:00")
        );
        // 02:00 is skipped and 03:00 is the jump itself: one run for both.
        assert_eq!(
            next("0 2,3 * * *", "2026-03-29T01:00+01:00", berlin),
            at("2026-03-29T03:00+02:00")
        );
        assert_eq!(
            next("0 2,3 * * *", "2026-03-29T03:00+02:00", berlin),
            at("2026-03-30T02:00+02:00")
        );
    }

    #[test]
    fn a_time_the_clocks_repeat_runs_once_the_first_time_round() {
        // Berlin goes from 03:00 summer time back to 02:00 winter time.
        let berlin = "Europe/Berlin";
        assert_eq!(
            next("30 2 * * *", "2026-10-24T02:30+02:00", berlin),
            at("2026-10-25T02:30+02:00")
        );
        assert_eq!(
            next("30 2 * * *", "2026-10-25T02:30+02:00", berlin),
            at("2026-10-26T02:30+01:00")
        );
    }

    #[test]
    fn the_moments_the_clocks_repeat_run_once() {
        let berlin = "Europe/Berlin";
        assert_eq!(
            next("*/30 * * * *", "2026-10-25T02:00+02:00", berlin),
            at("2026-10-25T02:30+02:00")
        );
        assert_eq!(
            next("*/30 * * * *", "2026-10-25T02:30+02:00", berlin),
            at("2026-10-25T03:00+01:00")
        );
        // From the second time round, whose moments ran the first time.
        assert_eq!(
            next("*/30 * * * *", "2026-10-25T02:15+01:00", berlin),
            at("2026-10-25T03:00+01:00")
        );
        assert_eq!(
            next("0 * * * *", "2026-10-25T01:30+02:00", berlin),
            at("2026-10-25T02:00+02:00")
        );
        assert_eq!(
            next("0 * * * *", "2026-10-25T02:00+02:00", berlin),
            at("2026-10-25T03:00+01:00")
        );
    }

    #[test]
    fn a_zone_without_daylight_saving_time_keeps_its_wall_clock() {
        for (after, then) in [
            ("2026-03-28T02:30+09:00", "2026-03-29T02:30+09:00"),
            ("2026-10-24T02:30+09:00", "2026-10-25T02:30+09:00"),
        ] {
            assert_eq!(next("30 2 * * *", after, "Asia/Tokyo"), at(then));
        }
        assert_eq!(
            next("30 2 * * *", "2026-03-28T02:30Z", "UTC"),
            at("2026-03-29T02:30Z")
        );
    }

    #[test]
    fn an_expression_with_no_moment_left_has_no_next_run() {
        let utc = zone("UTC");
        assert_eq!(
            cron("0 0 1 1 *").next_after(at("5000-01-01T00:00Z"), &utc),
            Err(NoNextRun)
        );
        assert_eq!(
            cron("@daily").next_after(Timestamp::MAX, &utc),
            Err(NoNextRun)
        );
    }
}
