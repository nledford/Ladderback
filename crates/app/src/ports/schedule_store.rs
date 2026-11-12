use async_trait::async_trait;
use domain::execution::retry::RetryScheduled;
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

/// A plan's retry, planned and not started yet, as the agent keeps it while
/// it is off (§5, §9 `retry_state`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRetry {
    pub plan: PlanId,
    /// Its times kept to the millisecond.
    pub retry: RetryScheduled,
}

/// Keeps each scheduled plan's planned run between the agent's starts, so a
/// run that passed while the agent was off is made up for when it comes back
/// (§5, decision 60), and each plan's pending retry, so a retry planned
/// before a restart runs after it (decision 76).
///
/// Contract (checked by `contract::schedule_store`):
/// - [`planned_runs`](Self::planned_runs) returns every planned run kept, one
///   per plan, in the order of the plans' IDs, each as it was last kept, its
///   time cut to the millisecond (never rounded up).
/// - [`plan_run`](Self::plan_run) keeps a run for its plan, replacing the
///   one kept before.
/// - [`unplan`](Self::unplan) forgets a plan's planned run; a plan with none
///   is no error.
/// - [`pending_retries`](Self::pending_retries),
///   [`keep_retry`](Self::keep_retry) and [`drop_retry`](Self::drop_retry)
///   do the same for each plan's pending retry, its attempt as kept and its
///   times cut to the millisecond, apart from the planned runs: forgetting
///   the one leaves the other.
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

    /// Every plan's pending retry, in the order of the plans' IDs.
    async fn pending_retries(&self) -> Result<Vec<PendingRetry>, StoreError>;

    /// Keeps `retry` as its plan's pending retry, in place of any kept
    /// before.
    async fn keep_retry(&self, retry: &PendingRetry) -> Result<(), StoreError>;

    /// Forgets `plan`'s pending retry: it started, or the plan's retries
    /// ended. A plan with none is no error.
    async fn drop_retry(&self, plan: &PlanId) -> Result<(), StoreError>;
}
