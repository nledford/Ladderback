use api::{OperationStep, ProgressUnit};

use super::*;

const OP: &str = "0190f3a0-0000-7000-8000-000000000001";
const OTHER: &str = "0190f3a0-0000-7000-8000-000000000002";

fn progress(operation: &str, current: u64) -> UiEvent {
    UiEvent::OperationProgress {
        operation: operation.to_owned(),
        step: Some(OperationStep::BackingUp),
        unit: ProgressUnit::Bytes,
        current,
        total: Some(100),
        at: Some("2027-01-15T08:00:05Z".to_owned()),
    }
}

fn succeeded(operation: &str) -> UiEvent {
    UiEvent::OperationSucceeded {
        operation: operation.to_owned(),
        finished_at: "2027-01-15T08:01:00Z".to_owned(),
    }
}

#[test]
fn the_latest_progress_is_kept_with_its_step_and_unit_by_their_wire_names() {
    let live = LiveProgress::new();
    live.hear(&progress(OP, 10));
    live.hear(&progress(OP, 40));
    assert_eq!(
        live.latest(),
        Some(ReportedProgress {
            op_id: operation_id(OP).unwrap(),
            step: Some(WireName::parse("backing-up").unwrap()),
            unit: WireName::parse("bytes").unwrap(),
            current: 40,
            total: Some(100),
            at: EpochSeconds::from_timestamp("2027-01-15T08:00:05Z".parse().unwrap()).unwrap(),
        })
    );
}

#[test]
fn a_progress_is_let_go_of_when_its_operation_ends_and_not_when_another_does() {
    let live = LiveProgress::new();
    live.hear(&progress(OP, 10));
    live.hear(&succeeded(OTHER));
    assert!(live.latest().is_some());
    live.hear(&succeeded(OP));
    assert_eq!(live.latest(), None);
}

#[test]
fn a_progress_is_let_go_of_when_its_operation_is_canceled() {
    let live = LiveProgress::new();
    live.hear(&progress(OP, 10));
    live.hear(&UiEvent::OperationCanceled {
        operation: OP.to_owned(),
        finished_at: "2027-01-15T08:01:00Z".to_owned(),
    });
    assert_eq!(live.latest(), None);
}

#[test]
fn a_progress_without_a_time_or_a_readable_id_changes_nothing() {
    let live = LiveProgress::new();
    live.hear(&progress(OP, 10));
    let mut timeless = progress(OP, 20);
    if let UiEvent::OperationProgress { at, .. } = &mut timeless {
        *at = None;
    }
    live.hear(&timeless);
    live.hear(&progress("not-an-id", 30));
    assert_eq!(live.latest().map(|progress| progress.current), Some(10));
}
