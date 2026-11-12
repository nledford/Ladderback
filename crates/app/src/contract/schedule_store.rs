//! The `ScheduleStore` contract (technical design §5, §9): each plan's
//! planned run, kept between the agent's starts.

use std::sync::Arc;

use async_trait::async_trait;
use domain::{PlanId, Timestamp};

use crate::ports::{PlannedRun, ScheduleStore};

/// Opens a schedule store: a new, empty one, or the one before opened again
/// on the same place.
#[async_trait]
pub trait ScheduleHarness: Send + Sync {
    /// A store that keeps nothing yet.
    async fn empty(&self) -> Arc<dyn ScheduleStore>;
    /// The store [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn ScheduleStore>;
}

pub async fn run(harness: &dyn ScheduleHarness) {
    a_new_store_keeps_no_planned_run(harness).await;
    a_planned_run_is_kept_for_its_plan(harness).await;
    a_plan_has_one_planned_run_the_last_one_kept(harness).await;
    planned_runs_come_in_the_order_of_their_plans(harness).await;
    an_unplanned_plan_has_no_planned_run(harness).await;
    a_plan_with_no_planned_run_can_be_unplanned(harness).await;
    a_planned_run_is_kept_to_the_millisecond(harness).await;
    a_planned_run_outlives_the_daemon(harness).await;
}

fn plan(name: &str) -> Option<PlanId> {
    let parsed = PlanId::parse(name);
    assert!(
        parsed.is_ok(),
        "the suite's own plan IDs are valid: {parsed:?}"
    );
    parsed.ok()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

fn run_of(name: &str, seconds: i64) -> Option<PlannedRun> {
    Some(PlannedRun {
        plan: plan(name)?,
        at: at(seconds),
    })
}

async fn keep(store: &dyn ScheduleStore, run: &PlannedRun) {
    let kept = store.plan_run(run).await;
    assert!(kept.is_ok(), "the store keeps the run: {kept:?}");
}

async fn planned(store: &dyn ScheduleStore) -> Vec<PlannedRun> {
    let planned = store.planned_runs().await;
    assert!(planned.is_ok(), "the store answers: {planned:?}");
    planned.unwrap_or_default()
}

async fn a_new_store_keeps_no_planned_run(harness: &dyn ScheduleHarness) {
    assert_eq!(planned(harness.empty().await.as_ref()).await, Vec::new());
}

async fn a_planned_run_is_kept_for_its_plan(harness: &dyn ScheduleHarness) {
    let Some(home) = run_of("demo-home", 60) else {
        return;
    };
    let store = harness.empty().await;
    keep(store.as_ref(), &home).await;
    assert_eq!(planned(store.as_ref()).await, vec![home]);
}

async fn a_plan_has_one_planned_run_the_last_one_kept(harness: &dyn ScheduleHarness) {
    let (Some(first), Some(last)) = (run_of("demo-home", 60), run_of("demo-home", 30)) else {
        return;
    };
    let store = harness.empty().await;
    keep(store.as_ref(), &first).await;
    keep(store.as_ref(), &last).await;
    assert_eq!(planned(store.as_ref()).await, vec![last]);
}

async fn planned_runs_come_in_the_order_of_their_plans(harness: &dyn ScheduleHarness) {
    let (Some(photos), Some(docs), Some(home)) = (
        run_of("demo-photos", 10),
        run_of("demo-docs", 30),
        run_of("demo-home", 20),
    ) else {
        return;
    };
    let store = harness.empty().await;
    keep(store.as_ref(), &photos).await;
    keep(store.as_ref(), &docs).await;
    keep(store.as_ref(), &home).await;
    assert_eq!(planned(store.as_ref()).await, vec![docs, home, photos]);
}

async fn an_unplanned_plan_has_no_planned_run(harness: &dyn ScheduleHarness) {
    let (Some(home), Some(docs)) = (run_of("demo-home", 60), run_of("demo-docs", 60)) else {
        return;
    };
    let store = harness.empty().await;
    keep(store.as_ref(), &home).await;
    keep(store.as_ref(), &docs).await;
    assert_eq!(store.unplan(&home.plan).await, Ok(()));
    assert_eq!(planned(store.as_ref()).await, vec![docs]);
}

async fn a_plan_with_no_planned_run_can_be_unplanned(harness: &dyn ScheduleHarness) {
    let Some(home) = plan("demo-home") else {
        return;
    };
    let store = harness.empty().await;
    assert_eq!(store.unplan(&home).await, Ok(()));
    assert_eq!(planned(store.as_ref()).await, Vec::new());
}

async fn a_planned_run_is_kept_to_the_millisecond(harness: &dyn ScheduleHarness) {
    let Some(home) = plan("demo-home") else {
        return;
    };
    let store = harness.empty().await;
    keep(
        store.as_ref(),
        &PlannedRun {
            plan: home.clone(),
            at: Timestamp::constant(1_800_000_000, 123_999_999),
        },
    )
    .await;
    assert_eq!(
        planned(store.as_ref()).await,
        vec![PlannedRun {
            plan: home,
            at: Timestamp::constant(1_800_000_000, 123_000_000),
        }],
        "cut to the millisecond, never rounded up"
    );
}

async fn a_planned_run_outlives_the_daemon(harness: &dyn ScheduleHarness) {
    let Some(home) = run_of("demo-home", 60) else {
        return;
    };
    let store = harness.empty().await;
    keep(store.as_ref(), &home).await;
    drop(store);
    assert_eq!(planned(harness.reopened().await.as_ref()).await, vec![home]);
}
