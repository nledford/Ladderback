use std::sync::Arc;

use domain::fleet::MachineIdentifier;

use crate::ports::ThisMachine;

/// Sets up this machine on a fresh data folder of its own: the fake told
/// which identifier the system gives, or the real adapter reading it from a
/// file the harness wrote, as `/etc/machine-id` is. Whether the real
/// system's own identifier reads can't be arranged, so the real adapter
/// tests that on its own.
pub trait ThisMachineHarness: Send + Sync {
    /// A machine whose system gives `identifier`, or none that reads when
    /// it is `None`, on an empty data folder.
    fn on(&self, identifier: Option<&str>) -> Arc<dyn ThisMachine>;
}

pub fn run(harness: &dyn ThisMachineHarness) {
    the_system_s_identifier_reads_the_same_each_time(harness);
    an_identifier_that_doesn_t_read_is_an_error(harness);
    a_fresh_data_folder_records_no_identifier(harness);
    a_recorded_identifier_reads_back(harness);
    a_later_record_replaces_the_earlier(harness);
    a_new_machine_id_is_new_each_time_and_records_the_identifier(harness);
}

/// The identifiers the suite records: two literals that parse.
fn here_and_there() -> Option<(MachineIdentifier, MachineIdentifier)> {
    let pair = MachineIdentifier::parse("here")
        .ok()
        .zip(MachineIdentifier::parse("there").ok());
    assert!(pair.is_some(), "both are machine identifiers");
    pair
}

fn the_system_s_identifier_reads_the_same_each_time(harness: &dyn ThisMachineHarness) {
    const SYSTEM: &str = "0123456789abcdef0123456789abcdef";
    let machine = harness.on(Some(SYSTEM));
    let first = machine.identifier();
    assert_eq!(
        first.as_ref().ok(),
        MachineIdentifier::parse(SYSTEM).ok().as_ref(),
        "the identifier reads"
    );
    assert_eq!(machine.identifier(), first, "it reads the same again");
}

fn an_identifier_that_doesn_t_read_is_an_error(harness: &dyn ThisMachineHarness) {
    assert!(harness.on(None).identifier().is_err());
}

fn a_fresh_data_folder_records_no_identifier(harness: &dyn ThisMachineHarness) {
    assert_eq!(harness.on(Some("here")).recorded(), Ok(None));
}

fn a_recorded_identifier_reads_back(harness: &dyn ThisMachineHarness) {
    let Some((here, _)) = here_and_there() else {
        return;
    };
    let machine = harness.on(Some("here"));
    assert_eq!(machine.record(&here), Ok(()));
    assert_eq!(machine.recorded(), Ok(Some(here)));
}

fn a_later_record_replaces_the_earlier(harness: &dyn ThisMachineHarness) {
    let Some((here, there)) = here_and_there() else {
        return;
    };
    let machine = harness.on(Some("here"));
    assert_eq!(machine.record(&there), Ok(()));
    assert_eq!(machine.record(&here), Ok(()));
    assert_eq!(machine.recorded(), Ok(Some(here)));
}

fn a_new_machine_id_is_new_each_time_and_records_the_identifier(harness: &dyn ThisMachineHarness) {
    let Some((here, there)) = here_and_there() else {
        return;
    };
    let machine = harness.on(Some("here"));
    assert_eq!(machine.record(&there), Ok(()));
    let first = machine.renew(&here);
    assert!(first.is_ok(), "a new ID: {first:?}");
    assert_eq!(machine.recorded(), Ok(Some(here.clone())));
    let second = machine.renew(&here);
    assert!(second.is_ok(), "another: {second:?}");
    assert_ne!(first, second, "each new ID is one never given before");
}
