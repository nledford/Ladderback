use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::{PlanId, Timestamp};

use crate::ports::{PlannedRun, ScheduleStore, StoreError};

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
        todo!()
    }

    async fn plan_run(&self, run: &PlannedRun) -> Result<(), StoreError> {
        let _ = run;
        todo!()
    }

    async fn unplan(&self, plan: &PlanId) -> Result<(), StoreError> {
        let _ = plan;
        todo!()
    }
}
