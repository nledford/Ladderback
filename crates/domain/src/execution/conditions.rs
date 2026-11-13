//! The conditions a plan sets before its scheduled run starts (technical
//! design §5): only on AC power, when the plan's schedule asks for it.

use crate::planning::{Link, LocationRule, RuleVerdict, Schedule};

/// What powers the machine, as the platform reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSource {
    /// Mains power: an adapter, a dock, a desktop's supply.
    Ac,
    /// The machine's own battery, or a UPS's, with no AC power.
    Battery,
}

/// A condition of the plan's that the machine doesn't meet now, so its
/// scheduled run doesn't start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnmetCondition {
    /// The plan asks for AC power and the machine is on battery.
    OnBattery,
    /// Every location of the plan's repository has a rule, and each is
    /// known not to hold on the machine's network link (decision 80).
    NoLocationRuleHolds,
}

/// Whether `schedule` lets its run start while the machine is on `power`.
/// `None` is a power source the platform couldn't report: it never holds a
/// run back, since a missed backup costs more than some battery.
pub fn power_condition(
    schedule: &Schedule,
    power: Option<PowerSource>,
) -> Result<(), UnmetCondition> {
    match power {
        Some(PowerSource::Battery) if schedule.on_ac_power() => Err(UnmetCondition::OnBattery),
        _ => Ok(()),
    }
}

/// Whether a scheduled run of a repository whose locations have `rules`, in
/// order, starts on `link`, `None` when the link couldn't be read (decision
/// 80): it is skipped only when every location has a rule and each is known
/// not to hold. A location with no rule is always eligible, and a rule that
/// can't be evaluated never skips a run: the job's own choice fails it where
/// it is recorded, notified and retried.
pub fn location_condition(
    rules: &[Option<LocationRule>],
    link: Option<Link>,
) -> Result<(), UnmetCondition> {
    todo!("{rules:?} {link:?} {:?}", RuleVerdict::NotMet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{CronExpression, NetworkKind};

    fn wired() -> Option<LocationRule> {
        Some(LocationRule::new(vec![NetworkKind::Wired]).unwrap())
    }

    #[test]
    fn a_run_is_skipped_when_every_location_has_a_rule_known_not_to_hold() {
        assert_eq!(
            location_condition(&[wired(), wired()], Some(Link::WiFi)),
            Err(UnmetCondition::NoLocationRuleHolds)
        );
    }

    #[test]
    fn a_run_starts_when_any_rule_holds() {
        let wifi = Some(LocationRule::new(vec![NetworkKind::WiFi]).unwrap());
        assert_eq!(
            location_condition(&[wired(), wifi], Some(Link::WiFi)),
            Ok(())
        );
    }

    #[test]
    fn a_location_with_no_rule_never_lets_the_rules_skip_a_run() {
        assert_eq!(
            location_condition(&[wired(), None], Some(Link::WiFi)),
            Ok(())
        );
        assert_eq!(location_condition(&[None], Some(Link::WiFi)), Ok(()));
    }

    #[test]
    fn a_rule_that_cannot_be_evaluated_never_skips_a_run() {
        for link in [None, Some(Link::NoRoute), Some(Link::NotWiFi)] {
            assert_eq!(
                location_condition(&[wired(), wired()], link),
                Ok(()),
                "{link:?}"
            );
        }
        // One location known not to hold, the other unknown: still a run.
        let wifi = Some(LocationRule::new(vec![NetworkKind::WiFi]).unwrap());
        assert_eq!(
            location_condition(&[wifi, wired()], Some(Link::NotWiFi)),
            Ok(())
        );
    }

    fn schedule(on_ac_power: bool) -> Schedule {
        Schedule::new(CronExpression::parse("0 * * * *").unwrap()).with_on_ac_power(on_ac_power)
    }

    #[test]
    fn a_plan_that_asks_for_ac_power_does_not_run_on_battery() {
        assert_eq!(
            power_condition(&schedule(true), Some(PowerSource::Battery)),
            Err(UnmetCondition::OnBattery)
        );
    }

    #[test]
    fn a_plan_that_asks_for_ac_power_runs_on_ac_power() {
        assert_eq!(
            power_condition(&schedule(true), Some(PowerSource::Ac)),
            Ok(())
        );
    }

    #[test]
    fn a_plan_that_asks_for_ac_power_runs_when_the_source_is_unknown() {
        assert_eq!(power_condition(&schedule(true), None), Ok(()));
    }

    #[test]
    fn a_plan_that_does_not_ask_runs_on_any_power_source() {
        for power in [Some(PowerSource::Battery), Some(PowerSource::Ac), None] {
            assert_eq!(
                power_condition(&schedule(false), power),
                Ok(()),
                "{power:?}"
            );
        }
    }
}
