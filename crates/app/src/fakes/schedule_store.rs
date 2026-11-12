use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::{PlanId, Timestamp};

use crate::ports::{PendingRetry, PlannedRun, ScheduleStore, StoreError};

/// The planned runs, kept in memory, for use-case tests and acceptance
/// scenarios. It keeps what the real store keeps, to the millisecond.
#[derive(Default)]
pub struct InMemoryScheduleStore {
    kept: Mutex<BTreeMap<PlanId, Timestamp>>,
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
        // Cut to the millisecond, as the real store keeps it.
        let at = Timestamp::from_millisecond(run.at.as_millisecond()).unwrap_or(run.at);
        self.kept().insert(run.plan.clone(), at);
        Ok(())
    }

    async fn unplan(&self, plan: &PlanId) -> Result<(), StoreError> {
        self.kept().remove(plan);
        Ok(())
    }

    async fn pending_retries(&self) -> Result<Vec<PendingRetry>, StoreError> {
        todo!()
    }

    async fn keep_retry(&self, retry: &PendingRetry) -> Result<(), StoreError> {
        let _ = retry;
        todo!()
    }

    async fn drop_retry(&self, plan: &PlanId) -> Result<(), StoreError> {
        let _ = plan;
        todo!()
    }
}
