//! When a plan runs (technical design §5, §6.1): a cron expression, and what
//! an agent does when the moment has passed or isn't convenient.

use jiff::SignedDuration;

use super::InvalidValue;

/// The most bytes a cron expression takes. Every value of every field, listed
/// one by one, fits in 360; the cap bounds what the parser is handed.
pub const MAX_CRON_BYTES: usize = 512;

/// A cron expression in `croner`'s syntax, as it was written: five fields
/// (minute, hour, day of month, month, day of week), or one of the nicknames
/// such as `@daily`. A field for seconds or for a year is refused: a backup
/// has no use for either, and refusing now leaves them to be allowed later
/// without breaking a file that was valid.
///
/// Which instants it matches, and in which time zone, is for the scheduler
/// (§5); this holds only that the expression can be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronExpression(String);

impl CronExpression {
    pub fn parse(_text: &str) -> Result<Self, InvalidValue> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// When a plan runs, and how it behaves when the moment is missed or isn't
/// convenient. Built from its cron expression, with the defaults of §6.1 for
/// the rest: no jitter, a run that was missed is made up for once, and any
/// power source will do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    cron: CronExpression,
    jitter: Option<SignedDuration>,
    catch_up: bool,
    on_ac_power: bool,
}

impl Schedule {
    pub fn new(_cron: CronExpression) -> Self {
        todo!()
    }

    /// Spreads runs over up to `jitter` after the expression's moment, so
    /// that machines on the same expression don't all start at once. Not below
    /// zero and at most [`MAX_DURATION`](super::MAX_DURATION).
    pub fn with_jitter(self, _jitter: SignedDuration) -> Result<Self, InvalidValue> {
        todo!()
    }

    /// Whether a run missed while the agent was off or asleep is made up for
    /// once the agent is back (§5).
    pub fn with_catch_up(self, _catch_up: bool) -> Self {
        todo!()
    }

    /// Whether a run waits for the machine to be on AC power (§5).
    pub fn with_on_ac_power(self, _on_ac_power: bool) -> Self {
        todo!()
    }

    pub fn cron(&self) -> &CronExpression {
        &self.cron
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
    use crate::planning::{MAX_DURATION, ValueKind};

    fn cron(text: &str) -> CronExpression {
        CronExpression::parse(text).unwrap()
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
        assert_eq!(schedule.cron().as_str(), "0 * * * *");
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
        assert_eq!(schedule.cron().as_str(), "0 3 * * 0");
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
}
