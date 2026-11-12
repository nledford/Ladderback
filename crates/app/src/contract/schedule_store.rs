//! The `ScheduleStore` contract (technical design §5, §9): each plan's
//! planned run, kept between the agent's starts.

use std::sync::Arc;

use async_trait::async_trait;
use domain::execution::retry::RetryScheduled;
use domain::{PlanId, Timestamp};

use crate::ports::{PendingRetry, PlannedRun, ScheduleStore};

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

/// The pending retries' part of the contract.
pub async fn run_retries(harness: &dyn ScheduleHarness) {
    a_new_store_keeps_no_pending_retry(harness).await;
    a_pending_retry_is_kept_for_its_plan_as_it_was_planned(harness).await;
    a_plan_has_one_pending_retry_the_last_one_kept(harness).await;
    pending_retries_come_in_the_order_of_their_plans(harness).await;
    a_dropped_retry_is_no_longer_pending(harness).await;
    retries_and_planned_runs_are_kept_apart(harness).await;
    a_pending_retry_is_kept_to_the_millisecond(harness).await;
    a_pending_retry_outlives_the_daemon(harness).await;
}

fn retry_of(name: &str, attempt: u32, seconds: i64, until: Option<i64>) -> Option<PendingRetry> {
    Some(PendingRetry {
        plan: plan(name)?,
        retry: RetryScheduled {
            attempt,
            at: at(seconds),
            until: until.map(at),
        },
    })
}

async fn keep_retry(store: &dyn ScheduleStore, retry: &PendingRetry) {
    let kept = store.keep_retry(retry).await;
    assert!(kept.is_ok(), "the store keeps the retry: {kept:?}");
}

async fn pending(store: &dyn ScheduleStore) -> Vec<PendingRetry> {
    let pending = store.pending_retries().await;
    assert!(pending.is_ok(), "the store answers: {pending:?}");
    pending.unwrap_or_default()
}

async fn a_new_store_keeps_no_pending_retry(harness: &dyn ScheduleHarness) {
    assert_eq!(pending(harness.empty().await.as_ref()).await, Vec::new());
}

async fn a_pending_retry_is_kept_for_its_plan_as_it_was_planned(harness: &dyn ScheduleHarness) {
    let (Some(home), Some(docs)) = (
        retry_of("demo-home", 3, 600, Some(3_600)),
        retry_of("demo-docs", 100, 60, None),
    ) else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(store.as_ref(), &home).await;
    keep_retry(store.as_ref(), &docs).await;
    assert_eq!(pending(store.as_ref()).await, vec![docs, home]);
}

async fn a_plan_has_one_pending_retry_the_last_one_kept(harness: &dyn ScheduleHarness) {
    let (Some(first), Some(last)) = (
        retry_of("demo-home", 1, 600, Some(3_600)),
        retry_of("demo-home", 2, 1_800, None),
    ) else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(store.as_ref(), &first).await;
    keep_retry(store.as_ref(), &last).await;
    assert_eq!(pending(store.as_ref()).await, vec![last]);
}

async fn pending_retries_come_in_the_order_of_their_plans(harness: &dyn ScheduleHarness) {
    let (Some(photos), Some(docs), Some(home)) = (
        retry_of("demo-photos", 1, 10, None),
        retry_of("demo-docs", 1, 30, None),
        retry_of("demo-home", 1, 20, None),
    ) else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(store.as_ref(), &photos).await;
    keep_retry(store.as_ref(), &docs).await;
    keep_retry(store.as_ref(), &home).await;
    assert_eq!(pending(store.as_ref()).await, vec![docs, home, photos]);
}

async fn a_dropped_retry_is_no_longer_pending(harness: &dyn ScheduleHarness) {
    let (Some(home), Some(docs)) = (
        retry_of("demo-home", 1, 60, None),
        retry_of("demo-docs", 1, 60, None),
    ) else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(store.as_ref(), &home).await;
    keep_retry(store.as_ref(), &docs).await;
    assert_eq!(store.drop_retry(&home.plan).await, Ok(()));
    assert_eq!(pending(store.as_ref()).await, vec![docs.clone()]);
    assert_eq!(
        store.drop_retry(&home.plan).await,
        Ok(()),
        "a plan with no pending retry is no error"
    );
    assert_eq!(pending(store.as_ref()).await, vec![docs]);
}

async fn retries_and_planned_runs_are_kept_apart(harness: &dyn ScheduleHarness) {
    let (Some(retry), Some(run)) = (
        retry_of("demo-home", 1, 60, Some(120)),
        run_of("demo-home", 120),
    ) else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(store.as_ref(), &retry).await;
    keep(store.as_ref(), &run).await;
    assert_eq!(store.unplan(&run.plan).await, Ok(()));
    assert_eq!(pending(store.as_ref()).await, vec![retry.clone()]);
    keep(store.as_ref(), &run).await;
    assert_eq!(store.drop_retry(&retry.plan).await, Ok(()));
    assert_eq!(planned(store.as_ref()).await, vec![run]);
}

async fn a_pending_retry_is_kept_to_the_millisecond(harness: &dyn ScheduleHarness) {
    let Some(home) = plan("demo-home") else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(
        store.as_ref(),
        &PendingRetry {
            plan: home.clone(),
            retry: RetryScheduled {
                attempt: 1,
                at: Timestamp::constant(1_800_000_000, 123_999_999),
                until: Some(Timestamp::constant(1_800_000_060, 456_999_999)),
            },
        },
    )
    .await;
    assert_eq!(
        pending(store.as_ref()).await,
        vec![PendingRetry {
            plan: home,
            retry: RetryScheduled {
                attempt: 1,
                at: Timestamp::constant(1_800_000_000, 123_000_000),
                until: Some(Timestamp::constant(1_800_000_060, 456_000_000)),
            },
        }],
        "cut to the millisecond, never rounded up"
    );
}

async fn a_pending_retry_outlives_the_daemon(harness: &dyn ScheduleHarness) {
    let Some(home) = retry_of("demo-home", 2, 60, Some(600)) else {
        return;
    };
    let store = harness.empty().await;
    keep_retry(store.as_ref(), &home).await;
    drop(store);
    assert_eq!(pending(harness.reopened().await.as_ref()).await, vec![home]);
}
