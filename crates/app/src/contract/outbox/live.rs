//! The hub's latest plans and progress of each agent (§7.2, #1440): kept per
//! agent, each in place of the one before, across restarts.

use domain::fleet::{
    EpochSeconds, LastRun, LastRunOutcome, PlansReport, ReportedPlan, ReportedProgress, WireName,
};
use domain::{OperationId, PlanId, Uuid};

use super::{AGENT, OTHER_AGENT, ReportInboxHarness, ok};

fn plans(names: &[&str], left_out: u64) -> Option<PlansReport> {
    let plans = names
        .iter()
        .enumerate()
        .map(|(n, name)| {
            Some(ReportedPlan {
                plan: PlanId::parse(name).ok()?,
                // Each field told on some plan and not on another.
                last: (n % 2 == 0).then_some(LastRun {
                    at: EpochSeconds::from_seconds(1_800_000_000 + n as u64),
                    outcome: LastRunOutcome::SucceededWithWarnings,
                }),
                last_success: (n % 3 != 2)
                    .then_some(EpochSeconds::from_seconds(1_799_990_000 + n as u64)),
                next_run: (n % 3 != 1).then_some(EpochSeconds::from_seconds(1_800_003_600)),
            })
        })
        .collect::<Option<_>>()?;
    PlansReport::from_parts(plans, left_out)
}

fn progress(
    op: u128,
    current: u64,
    step: Option<&str>,
    total: Option<u64>,
) -> Option<ReportedProgress> {
    Some(ReportedProgress {
        op_id: OperationId::from_uuid(Uuid::from_u128(op)),
        step: match step {
            Some(step) => Some(WireName::parse(step).ok()?),
            None => None,
        },
        unit: WireName::parse("bytes").ok()?,
        current,
        total,
        at: EpochSeconds::from_seconds(1_800_000_000 + current),
    })
}

/// An agent's plans are kept whole, in their order, each snapshot in place
/// of the one before, apart from another agent's, and outlive the hub.
pub(super) async fn the_latest_plans_are_kept_in_place_of_the_last(
    harness: &dyn ReportInboxHarness,
) {
    assert!(plans_kept(harness).await.is_some(), "the plans read");
}

async fn plans_kept(harness: &dyn ReportInboxHarness) -> Option<()> {
    let inbox = harness.empty().await;
    assert_eq!(ok(inbox.plans(AGENT).await, "no plans yet"), Some(None));
    let first = plans(&["photos", "home", "work"], 2)?;
    ok(inbox.keep_plans(AGENT, &first).await, "keeping plans");
    assert_eq!(ok(inbox.plans(AGENT).await, "the plans"), Some(Some(first)));

    let second = plans(&["home"], 0)?;
    ok(inbox.keep_plans(AGENT, &second).await, "keeping new plans");
    let other = plans(&["other", "more"], 5)?;
    ok(
        inbox.keep_plans(OTHER_AGENT, &other).await,
        "another agent's plans",
    );
    let none = plans(&[], 0)?;
    drop(inbox);
    let inbox = harness.reopened().await;
    assert_eq!(
        ok(inbox.plans(AGENT).await, "the new plans"),
        Some(Some(second))
    );
    assert_eq!(
        ok(inbox.plans(OTHER_AGENT).await, "the other plans"),
        Some(Some(other))
    );
    ok(inbox.keep_plans(AGENT, &none).await, "keeping no plans");
    assert_eq!(ok(inbox.plans(AGENT).await, "no plans"), Some(Some(none)));
    Some(())
}

/// An agent's latest progress is kept whole, in place of the one before of
/// whichever operation, apart from another agent's, and outlives the hub.
pub(super) async fn the_latest_progress_is_kept_in_place_of_the_last(
    harness: &dyn ReportInboxHarness,
) {
    assert!(progress_kept(harness).await.is_some(), "the progress reads");
}

async fn progress_kept(harness: &dyn ReportInboxHarness) -> Option<()> {
    let inbox = harness.empty().await;
    assert_eq!(
        ok(inbox.progress(AGENT).await, "no progress yet"),
        Some(None)
    );
    let first = progress(1, 10, Some("backing-up"), Some(100))?;
    ok(inbox.keep_progress(AGENT, &first).await, "keeping progress");
    assert_eq!(
        ok(inbox.progress(AGENT).await, "the progress"),
        Some(Some(first))
    );
    let second = progress(2, 20, None, None)?;
    ok(
        inbox.keep_progress(AGENT, &second).await,
        "keeping progress",
    );
    let other = progress(3, 30, Some("reading-index"), Some(31))?;
    ok(
        inbox.keep_progress(OTHER_AGENT, &other).await,
        "another's progress",
    );
    drop(inbox);
    let inbox = harness.reopened().await;
    assert_eq!(
        ok(inbox.progress(AGENT).await, "the newer progress"),
        Some(Some(second))
    );
    assert_eq!(
        ok(inbox.progress(OTHER_AGENT).await, "the other progress"),
        Some(Some(other))
    );
    Some(())
}
