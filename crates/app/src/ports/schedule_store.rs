use async_trait::async_trait;
use domain::{PlanId, Timestamp};

use crate::ports::StoreError;

/// A plan's planned run, as the agent keeps it while it is off or asleep
/// (technical design §5, §9 `schedule_state`): when the plan runs next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedRun {
    pub plan: PlanId,
    /// Kept to the millisecond.
    pub at: Timestamp,
}

/// Keeps each scheduled plan's planned run between the agent's starts, so a
/// run that passed while the agent was off is made up for when it comes back
/// (§5, decision 60).
///
/// Contract (checked by `contract::schedule_store`):
/// - [`planned_runs`](Self::planned_runs) returns every planned run kept, one
///   per plan, in the order of the plans' IDs, each as it was last kept, its
///   time cut to the millisecond (never rounded up).
/// - [`plan_run`](Self::plan_run) keeps a run for its plan, replacing the
///   one kept before.
/// - [`unplan`](Self::unplan) forgets a plan's planned run; a plan with none
///   is no error.
/// - What is kept outlives the handle: a store opened again on the same
///   place returns it.
#[async_trait]
pub trait ScheduleStore: Send + Sync {
    /// Every plan's planned run, in the order of the plans' IDs.
    async fn planned_runs(&self) -> Result<Vec<PlannedRun>, StoreError>;

    /// Keeps `run` as its plan's planned run, in place of any kept before.
    async fn plan_run(&self, run: &PlannedRun) -> Result<(), StoreError>;

    /// Forgets `plan`'s planned run: the plan has no schedule any more.
    async fn unplan(&self, plan: &PlanId) -> Result<(), StoreError>;
}
