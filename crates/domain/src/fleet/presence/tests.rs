use super::*;
use crate::Uuid;

fn machine(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

/// `minutes` minutes into the hub's day.
fn at(minutes: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + minutes * 60).unwrap()
}

#[test]
fn a_machine_offline_for_15_minutes_is_due_once_with_since_when() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    let approved = [(machine(1), false)];
    assert_eq!(alerts.due(&approved, at(14)), DueAlerts::default());
    let due = alerts.due(&approved, at(15));
    assert_eq!(due.offline, [(machine(1), at(0))]);
    assert_eq!(due.online, []);
    assert_eq!(alerts.due(&approved, at(60)), DueAlerts::default());
}

#[test]
fn an_offline_machine_the_alerts_dont_know_is_offline_since_the_watch() {
    let mut alerts = OfflineAlerts::default();
    let approved = [(machine(1), false), (machine(2), true)];
    assert_eq!(alerts.due(&approved, at(0)), DueAlerts::default());
    assert_eq!(alerts.due(&approved, at(14)), DueAlerts::default());
    assert_eq!(alerts.due(&approved, at(15)).offline, [(machine(1), at(0))]);
}

#[test]
fn a_machine_already_offline_stays_offline_since_when_it_went() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.went_offline(machine(1), at(10));
    let due = alerts.due(&[(machine(1), false)], at(15));
    assert_eq!(due.offline, [(machine(1), at(0))]);
}

#[test]
fn a_machine_back_after_its_alert_is_due_online_once() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.due(&[(machine(1), false)], at(15));
    alerts.came_online(machine(1), at(20));
    let due = alerts.due(&[(machine(1), true)], at(20));
    assert_eq!(due.offline, []);
    assert_eq!(due.online, [(machine(1), at(20))]);
    assert_eq!(
        alerts.due(&[(machine(1), true)], at(21)),
        DueAlerts::default()
    );
}

#[test]
fn a_machine_back_before_its_alert_is_never_due() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.came_online(machine(1), at(14));
    assert_eq!(
        alerts.due(&[(machine(1), true)], at(30)),
        DueAlerts::default()
    );
}

#[test]
fn a_machine_back_and_gone_again_tells_its_return_and_starts_a_new_wait() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.due(&[(machine(1), false)], at(15));
    alerts.came_online(machine(1), at(20));
    alerts.went_offline(machine(1), at(21));
    let due = alerts.due(&[(machine(1), false)], at(22));
    assert_eq!(due.online, [(machine(1), at(20))]);
    assert_eq!(due.offline, []);
    assert_eq!(
        alerts.due(&[(machine(1), false)], at(36)).offline,
        [(machine(1), at(21))]
    );
}

#[test]
fn a_removed_machine_is_never_due() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.removed(machine(1));
    // Removed before the watch read the approved machines.
    assert_eq!(
        alerts.due(&[(machine(1), false)], at(15)).offline,
        [],
        "a new wait from the watch"
    );
    let mut back = OfflineAlerts::default();
    back.went_offline(machine(1), at(0));
    back.due(&[(machine(1), false)], at(15));
    back.came_online(machine(1), at(20));
    back.removed(machine(1));
    assert_eq!(
        back.due(&[(machine(1), true)], at(20)),
        DueAlerts::default()
    );
}

#[test]
fn a_machine_no_longer_approved_is_forgotten() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.went_offline(machine(2), at(0));
    alerts.due(&[(machine(1), false), (machine(2), false)], at(15));
    alerts.came_online(machine(1), at(16));
    // Neither is approved any more.
    assert_eq!(alerts.due(&[], at(17)), DueAlerts::default());
    alerts.went_offline(machine(3), at(17));
    assert_eq!(alerts.due(&[], at(40)), DueAlerts::default());
}

#[test]
fn an_online_machine_is_never_due_offline() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    assert_eq!(
        alerts.due(&[(machine(1), true)], at(15)),
        DueAlerts::default()
    );
    assert_eq!(
        alerts.due(&[(machine(1), true)], at(30)),
        DueAlerts::default()
    );
}

#[test]
fn an_untold_alert_is_due_again_and_its_return_is_not_told() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.due(&[(machine(1), false)], at(15));
    alerts.untold(machine(1), at(0));
    assert_eq!(
        alerts.due(&[(machine(1), false)], at(16)).offline,
        [(machine(1), at(0))]
    );
    alerts.untold(machine(1), at(0));
    alerts.came_online(machine(1), at(17));
    assert_eq!(
        alerts.due(&[(machine(1), true)], at(17)),
        DueAlerts::default()
    );
}

#[test]
fn an_untold_alert_of_a_machine_that_came_back_meanwhile_tells_no_return() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.due(&[(machine(1), false)], at(15));
    // It came back while the alert was being told, and went again.
    alerts.came_online(machine(1), at(15));
    alerts.went_offline(machine(1), at(16));
    alerts.untold(machine(1), at(0));
    let due = alerts.due(&[(machine(1), false)], at(17));
    assert_eq!(due, DueAlerts::default());
    assert_eq!(
        alerts.due(&[(machine(1), false)], at(31)).offline,
        [(machine(1), at(16))]
    );
}

#[test]
fn an_untold_alert_leaves_a_later_one_alerted() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(1), at(0));
    alerts.due(&[(machine(1), false)], at(15));
    alerts.untold(machine(1), at(5));
    assert_eq!(
        alerts.due(&[(machine(1), false)], at(16)),
        DueAlerts::default()
    );
}

#[test]
fn several_machines_are_due_by_machine_id() {
    let mut alerts = OfflineAlerts::default();
    alerts.went_offline(machine(2), at(0));
    alerts.went_offline(machine(1), at(1));
    let due = alerts.due(&[(machine(1), false), (machine(2), false)], at(16));
    assert_eq!(due.offline, [(machine(1), at(1)), (machine(2), at(0))]);
}
