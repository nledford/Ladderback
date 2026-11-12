//! Properties of the next run (technical design §5) across changes of the
//! clocks: over many zones and instants near their changes, the next moment
//! of a cron expression is strictly after the instant it is asked after, no
//! moment between them is skipped, no wall-clock moment runs twice, and
//! jitter keeps a run before the schedule's following moment.

use std::collections::BTreeSet;

use croner::Cron;
use croner::parser::CronParser;
use jiff::civil::DateTime;
use proptest::prelude::*;
use proptest::sample::select;

use super::next_run::{NextRun, RandomDraw};
use crate::planning::{CronExpression, Schedule};
use crate::{SignedDuration, TimeZone, Timestamp};

/// Zones with and without daylight saving time: shifts of an hour north and
/// south of the equator, of half an hour (Lord Howe), at midnight
/// (Santiago), at a quarter to the hour (Chatham), and none at all.
const ZONES: [&str; 11] = [
    "Europe/Berlin",
    "Europe/London",
    "America/New_York",
    "America/St_Johns",
    "America/Santiago",
    "Australia/Sydney",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "Asia/Tokyo",
    "Asia/Kolkata",
    "UTC",
];

/// Expressions with a moment at least once a day, so that the walk over the
/// minutes between two moments stays short: each minute or quarter hour, the
/// hours a change of the clocks touches, or once a day.
const EXPRESSIONS: [&str; 10] = [
    "* * * * *",
    "*/15 * * * *",
    "0 * * * *",
    "30 * * * *",
    "0,30 1-3 * * *",
    "30 2 * * *",
    "0 2,3 * * *",
    "45 0 * * *",
    "0 0 * * *",
    "15 1 * * *",
];

/// The longest stretch a zone here repeats when its clocks go back.
const LONGEST_REPEAT: SignedDuration = SignedDuration::from_hours(3);

/// How far after a change of the clocks some instants are drawn, so that many
/// fall the second time round of a repeated hour, which a draw over the
/// whole three days around the change seldom does.
const SECOND_TIME_ROUND: i64 = 3_600;

/// An instant from 2024 on, near one of the zone's changes of the clocks
/// when it has any: up to a day and a half either side of it.
fn instant(zone: &TimeZone, change: usize, seconds: i64) -> Timestamp {
    let start = Timestamp::from_second(1_704_067_200).expect("2024-01-01 is an instant");
    let base = zone
        .following(start)
        .nth(change)
        .map_or(start, |transition| transition.timestamp());
    base + SignedDuration::from_secs(seconds)
}

/// Every whole minute strictly after `after` and before `until`.
fn minutes_between(after: Timestamp, until: Timestamp) -> impl Iterator<Item = Timestamp> {
    let first = after.as_second().div_euclid(60) * 60 + 60;
    (0..)
        .map(move |n| Timestamp::from_second(first + 60 * n).expect("an instant"))
        .take_while(move |minute| *minute < until)
}

fn croner(expression: &str) -> Cron {
    CronParser::builder()
        .build()
        .parse(expression)
        .expect("a cron expression")
}

fn matches(cron: &Cron, civil: DateTime) -> bool {
    cron.is_time_matching(&civil).expect("a time croner reads")
}

proptest! {
    #![proptest_config(crate::properties::config())]

    #[test]
    fn the_next_moment_is_after_the_instant_and_skips_or_repeats_none(
        zone in select(ZONES.to_vec()),
        expression in select(EXPRESSIONS.to_vec()),
        change in 0usize..6,
        seconds in prop_oneof![-129_600i64..=129_600, 0i64..SECOND_TIME_ROUND],
        nanos in 0i64..1_000_000_000,
    ) {
        let tz = TimeZone::get(zone).expect("a zone the build carries");
        let cron = croner(expression);
        let schedule = CronExpression::parse(expression).expect("a cron expression");
        let after = instant(&tz, change, seconds) + SignedDuration::from_nanos(nanos);
        let next = schedule.next_after(after, &tz).expect("a next moment");

        prop_assert!(next > after, "{next} is not after {after}");

        // Walking the real minutes from `after` to `next`, none shows a
        // moment of the expression on the wall clock for the first time.
        let mut seen: BTreeSet<DateTime> = minutes_between(after - LONGEST_REPEAT, after)
            .map(|minute| tz.to_datetime(minute))
            .collect();
        seen.insert(tz.to_datetime(after));
        for minute in minutes_between(after, next) {
            let civil = tz.to_datetime(minute);
            prop_assert!(
                !(seen.insert(civil) && matches(&cron, civil)),
                "{expression} in {zone} skips {civil} ({minute}) between {after} and {next}",
            );
        }

        // Nor does a jump of the clocks before `next` pass over one: a moment
        // they skip runs at the jump itself.
        for transition in tz.following(after).take_while(|t| t.timestamp() < next) {
            let jump = transition.timestamp();
            let from = tz.to_datetime(jump - SignedDuration::from_nanos(1));
            let to = tz.to_datetime(jump);
            let mut skipped = from;
            while let Ok(minute) = skipped.round(jiff::Unit::Minute) {
                if minute >= to {
                    break;
                }
                prop_assert!(
                    minute <= from || !matches(&cron, minute),
                    "{expression} in {zone} skips {minute}, which the clocks jump over at {jump}",
                );
                skipped = minute + SignedDuration::from_mins(1);
            }
        }

        // Nor had `next`'s wall-clock time come round already, as it has when
        // `after` is the second time round of a repeated stretch.
        prop_assert!(
            !seen.contains(&tz.to_datetime(next)),
            "{expression} in {zone} runs {} twice, again at {next}", tz.to_datetime(next),
        );

        // And the moment after it is another wall-clock time.
        let then = schedule.next_after(next, &tz).expect("a next moment");
        prop_assert_ne!(tz.to_datetime(then), tz.to_datetime(next));
    }

    #[test]
    fn a_jittered_run_is_after_the_last_and_before_the_following_moment(
        zone in select(ZONES.to_vec()),
        expression in select(EXPRESSIONS.to_vec()),
        change in 0usize..6,
        seconds in -129_600i64..=129_600,
        jitter_minutes in 0i64..=180,
        draw in any::<u64>(),
    ) {
        let tz = TimeZone::get(zone).expect("a zone the build carries");
        let cron = CronExpression::parse(expression).expect("a cron expression");
        let jitter = SignedDuration::from_mins(jitter_minutes);
        let schedule = Schedule::new(cron.clone()).with_jitter(jitter).expect("a jitter");
        let started = instant(&tz, change, seconds);
        let run = NextRun::after(&schedule, &tz, started, RandomDraw::new(draw))
            .expect("a next run");
        let moment = cron.next_after(started, &tz).expect("a next moment");
        let following = cron.next_after(moment, &tz).expect("a moment after it");
        prop_assert!(run.at >= moment, "{} is before its moment {moment}", run.at);
        prop_assert!(run.at > started);
        prop_assert!(run.at < following, "{} reaches {following}", run.at);
        prop_assert!(run.at.duration_since(moment) <= jitter);
    }
}
