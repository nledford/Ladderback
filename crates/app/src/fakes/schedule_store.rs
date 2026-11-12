use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::execution::retry::RetryScheduled;
use domain::{PlanId, Timestamp};

use crate::ports::{PendingRetry, PlannedRun, ScheduleStore, StoreError};

/// The planned runs and pending retries, kept in memory, for use-case tests
/// and acceptance scenarios. It keeps what the real store keeps, to the
/// millisecond.
#[derive(Default)]
pub struct InMemoryScheduleStore {
    kept: Mutex<BTreeMap<PlanId, Timestamp>>,
    retries: Mutex<BTreeMap<PlanId, RetryScheduled>>,
}

impl InMemoryScheduleStore {
    pub fn new() -> Self {
        Self::default()
    }

    // Each call changes one entry whole, so a panic elsewhere can't leave
    // one half-written.
    fn kept(&self) -> MutexGuard<'_, BTreeMap<PlanId, Timestamp>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn retries(&self) -> MutexGuard<'_, BTreeMap<PlanId, RetryScheduled>> {
        self.retries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `at` cut to the millisecond, as the real store keeps it.
fn to_the_millisecond(at: Timestamp) -> Timestamp {
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
}

#[async_trait]
impl ScheduleStore for InMemoryScheduleStore {
    async fn planned_runs(&self) -> Result<Vec<PlannedRun>, StoreError> {
        Ok(self
            .kept()
            .iter()
            .map(|(plan, at)| PlannedRun {
                plan: plan.clone(),
                at: *at,
            })
            .collect())
    }

    async fn plan_run(&self, run: &PlannedRun) -> Result<(), StoreError> {
        self.kept()
            .insert(run.plan.clone(), to_the_millisecond(run.at));
        Ok(())
    }

    async fn unplan(&self, plan: &PlanId) -> Result<(), StoreError> {
        self.kept().remove(plan);
        Ok(())
    }

    async fn pending_retries(&self) -> Result<Vec<PendingRetry>, StoreError> {
        Ok(self
            .retries()
            .iter()
            .map(|(plan, retry)| PendingRetry {
                plan: plan.clone(),
                retry: *retry,
            })
            .collect())
    }

    async fn keep_retry(&self, retry: &PendingRetry) -> Result<(), StoreError> {
        let RetryScheduled { attempt, at, until } = retry.retry;
        self.retries().insert(
            retry.plan.clone(),
            RetryScheduled {
                attempt,
                at: to_the_millisecond(at),
                until: until.map(to_the_millisecond),
            },
        );
        Ok(())
    }

    async fn drop_retry(&self, plan: &PlanId) -> Result<(), StoreError> {
        self.retries().remove(plan);
        Ok(())
    }
}
