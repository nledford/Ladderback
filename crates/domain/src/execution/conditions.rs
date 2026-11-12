//! The conditions a plan sets before its scheduled run starts (technical
//! design §5): only on AC power, when the plan's schedule asks for it.

use crate::planning::Schedule;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::CronExpression;

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
