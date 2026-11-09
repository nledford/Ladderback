//! How much history a plan keeps (technical design §6.1): rustic's
//! `forget` policy, `KeepOptions`, under the keys rustic's profiles use.
//!
//! Left out on purpose, because Steading never deletes a snapshot on a rule
//! it can't tie to a plan (I6): `keep-tags`, `keep-ids`, `keep-none` and
//! `delete-unchanged`. A policy that sets nothing keeps everything, and no
//! policy keeps nothing; a policy of only zero counts, or a zero-length
//! `keep-within`, is set and keeps nothing (§6.1).

use std::fmt;
use std::str::FromStr;

use jiff::Span;

use super::{InvalidValue, ValueKind};

/// How many of the newest snapshots of one kind to keep: `-1` keeps all of
/// them, and `0` or more keeps that many. What rustic's `keep-*` counts take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepCount(i32);

impl KeepCount {
    /// Reads a count as the file gives it: `-1`, or `0` up to `i32::MAX`.
    pub fn new(count: i64) -> Result<Self, InvalidValue> {
        i32::try_from(count)
            .ok()
            .filter(|count| *count >= -1)
            .map(Self)
            .ok_or_else(|| InvalidValue::new(ValueKind::KeepCount, count.to_string()))
    }

    /// The count as the file writes it: `-1` for all.
    pub fn get(self) -> i32 {
        self.0
    }

    pub fn is_all(self) -> bool {
        self.0 == -1
    }
}

/// How far back from the latest snapshot to keep one kind of snapshot: a
/// span of calendar time, such as `1 day` or `2 years`. It is a `jiff::Span`
/// as rustic reads one, so a month or a year is the calendar's, not a fixed
/// number of days. Never below zero. Two of them are equal when they are
/// written with the same units (`1h` isn't `60m`), which is what a file
/// that is written and read again must give back.
#[derive(Clone, Copy, Debug)]
pub struct KeepWithin(Span);

impl KeepWithin {
    /// Reads a span in `jiff`'s friendly or ISO 8601 form.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        Span::from_str(text)
            .ok()
            .filter(|span| !span.is_negative())
            .map(Self)
            .ok_or_else(|| InvalidValue::new(ValueKind::KeepWithin, text))
    }

    pub fn span(&self) -> Span {
        self.0
    }
}

impl PartialEq for KeepWithin {
    fn eq(&self, other: &Self) -> bool {
        self.0.fieldwise() == other.0.fieldwise()
    }
}

impl Eq for KeepWithin {}

/// The compact form `jiff` reads back, such as `1d`, `1w` or `2y`.
impl fmt::Display for KeepWithin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl FromStr for KeepWithin {
    type Err = InvalidValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

/// Which snapshots of a plan to keep. Each count keeps the newest snapshot of
/// each of that many periods; each `keep_within` keeps every snapshot of its
/// kind newer than the span. Snapshots that no setting keeps are forgotten.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub keep_last: Option<KeepCount>,
    pub keep_minutely: Option<KeepCount>,
    pub keep_hourly: Option<KeepCount>,
    pub keep_daily: Option<KeepCount>,
    pub keep_weekly: Option<KeepCount>,
    pub keep_monthly: Option<KeepCount>,
    pub keep_quarter_yearly: Option<KeepCount>,
    pub keep_half_yearly: Option<KeepCount>,
    pub keep_yearly: Option<KeepCount>,
    pub keep_within: Option<KeepWithin>,
    pub keep_within_minutely: Option<KeepWithin>,
    pub keep_within_hourly: Option<KeepWithin>,
    pub keep_within_daily: Option<KeepWithin>,
    pub keep_within_weekly: Option<KeepWithin>,
    pub keep_within_monthly: Option<KeepWithin>,
    pub keep_within_quarter_yearly: Option<KeepWithin>,
    pub keep_within_half_yearly: Option<KeepWithin>,
    pub keep_within_yearly: Option<KeepWithin>,
}

impl RetentionPolicy {
    /// Whether nothing is set, which keeps every snapshot: a plan with no
    /// policy is never forgotten from.
    pub fn is_empty(&self) -> bool {
        // Every field is named, so that a field added later is a compile
        // error here until it is counted.
        let Self {
            keep_last,
            keep_minutely,
            keep_hourly,
            keep_daily,
            keep_weekly,
            keep_monthly,
            keep_quarter_yearly,
            keep_half_yearly,
            keep_yearly,
            keep_within,
            keep_within_minutely,
            keep_within_hourly,
            keep_within_daily,
            keep_within_weekly,
            keep_within_monthly,
            keep_within_quarter_yearly,
            keep_within_half_yearly,
            keep_within_yearly,
        } = self;
        let counts = [
            keep_last,
            keep_minutely,
            keep_hourly,
            keep_daily,
            keep_weekly,
            keep_monthly,
            keep_quarter_yearly,
            keep_half_yearly,
            keep_yearly,
        ];
        let spans = [
            keep_within,
            keep_within_minutely,
            keep_within_hourly,
            keep_within_daily,
            keep_within_weekly,
            keep_within_monthly,
            keep_within_quarter_yearly,
            keep_within_half_yearly,
            keep_within_yearly,
        ];
        counts.iter().all(|count| count.is_none()) && spans.iter().all(|span| span.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(kind: ValueKind, text: &str) -> InvalidValue {
        InvalidValue::new(kind, text)
    }

    #[test]
    fn a_count_is_minus_one_for_all_or_zero_up_to_i32_max() {
        for count in [-1, 0, 1, 7, 365, i64::from(i32::MAX)] {
            let keep = KeepCount::new(count).unwrap();
            assert_eq!(i64::from(keep.get()), count);
        }
        for count in [-2, -1_000, i64::from(i32::MAX) + 1, i64::MIN, i64::MAX] {
            assert_eq!(
                KeepCount::new(count),
                Err(refused(ValueKind::KeepCount, &count.to_string())),
                "{count}"
            );
        }
    }

    #[test]
    fn only_minus_one_keeps_all() {
        assert!(KeepCount::new(-1).unwrap().is_all());
        for count in [0, 1, i64::from(i32::MAX)] {
            assert!(!KeepCount::new(count).unwrap().is_all(), "{count}");
        }
    }

    #[test]
    fn a_span_is_read_the_way_rustic_reads_one() {
        for text in [
            "1 day", "1 week", "1 months", "1 year", "2 years", "P1Y", "36h", "0s",
        ] {
            let within = KeepWithin::parse(text).unwrap();
            assert!(!within.span().is_negative(), "{text}");
        }
        let year = KeepWithin::parse("1 year").unwrap().span();
        assert_eq!((year.get_years(), year.get_months()), (1, 0));
        let months = KeepWithin::parse("1 months").unwrap().span();
        assert_eq!((months.get_years(), months.get_months()), (0, 1));
    }

    #[test]
    fn a_span_below_zero_or_not_a_span_is_refused() {
        for text in ["", "soon", "5", "-1d", "1d ago", "-P1Y", "1 parsec"] {
            assert_eq!(
                KeepWithin::parse(text),
                Err(refused(ValueKind::KeepWithin, text)),
                "{text}"
            );
        }
    }

    #[test]
    fn spans_are_equal_when_written_with_the_same_units() {
        let parse = |text| KeepWithin::parse(text).unwrap();
        assert_eq!(parse("1 hour"), parse("1h"));
        assert_ne!(parse("1h"), parse("60m"));
        assert_ne!(parse("1d"), parse("2d"));
        assert_ne!(parse("1d"), parse("1w"));
    }

    #[test]
    fn a_span_is_written_compactly_and_reads_back_the_same() {
        for (text, written) in [
            ("1 day", "1d"),
            ("1 week", "1w"),
            ("1 months", "1mo"),
            ("2 years", "2y"),
            ("36 hours", "36h"),
            ("0 seconds", "0s"),
        ] {
            let within = KeepWithin::parse(text).unwrap();
            assert_eq!(within.to_string(), written, "{text}");
            assert_eq!(written.parse::<KeepWithin>(), Ok(within), "{text}");
        }
    }

    #[test]
    fn a_policy_that_sets_nothing_keeps_everything() {
        assert!(RetentionPolicy::default().is_empty());
    }

    #[test]
    fn a_policy_that_sets_anything_is_not_empty() {
        let count = Some(KeepCount::new(3).unwrap());
        let within = Some(KeepWithin::parse("1 week").unwrap());
        type Setter = fn(&mut RetentionPolicy, Option<KeepCount>, Option<KeepWithin>);
        let setters: [(&str, Setter); 18] = [
            ("keep-last", |p, c, _| p.keep_last = c),
            ("keep-minutely", |p, c, _| p.keep_minutely = c),
            ("keep-hourly", |p, c, _| p.keep_hourly = c),
            ("keep-daily", |p, c, _| p.keep_daily = c),
            ("keep-weekly", |p, c, _| p.keep_weekly = c),
            ("keep-monthly", |p, c, _| p.keep_monthly = c),
            ("keep-quarter-yearly", |p, c, _| p.keep_quarter_yearly = c),
            ("keep-half-yearly", |p, c, _| p.keep_half_yearly = c),
            ("keep-yearly", |p, c, _| p.keep_yearly = c),
            ("keep-within", |p, _, w| p.keep_within = w),
            ("keep-within-minutely", |p, _, w| p.keep_within_minutely = w),
            ("keep-within-hourly", |p, _, w| p.keep_within_hourly = w),
            ("keep-within-daily", |p, _, w| p.keep_within_daily = w),
            ("keep-within-weekly", |p, _, w| p.keep_within_weekly = w),
            ("keep-within-monthly", |p, _, w| p.keep_within_monthly = w),
            ("keep-within-quarter-yearly", |p, _, w| {
                p.keep_within_quarter_yearly = w;
            }),
            ("keep-within-half-yearly", |p, _, w| {
                p.keep_within_half_yearly = w;
            }),
            ("keep-within-yearly", |p, _, w| p.keep_within_yearly = w),
        ];
        for (name, set) in setters {
            let mut policy = RetentionPolicy::default();
            set(&mut policy, count, within);
            assert!(!policy.is_empty(), "{name} was ignored");
        }
    }

    #[test]
    fn a_count_of_zero_is_still_a_setting() {
        let policy = RetentionPolicy {
            keep_daily: Some(KeepCount::new(0).unwrap()),
            ..RetentionPolicy::default()
        };
        assert!(!policy.is_empty());
    }
}
