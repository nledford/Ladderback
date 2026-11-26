use uuid::Uuid;

use super::*;

fn op(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

fn at(seconds: u64) -> EpochSeconds {
    EpochSeconds::from_seconds(seconds)
}

fn report(n: u128, happening: Happening, seconds: u64) -> AgentReport {
    AgentReport {
        op_id: op(n),
        happening,
        at: at(seconds),
    }
}

fn failed(code: &str) -> Happening {
    Happening::Failed {
        code: WireName::parse(code).unwrap(),
    }
}

/// A start and its end are one operation; a start alone runs; an end alone
/// has no start. The operation whose report came last is first.
#[test]
fn reports_are_read_as_operations_newest_first() {
    let reports = [
        report(1, Happening::Started, 100),
        report(1, Happening::Succeeded, 160),
        report(2, failed("repository-unreachable"), 200),
        report(3, Happening::Started, 300),
    ];
    assert_eq!(
        latest_operations(&reports, OPERATIONS_SHOWN),
        [
            ReportedOperation {
                op_id: op(3),
                started_at: Some(at(300)),
                ended_at: None,
                outcome: ReportedOutcome::Running,
            },
            ReportedOperation {
                op_id: op(2),
                started_at: None,
                ended_at: Some(at(200)),
                outcome: ReportedOutcome::Failed {
                    code: WireName::parse("repository-unreachable").unwrap(),
                },
            },
            ReportedOperation {
                op_id: op(1),
                started_at: Some(at(100)),
                ended_at: Some(at(160)),
                outcome: ReportedOutcome::Succeeded,
            },
        ]
    );
}

/// A cancel ends its operation as cancelled, and a start heard after it
/// doesn't set it running again.
#[test]
fn a_cancel_ends_its_operation() {
    let reports = [
        report(1, Happening::Started, 100),
        report(1, Happening::Canceled, 160),
        report(1, Happening::Started, 100),
    ];
    assert_eq!(
        latest_operations(&reports, OPERATIONS_SHOWN),
        [ReportedOperation {
            op_id: op(1),
            started_at: Some(at(100)),
            ended_at: Some(at(160)),
            outcome: ReportedOutcome::Canceled,
        }]
    );
}

/// An operation is ordered by its latest report: one that started first and
/// ended last is the newest.
#[test]
fn an_operation_is_as_new_as_its_latest_report() {
    let reports = [
        report(1, Happening::Started, 100),
        report(2, Happening::Started, 110),
        report(2, Happening::Succeeded, 120),
        report(1, Happening::Succeeded, 130),
    ];
    let ids: Vec<_> = latest_operations(&reports, OPERATIONS_SHOWN)
        .into_iter()
        .map(|operation| operation.op_id)
        .collect();
    assert_eq!(ids, [op(1), op(2)]);
}

/// An end settles an operation: its start heard again later, as a resend
/// brings it, keeps its outcome and its place.
#[test]
fn a_start_after_the_end_leaves_the_operation_ended() {
    let reports = [
        report(1, Happening::Started, 100),
        report(1, Happening::Succeeded, 150),
        report(2, Happening::Started, 160),
        report(1, Happening::Started, 100),
    ];
    let operations = latest_operations(&reports, OPERATIONS_SHOWN);
    assert_eq!(
        operations.iter().map(|o| o.op_id).collect::<Vec<_>>(),
        [op(2), op(1)]
    );
    assert_eq!(
        operations.get(1),
        Some(&ReportedOperation {
            op_id: op(1),
            started_at: Some(at(100)),
            ended_at: Some(at(150)),
            outcome: ReportedOutcome::Succeeded,
        })
    );
}

/// At most `limit` operations, the newest; none for no reports.
#[test]
fn only_the_latest_operations_are_kept() {
    let reports: Vec<_> = (1..=4)
        .map(|n| report(n, Happening::Started, 100 + u64::try_from(n).unwrap()))
        .collect();
    let ids: Vec<_> = latest_operations(&reports, 2)
        .into_iter()
        .map(|operation| operation.op_id)
        .collect();
    assert_eq!(ids, [op(4), op(3)]);
    assert_eq!(latest_operations(&reports, 0), []);
    assert_eq!(latest_operations(&[], OPERATIONS_SHOWN), []);
}

/// The latest reports the hub reads hold each operation shown whole, its
/// start and its end, when every operation sent both.
#[test]
fn the_reports_read_hold_each_operation_shown_whole() {
    let reports: Vec<_> = (1..=u128::try_from(2 * OPERATIONS_SHOWN).unwrap())
        .flat_map(|n| {
            let seconds = 100 * u64::try_from(n).unwrap();
            [
                report(n, Happening::Started, seconds),
                report(n, Happening::Succeeded, seconds + 50),
            ]
        })
        .collect();
    let (_, read) = reports.split_at(reports.len().saturating_sub(REPORTS_READ));
    let operations = latest_operations(read, OPERATIONS_SHOWN);
    assert_eq!(operations.len(), OPERATIONS_SHOWN);
    assert!(
        operations
            .iter()
            .all(|operation| operation.started_at.is_some() && operation.ended_at.is_some()),
        "{operations:?}"
    );
}

/// A start that reaches the hub after its end fills the start in, and a
/// second end changes nothing: neither makes the operation newer.
#[test]
fn a_late_start_fills_the_start_in_and_a_second_end_changes_nothing() {
    let reports = [
        report(1, Happening::Succeeded, 150),
        report(2, Happening::Started, 160),
        report(1, Happening::Started, 100),
        report(1, failed("repository-unreachable"), 170),
    ];
    assert_eq!(
        latest_operations(&reports, OPERATIONS_SHOWN),
        [
            ReportedOperation {
                op_id: op(2),
                started_at: Some(at(160)),
                ended_at: None,
                outcome: ReportedOutcome::Running,
            },
            ReportedOperation {
                op_id: op(1),
                started_at: Some(at(100)),
                ended_at: Some(at(150)),
                outcome: ReportedOutcome::Succeeded,
            },
        ]
    );
}
