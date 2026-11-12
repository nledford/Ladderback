//! How the scheduler hears that a backup ended, so that it can retry a
//! failed one (technical design §5). Each run of a plan's backup, however it
//! started, says how it ended through a [`RunEndSender`]; the scheduler reads
//! the ends from its [`RunEnds`] as they come, between its sleeps.

use std::sync::{Mutex, PoisonError};

use domain::execution::{Operation, OperationKind, OperationStatus, Trigger};
use domain::{OperationId, PlanId};
use futures::StreamExt;
use futures::channel::mpsc;

/// How a plan's backup ended, as far as its retries care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// It succeeded, with or without warnings: the retries end.
    Succeeded,
    /// It failed: the plan may retry it.
    Failed,
    /// A person cancelled it: the retries end.
    Cancelled,
}

/// A backup of a plan that ended: which plan, what started it, and how it
/// ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunEnd {
    /// The run's operation: a retry's announcement names the run it
    /// retries.
    pub operation: OperationId,
    pub plan: PlanId,
    pub trigger: Trigger,
    pub outcome: RunOutcome,
}

impl RunEnd {
    /// How `operation` ended, when it is a backup of a plan that has ended.
    pub fn of(operation: &Operation) -> Option<Self> {
        if operation.kind() != OperationKind::Backup {
            return None;
        }
        let outcome = match operation.status() {
            OperationStatus::Succeeded | OperationStatus::SucceededWithWarnings => {
                RunOutcome::Succeeded
            }
            OperationStatus::Failed { .. } => RunOutcome::Failed,
            OperationStatus::Cancelled => RunOutcome::Cancelled,
            OperationStatus::Queued | OperationStatus::Running => return None,
        };
        Some(Self {
            operation: operation.id(),
            plan: operation.target().plan.clone()?,
            trigger: operation.trigger(),
            outcome,
        })
    }
}

/// Where a run says how it ended. Cloned into every run; a run that ends
/// after the scheduler stopped says it to nobody.
#[derive(Clone)]
pub struct RunEndSender(mpsc::UnboundedSender<RunEnd>);

impl RunEndSender {
    pub fn send(&self, end: RunEnd) {
        // Gone only once the scheduler is: nobody is left to retry it.
        let _ = self.0.unbounded_send(end);
    }
}

/// The ends the runs said, in the order they said them, which the scheduler
/// reads. One per run of a backup, so they are as many as the runs.
pub struct RunEnds(Mutex<mpsc::UnboundedReceiver<RunEnd>>);

impl RunEnds {
    /// An end said and not read yet, if there is one.
    pub fn try_next(&self) -> Option<RunEnd> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_recv()
            .ok()
    }

    /// Waits for the next end; `None` once every sender is gone.
    pub async fn next(&self) -> Option<RunEnd> {
        futures::future::poll_fn(|context| {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .poll_next_unpin(context)
        })
        .await
    }
}

/// A sender the runs say their ends through, and the ends the scheduler
/// reads.
pub fn run_ends() -> (RunEndSender, RunEnds) {
    let (sender, receiver) = mpsc::unbounded();
    (RunEndSender(sender), RunEnds(Mutex::new(receiver)))
}

#[cfg(test)]
mod tests {
    use domain::execution::{FailureReason, Operation, OperationKind, Target, Trigger};
    use domain::{OperationId, PlanId, Timestamp};

    use super::{RunEnd, RunOutcome, run_ends};

    fn plan() -> PlanId {
        PlanId::parse("home").unwrap()
    }

    fn operation(kind: OperationKind) -> Operation {
        Operation::queue(
            OperationId::from_uuid(uuid::Uuid::from_u128(1)),
            kind,
            Trigger::Retry,
            Target {
                plan: Some(plan()),
                repository: None,
            },
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn a_backup_that_ended_says_its_plan_trigger_and_outcome() {
        let mut failed = operation(OperationKind::Backup);
        failed
            .fail(FailureReason::DeadlinePassed, Timestamp::UNIX_EPOCH)
            .unwrap();
        assert_eq!(
            RunEnd::of(&failed),
            Some(RunEnd {
                operation: OperationId::from_uuid(uuid::Uuid::from_u128(1)),
                plan: plan(),
                trigger: Trigger::Retry,
                outcome: RunOutcome::Failed,
            })
        );
    }

    #[test]
    fn a_run_not_ended_or_not_a_backup_says_nothing() {
        assert_eq!(RunEnd::of(&operation(OperationKind::Backup)), None);
        let mut check = operation(OperationKind::Check);
        check
            .fail(FailureReason::DeadlinePassed, Timestamp::UNIX_EPOCH)
            .unwrap();
        assert_eq!(RunEnd::of(&check), None);
    }

    #[test]
    fn the_ends_come_in_the_order_they_were_said() {
        let (sender, ends) = run_ends();
        assert_eq!(ends.try_next(), None);
        let end = |outcome| RunEnd {
            operation: OperationId::from_uuid(uuid::Uuid::from_u128(2)),
            plan: plan(),
            trigger: Trigger::Schedule,
            outcome,
        };
        sender.send(end(RunOutcome::Failed));
        sender.send(end(RunOutcome::Succeeded));
        assert_eq!(ends.try_next(), Some(end(RunOutcome::Failed)));
        assert_eq!(
            futures::executor::block_on(ends.next()),
            Some(end(RunOutcome::Succeeded))
        );
        drop(sender);
        assert_eq!(futures::executor::block_on(ends.next()), None);
    }
}
