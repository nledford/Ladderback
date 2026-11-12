use std::sync::Arc;

use domain::execution::conditions::PowerSource;

use crate::ports::PowerSupply;

/// Sets up a power supply in a known state: a fake told what to say, or a
/// real adapter reading a machine the harness made up.
pub trait PowerHarness: Send + Sync {
    /// A laptop on its charger.
    fn on_ac(&self) -> Arc<dyn PowerSupply>;
    /// A laptop on its battery, its charger unplugged.
    fn on_battery(&self) -> Arc<dyn PowerSupply>;
    /// A desktop: no battery at all.
    fn without_battery(&self) -> Arc<dyn PowerSupply>;
    /// A laptop whose battery is full or charging, with no charger the
    /// platform reports: not on battery.
    fn battery_not_discharging(&self) -> Arc<dyn PowerSupply>;
    /// A source the adapter can read but doesn't understand.
    fn unrecognized(&self) -> Arc<dyn PowerSupply>;
    /// A source the adapter can't read.
    fn unreadable(&self) -> Arc<dyn PowerSupply>;
}

pub async fn run(harness: &dyn PowerHarness) {
    a_machine_on_its_charger_is_on_ac_power(harness).await;
    a_machine_on_its_battery_is_on_battery(harness).await;
    a_machine_without_a_battery_is_on_ac_power(harness).await;
    a_battery_that_is_not_running_down_is_not_on_battery(harness).await;
    a_source_that_says_something_unknown_is_an_error(harness).await;
    a_source_that_cannot_be_read_is_an_error(harness).await;
}

async fn a_machine_on_its_charger_is_on_ac_power(harness: &dyn PowerHarness) {
    assert_eq!(harness.on_ac().source().await, Ok(PowerSource::Ac));
}

async fn a_machine_on_its_battery_is_on_battery(harness: &dyn PowerHarness) {
    assert_eq!(
        harness.on_battery().source().await,
        Ok(PowerSource::Battery)
    );
}

async fn a_machine_without_a_battery_is_on_ac_power(harness: &dyn PowerHarness) {
    assert_eq!(
        harness.without_battery().source().await,
        Ok(PowerSource::Ac)
    );
}

async fn a_battery_that_is_not_running_down_is_not_on_battery(harness: &dyn PowerHarness) {
    assert_eq!(
        harness.battery_not_discharging().source().await,
        Ok(PowerSource::Ac)
    );
}

async fn a_source_that_says_something_unknown_is_an_error(harness: &dyn PowerHarness) {
    let answer = harness.unrecognized().source().await;
    assert!(
        matches!(
            answer,
            Err(crate::ports::PlatformError::Unrecognized { .. })
        ),
        "a source this adapter doesn't understand is no answer, not a guess: {answer:?}"
    );
}

async fn a_source_that_cannot_be_read_is_an_error(harness: &dyn PowerHarness) {
    let answer = harness.unreadable().source().await;
    assert!(
        matches!(answer, Err(crate::ports::PlatformError::Unreadable { .. })),
        "a source that can't be read is no answer, not a guess: {answer:?}"
    );
}
