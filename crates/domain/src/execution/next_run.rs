//! When a plan runs next (technical design §5; the domain service `NextRun`
//! of §3.1): the schedule's next moment, spread by jitter, and a catch-up for
//! a run that passed while the agent was off or asleep.
//!
//! The agent plans a run when the one before it starts, keeps the planned
//! run (§9, `schedule_state.next_run`), and asks again only when it starts or
//! wakes. Nothing here reads a clock or draws a random number: `now` and the
//! draw are passed in, from the `Clock` and `Random` ports.

use super::Trigger;
use crate::planning::{NoNextRun, Schedule};
use crate::{TimeZone, Timestamp};

/// A number the `Random` port drew, uniformly over every `u64`. A run's
/// jitter is the same share of the stretch it may be spread over as the
/// draw is of `u64::MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomDraw(u64);

impl RandomDraw {
    pub const LOWEST: Self = Self(0);
    pub const HIGHEST: Self = Self(u64::MAX);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// When a plan runs next, and what starts that run: its schedule, or a
/// catch-up for a run it missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NextRun {
    pub at: Timestamp,
    pub trigger: Trigger,
}

impl NextRun {
    /// The run planned after one that started at `started`; for a plan the
    /// agent schedules for the first time, `started` is now. It is the
    /// schedule's first moment after `started`, plus a jitter drawn by
    /// `draw`, and always strictly after `started`.
    ///
    /// Jitter puts the run up to the schedule's jitter after its moment, and
    /// always before the schedule's following moment, so that it never
    /// takes the place of that moment's run.
    pub fn after(
        schedule: &Schedule,
        zone: &TimeZone,
        started: Timestamp,
        draw: RandomDraw,
    ) -> Result<Self, NoNextRun> {
        let _ = (schedule, zone, started, draw);
        todo!()
    }

    /// What becomes of the `planned` run when the agent starts or wakes at
    /// `now`. A run due before or at `now` was missed: with catch-up it runs
    /// once, now, however many of the schedule's moments passed meanwhile;
    /// without, it is skipped, with the rest, for the first moment after
    /// `now`. A run still to come stays planned, unless no moment after
    /// `now` could have put it that late, as when the clock was set back
    /// after it was planned: then it is planned again from `now`.
    ///
    /// A run that is due when the agent finds it due on time runs as
    /// scheduled; this is only for the moment the agent comes back.
    pub fn resume(
        schedule: &Schedule,
        zone: &TimeZone,
        planned: Timestamp,
        now: Timestamp,
        draw: RandomDraw,
    ) -> Result<Self, NoNextRun> {
        let _ = (schedule, zone, planned, now, draw);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignedDuration;
    use crate::planning::{CronExpression, Interval};

    const MIDDLE: RandomDraw = RandomDraw::new(1 << 63);

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn on(cron: &str) -> Schedule {
        Schedule::new(CronExpression::parse(cron).unwrap())
    }

    fn every(interval: SignedDuration) -> Schedule {
        Schedule::new(Interval::new(interval).unwrap())
    }

    fn jittered(schedule: Schedule, jitter: SignedDuration) -> Schedule {
        schedule.with_jitter(jitter).unwrap()
    }

    fn scheduled(text: &str) -> NextRun {
        NextRun {
            at: at(text),
            trigger: Trigger::Schedule,
        }
    }

    fn catch_up(text: &str) -> NextRun {
        NextRun {
            at: at(text),
            trigger: Trigger::CatchUp,
        }
    }

    #[test]
    fn without_jitter_a_plan_runs_at_its_schedule_s_next_moment() {
        for draw in [RandomDraw::LOWEST, MIDDLE, RandomDraw::HIGHEST] {
            assert_eq!(
                NextRun::after(
                    &on("0 * * * *"),
                    &TimeZone::UTC,
                    at("2026-06-10T10:00:30Z"),
                    draw
                ),
                Ok(scheduled("2026-06-10T11:00Z"))
            );
            // A run that started on its moment is followed by the next one.
            assert_eq!(
                NextRun::after(
                    &on("0 * * * *"),
                    &TimeZone::UTC,
                    at("2026-06-10T10:00Z"),
                    draw
                ),
                Ok(scheduled("2026-06-10T11:00Z"))
            );
        }
    }

    #[test]
    fn a_cron_schedule_runs_on_the_wall_clock_of_its_zone() {
        let berlin = TimeZone::get("Europe/Berlin").unwrap();
        assert_eq!(
            NextRun::after(
                &on("0 3 * * *"),
                &berlin,
                at("2026-06-10T03:00+02:00"),
                MIDDLE
            ),
            Ok(scheduled("2026-06-11T03:00+02:00"))
        );
    }

    #[test]
    fn jitter_spreads_a_run_over_the_stretch_after_its_moment() {
        let schedule = jittered(on("0 3 * * *"), SignedDuration::from_mins(10));
        let last = at("2026-06-10T03:04Z");
        for (draw, then) in [
            (RandomDraw::LOWEST, "2026-06-11T03:00Z"),
            (RandomDraw::new(1 << 62), "2026-06-11T03:02:30Z"),
            (MIDDLE, "2026-06-11T03:05Z"),
            (RandomDraw::HIGHEST, "2026-06-11T03:10Z"),
        ] {
            assert_eq!(
                NextRun::after(&schedule, &TimeZone::UTC, last, draw),
                Ok(scheduled(then)),
                "{draw:?}"
            );
        }
    }

    #[test]
    fn jitter_never_reaches_the_schedule_s_following_moment() {
        let two_hours = SignedDuration::from_hours(2);
        assert_eq!(
            NextRun::after(
                &jittered(on("0 * * * *"), two_hours),
                &TimeZone::UTC,
                at("2026-06-10T09:00Z"),
                RandomDraw::HIGHEST
            ),
            Ok(scheduled("2026-06-10T10:59:59.999Z"))
        );
        assert_eq!(
            NextRun::after(
                &jittered(every(SignedDuration::from_hours(1)), two_hours),
                &TimeZone::UTC,
                at("2026-06-10T09:00Z"),
                RandomDraw::HIGHEST
            ),
            Ok(scheduled("2026-06-10T10:59:59.999Z"))
        );
    }

    #[test]
    fn a_plan_on_an_interval_runs_that_long_after_its_last_run_started() {
        assert_eq!(
            NextRun::after(
                &every(SignedDuration::from_hours(6)),
                &TimeZone::UTC,
                at("2026-06-10T03:17:20Z"),
                MIDDLE
            ),
            Ok(scheduled("2026-06-10T09:17:20Z"))
        );
    }

    #[test]
    fn a_planned_run_still_to_come_stays_planned() {
        let schedule = jittered(on("0 * * * *"), SignedDuration::from_mins(5));
        for planned in ["2026-06-10T11:03Z", "2026-06-10T10:30:00.000000001Z"] {
            assert_eq!(
                NextRun::resume(
                    &schedule,
                    &TimeZone::UTC,
                    at(planned),
                    at("2026-06-10T10:30Z"),
                    RandomDraw::HIGHEST
                ),
                Ok(scheduled(planned))
            );
        }
    }

    #[test]
    fn a_run_that_passed_while_the_agent_was_away_runs_once_at_once() {
        let schedule = jittered(on("0 * * * *"), SignedDuration::from_mins(5));
        for (planned, now) in [
            ("2026-06-10T10:00Z", "2026-06-10T15:20Z"),
            ("2026-06-10T15:20Z", "2026-06-10T15:20Z"),
        ] {
            assert_eq!(
                NextRun::resume(&schedule, &TimeZone::UTC, at(planned), at(now), MIDDLE),
                Ok(catch_up(now))
            );
        }
        // Once the catch-up starts, the schedule takes over again.
        assert_eq!(
            NextRun::after(
                &on("0 * * * *"),
                &TimeZone::UTC,
                at("2026-06-10T15:20Z"),
                MIDDLE
            ),
            Ok(scheduled("2026-06-10T16:00Z"))
        );
    }

    #[test]
    fn without_catch_up_the_runs_missed_are_skipped_for_the_next_moment() {
        let schedule = on("0 * * * *").with_catch_up(false);
        for planned in ["2026-06-10T10:00Z", "2026-06-10T15:20Z"] {
            assert_eq!(
                NextRun::resume(
                    &schedule,
                    &TimeZone::UTC,
                    at(planned),
                    at("2026-06-10T15:20Z"),
                    MIDDLE
                ),
                Ok(scheduled("2026-06-10T16:00Z"))
            );
        }
    }

    #[test]
    fn an_interval_shorter_than_the_time_away_catches_up_once() {
        let hourly = every(SignedDuration::from_hours(1));
        let planned = NextRun::after(&hourly, &TimeZone::UTC, at("2026-06-10T09:00Z"), MIDDLE);
        assert_eq!(planned, Ok(scheduled("2026-06-10T10:00Z")));
        assert_eq!(
            NextRun::resume(
                &hourly,
                &TimeZone::UTC,
                at("2026-06-10T10:00Z"),
                at("2026-06-10T15:20Z"),
                MIDDLE
            ),
            Ok(catch_up("2026-06-10T15:20Z"))
        );
        assert_eq!(
            NextRun::after(&hourly, &TimeZone::UTC, at("2026-06-10T15:20Z"), MIDDLE),
            Ok(scheduled("2026-06-10T16:20Z"))
        );
        // Without catch-up, the interval counts from now.
        assert_eq!(
            NextRun::resume(
                &hourly.with_catch_up(false),
                &TimeZone::UTC,
                at("2026-06-10T10:00Z"),
                at("2026-06-10T15:20Z"),
                MIDDLE
            ),
            Ok(scheduled("2026-06-10T16:20Z"))
        );
    }

    #[test]
    fn a_run_planned_before_the_clock_went_back_comes_back_to_the_next_moment() {
        let berlin = TimeZone::get("Europe/Berlin").unwrap();
        let now = at("2026-06-10T12:00+02:00");
        assert_eq!(
            NextRun::resume(
                &on("0 3 * * *"),
                &berlin,
                at("2027-06-11T03:00+02:00"),
                now,
                MIDDLE
            ),
            Ok(scheduled("2026-06-11T03:00+02:00"))
        );
        // The latest the next moment's jitter could have put it stays.
        let schedule = jittered(on("0 3 * * *"), SignedDuration::from_mins(10));
        assert_eq!(
            NextRun::resume(
                &schedule,
                &berlin,
                at("2026-06-11T03:10+02:00"),
                now,
                MIDDLE
            ),
            Ok(scheduled("2026-06-11T03:10+02:00"))
        );
        assert_eq!(
            NextRun::resume(
                &schedule,
                &berlin,
                at("2026-06-11T03:10:00.000000001+02:00"),
                now,
                RandomDraw::LOWEST
            ),
            Ok(scheduled("2026-06-11T03:00+02:00"))
        );
        assert_eq!(
            NextRun::resume(
                &every(SignedDuration::from_hours(1)),
                &berlin,
                at("2027-06-11T03:00+02:00"),
                now,
                MIDDLE
            ),
            Ok(scheduled("2026-06-10T13:00+02:00"))
        );
    }

    #[test]
    fn a_schedule_with_no_moment_left_has_no_next_run() {
        let hour = SignedDuration::from_hours(1);
        let late = Timestamp::MAX - hour;
        assert_eq!(
            NextRun::after(&every(hour), &TimeZone::UTC, late, MIDDLE),
            Ok(NextRun {
                at: Timestamp::MAX,
                trigger: Trigger::Schedule
            })
        );
        // Jitter needs the moment after that one, which no instant holds.
        assert_eq!(
            NextRun::after(&jittered(every(hour), hour), &TimeZone::UTC, late, MIDDLE),
            Err(NoNextRun)
        );
        assert_eq!(
            NextRun::after(&every(hour), &TimeZone::UTC, Timestamp::MAX, MIDDLE),
            Err(NoNextRun)
        );
        assert_eq!(
            NextRun::resume(
                &every(hour).with_catch_up(false),
                &TimeZone::UTC,
                late,
                Timestamp::MAX,
                MIDDLE
            ),
            Err(NoNextRun)
        );
    }
}
