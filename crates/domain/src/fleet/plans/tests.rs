use uuid::Uuid;

use super::*;

fn plan(n: usize) -> ReportedPlan {
    ReportedPlan {
        plan: PlanId::parse(&format!("plan-{n}")).unwrap(),
        last: None,
        next_run: Some(EpochSeconds::from_seconds(1_800_000_000)),
    }
}

fn op(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

fn progress_of(n: u128) -> ReportedProgress {
    ReportedProgress {
        op_id: op(n),
        step: None,
        unit: WireName::known("bytes"),
        current: 5,
        total: Some(10),
        at: EpochSeconds::from_seconds(1_800_000_000),
    }
}

fn operation(n: u128, outcome: ReportedOutcome) -> ReportedOperation {
    ReportedOperation {
        op_id: op(n),
        started_at: Some(EpochSeconds::from_seconds(1_800_000_000)),
        ended_at: None,
        outcome,
    }
}

#[test]
fn a_report_keeps_the_first_plans_up_to_its_cap_and_counts_the_rest() {
    let all: Vec<_> = (0..MAX_PLANS_REPORTED + 4).map(plan).collect();
    let report = PlansReport::of(all.clone());
    assert_eq!(report.plans(), all.get(..MAX_PLANS_REPORTED).unwrap());
    assert_eq!(report.left_out(), 4);

    let few = PlansReport::of(vec![plan(1), plan(2)]);
    assert_eq!(few.plans(), [plan(1), plan(2)]);
    assert_eq!(few.left_out(), 0);
    let full = PlansReport::of((0..MAX_PLANS_REPORTED).map(plan).collect());
    assert_eq!(full.left_out(), 0);
}

#[test]
fn a_snapshot_with_more_plans_than_the_cap_is_refused() {
    let at_cap: Vec<_> = (0..MAX_PLANS_REPORTED).map(plan).collect();
    assert_eq!(
        PlansReport::from_parts(at_cap.clone(), 3),
        Some(PlansReport {
            plans: at_cap,
            left_out: 3
        })
    );
    let over: Vec<_> = (0..=MAX_PLANS_REPORTED).map(plan).collect();
    assert_eq!(PlansReport::from_parts(over, 0), None);
}

/// A report too large for its message gives up its last plan first, so the
/// plans kept are the first in the configuration's order, and the plan
/// left out is counted (#1698).
#[test]
fn leaving_out_the_last_plan_counts_it_with_the_rest() {
    let mut report = PlansReport::of((0..MAX_PLANS_REPORTED + 3).map(plan).collect());
    assert!(report.leave_out_last());
    assert_eq!(
        report.plans(),
        (0..MAX_PLANS_REPORTED - 1).map(plan).collect::<Vec<_>>()
    );
    assert_eq!(report.left_out(), 4);

    let mut none = PlansReport::from_parts(Vec::new(), 2).unwrap();
    assert!(!none.leave_out_last());
    assert_eq!(none, PlansReport::from_parts(Vec::new(), 2).unwrap());

    let mut most = PlansReport::from_parts(vec![plan(1), plan(2)], u64::MAX).unwrap();
    assert!(most.leave_out_last());
    assert_eq!(most.plans(), [plan(1)]);
    assert_eq!(most.left_out(), u64::MAX);
}

#[test]
fn each_outcome_reads_back_from_its_wire_name_and_no_other_name_reads() {
    for outcome in [
        LastRunOutcome::Queued,
        LastRunOutcome::Running,
        LastRunOutcome::Succeeded,
        LastRunOutcome::SucceededWithWarnings,
        LastRunOutcome::Failed,
        LastRunOutcome::Canceled,
    ] {
        assert_eq!(
            LastRunOutcome::from_wire_name(outcome.wire_name()),
            Some(outcome)
        );
    }
    assert_eq!(LastRunOutcome::from_wire_name("exploded"), None);
    assert_eq!(LastRunOutcome::Succeeded.wire_name(), "succeeded");
    assert_eq!(
        LastRunOutcome::SucceededWithWarnings.wire_name(),
        "succeeded-with-warnings"
    );
    // The wire and the hub's store keep the British spelling: changing it
    // would change the sync protocol and need a migration.
    assert_eq!(LastRunOutcome::Canceled.wire_name(), "cancelled");
}

#[test]
fn progress_shows_only_while_its_operation_runs() {
    let running = [
        operation(2, ReportedOutcome::Succeeded),
        operation(1, ReportedOutcome::Running),
    ];
    assert_eq!(
        progress_shown(&running, Some(progress_of(1))),
        Some(progress_of(1))
    );
    // Its operation ended, or the hub heard nothing of it.
    assert_eq!(progress_shown(&running, Some(progress_of(2))), None);
    assert_eq!(progress_shown(&running, Some(progress_of(3))), None);
    assert_eq!(progress_shown(&running, None), None);
    assert_eq!(progress_shown(&[], Some(progress_of(1))), None);
}

/// A canceled operation's end, as its reports tell it, hides its progress
/// as any other end does.
#[test]
fn a_canceled_operations_progress_is_not_shown() {
    use crate::fleet::{AgentReport, Happening, OPERATIONS_SHOWN, latest_operations};

    let report = |happening, seconds| AgentReport {
        op_id: progress_of(1).op_id,
        happening,
        at: EpochSeconds::from_seconds(seconds),
    };
    let reports = [
        report(Happening::Started, 100),
        report(Happening::Canceled, 160),
    ];
    let operations = latest_operations(&reports, OPERATIONS_SHOWN);
    assert_eq!(progress_shown(&operations, Some(progress_of(1))), None);
}

#[test]
fn a_last_run_is_told_at_its_end_else_its_start_else_when_it_was_queued() {
    let at = |second| Timestamp::from_second(second).unwrap();
    let queued = at(100);
    let run = |started, finished| {
        LastRun::of(&OperationStatus::Succeeded, queued, started, finished).map(|last| last.at)
    };
    assert_eq!(
        run(Some(at(200)), Some(at(300))),
        Some(EpochSeconds::from_seconds(300))
    );
    assert_eq!(
        run(Some(at(200)), None),
        Some(EpochSeconds::from_seconds(200))
    );
    assert_eq!(run(None, None), Some(EpochSeconds::from_seconds(100)));
    assert_eq!(
        LastRun::of(&OperationStatus::Queued, at(-1), None, None),
        None
    );
}

#[test]
fn a_last_run_stands_as_its_operation_does() {
    use crate::execution::FailureReason;
    for (status, outcome) in [
        (OperationStatus::Queued, LastRunOutcome::Queued),
        (OperationStatus::Running, LastRunOutcome::Running),
        (OperationStatus::Succeeded, LastRunOutcome::Succeeded),
        (
            OperationStatus::SucceededWithWarnings,
            LastRunOutcome::SucceededWithWarnings,
        ),
        (
            OperationStatus::Failed {
                reason: FailureReason::RepositoryNotFound,
            },
            LastRunOutcome::Failed,
        ),
        (OperationStatus::Canceled, LastRunOutcome::Canceled),
    ] {
        assert_eq!(LastRunOutcome::of(&status), outcome);
    }
}
