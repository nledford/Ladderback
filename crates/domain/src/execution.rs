//! Execution context: one operation's lifecycle (technical design §3.1).

pub mod deadline;
pub mod plan_tags;

use serde::{Deserialize, Serialize};

use crate::{OperationId, SignedDuration, Timestamp};

/// Where an operation is in its lifecycle:
/// `Queued` → `Running` → `Succeeded` / `SucceededWithWarnings` / `Failed` / `Cancelled`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    Queued,
    Running,
    Succeeded,
    SucceededWithWarnings,
    Failed { reason: FailureReason },
    Cancelled,
}

impl OperationStatus {
    /// Whether the operation has ended and can no longer change.
    pub fn is_finished(&self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
}

/// Why an operation failed. Typed, never a sentence: adapters map it onto
/// published codes and the presentation layer renders it (§8.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    /// The engine couldn't be started at all, so the operation never ran.
    /// `detail` is the platform's own message.
    EngineUnavailable { detail: String },
    /// The engine refused the job it was given.
    JobRejected { detail: String },
    /// The engine stopped without reporting a result, for example because its
    /// process crashed or was killed. `exit_code` is `None` when it was ended by a signal.
    EngineExited { exit_code: Option<i32> },
    /// The engine reported that the operation failed. `detail` is the engine's own message.
    EngineFailed { detail: String },
    /// The engine reported events out of order: `transition` is the step the
    /// operation refused.
    ProtocolViolated { transition: Transition },
    /// A backup was still running at its deadline and stopped without
    /// writing a snapshot (I3).
    DeadlinePassed,
}

/// A problem with a single item that doesn't fail the operation, such as a
/// file that couldn't be read during a backup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    /// The affected item, usually a path.
    pub item: Option<String>,
    /// What the engine was doing.
    pub during: String,
    /// The engine's own message, verbatim.
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    SourceUnreadable,
    /// A warning this version doesn't know, for example from a newer worker.
    Other,
}

/// What was attempted when a lifecycle transition was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transition {
    Start,
    ReportProgress,
    RecordWarning,
    Succeed,
    Fail,
    Cancel,
}

/// A refused lifecycle transition: a programming or protocol error, never
/// shown to users as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleError {
    pub transition: Transition,
    pub status: OperationStatus,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for LifecycleError {}

/// One run of one action. Aggregate root of the Execution context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    id: OperationId,
    status: OperationStatus,
    queued_at: Timestamp,
    started_at: Option<Timestamp>,
    finished_at: Option<Timestamp>,
    warnings: Vec<Warning>,
}

impl Operation {
    pub fn queue(id: OperationId, at: Timestamp) -> Self {
        Self {
            id,
            status: OperationStatus::Queued,
            queued_at: at,
            started_at: None,
            finished_at: None,
            warnings: Vec::new(),
        }
    }

    pub fn id(&self) -> OperationId {
        self.id
    }

    pub fn status(&self) -> &OperationStatus {
        &self.status
    }

    pub fn queued_at(&self) -> Timestamp {
        self.queued_at
    }

    pub fn started_at(&self) -> Option<Timestamp> {
        self.started_at
    }

    pub fn finished_at(&self) -> Option<Timestamp> {
        self.finished_at
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Time from start to finish; `None` until the operation has both started and finished.
    pub fn duration(&self) -> Option<SignedDuration> {
        Some(self.finished_at?.duration_since(self.started_at?))
    }

    pub fn start(&mut self, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Start, |s| *s == OperationStatus::Queued)?;
        self.status = OperationStatus::Running;
        self.started_at = Some(at);
        Ok(())
    }

    /// Progress only comes from a running operation.
    pub fn report_progress(&self) -> Result<(), LifecycleError> {
        self.require(Transition::ReportProgress, |s| {
            *s == OperationStatus::Running
        })
    }

    pub fn record_warning(&mut self, warning: Warning) -> Result<(), LifecycleError> {
        self.require(Transition::RecordWarning, |s| {
            *s == OperationStatus::Running
        })?;
        self.warnings.push(warning);
        Ok(())
    }

    /// Ends a running operation successfully: `SucceededWithWarnings` if any
    /// warning was recorded, otherwise `Succeeded`.
    pub fn succeed(&mut self, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Succeed, |s| *s == OperationStatus::Running)?;
        let status = if self.warnings.is_empty() {
            OperationStatus::Succeeded
        } else {
            OperationStatus::SucceededWithWarnings
        };
        self.finish(status, at);
        Ok(())
    }

    /// Ends a queued or running operation as failed. A queued operation can
    /// fail when its engine can't be started at all.
    pub fn fail(&mut self, reason: FailureReason, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Fail, |s| !s.is_finished())?;
        self.finish(OperationStatus::Failed { reason }, at);
        Ok(())
    }

    pub fn cancel(&mut self, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Cancel, |s| !s.is_finished())?;
        self.finish(OperationStatus::Cancelled, at);
        Ok(())
    }

    fn require(
        &self,
        transition: Transition,
        allowed: impl FnOnce(&OperationStatus) -> bool,
    ) -> Result<(), LifecycleError> {
        if allowed(&self.status) {
            Ok(())
        } else {
            Err(LifecycleError {
                transition,
                status: self.status.clone(),
            })
        }
    }

    fn finish(&mut self, status: OperationStatus, at: Timestamp) {
        self.status = status;
        self.finished_at = Some(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn t(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).unwrap()
    }

    fn queued() -> Operation {
        Operation::queue(OperationId::from_uuid(Uuid::from_u128(7)), t(0))
    }

    fn running() -> Operation {
        let mut op = queued();
        op.start(t(10)).unwrap();
        op
    }

    fn unreadable(path: &str) -> Warning {
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some(path.to_owned()),
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        }
    }

    fn failed() -> OperationStatus {
        OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: Some(1) },
        }
    }

    #[test]
    fn only_a_running_operation_reports_progress() {
        assert_eq!(
            queued().report_progress(),
            Err(LifecycleError {
                transition: Transition::ReportProgress,
                status: OperationStatus::Queued,
            })
        );
        assert_eq!(running().report_progress(), Ok(()));
        let mut done = running();
        done.succeed(t(20)).unwrap();
        assert!(done.report_progress().is_err());
    }

    #[test]
    fn a_new_operation_is_queued_with_no_start_or_finish() {
        let op = queued();
        assert_eq!(op.status(), &OperationStatus::Queued);
        assert_eq!(op.queued_at(), t(0));
        assert_eq!(op.started_at(), None);
        assert_eq!(op.finished_at(), None);
        assert_eq!(op.duration(), None);
    }

    #[test]
    fn starting_a_queued_operation_makes_it_running() {
        let op = running();
        assert_eq!(op.status(), &OperationStatus::Running);
        assert_eq!(op.started_at(), Some(t(10)));
    }

    #[test]
    fn an_operation_without_warnings_succeeds() {
        let mut op = running();
        op.succeed(t(70)).unwrap();
        assert_eq!(op.status(), &OperationStatus::Succeeded);
        assert_eq!(op.finished_at(), Some(t(70)));
        assert_eq!(op.duration(), Some(SignedDuration::from_secs(60)));
    }

    #[test]
    fn warnings_turn_success_into_success_with_warnings() {
        let mut op = running();
        op.record_warning(unreadable("/a")).unwrap();
        op.record_warning(unreadable("/b")).unwrap();
        op.succeed(t(20)).unwrap();
        assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);
        assert_eq!(op.warnings(), &[unreadable("/a"), unreadable("/b")]);
    }

    #[test]
    fn a_failed_operation_keeps_its_reason_and_warnings() {
        let mut op = running();
        op.record_warning(unreadable("/a")).unwrap();
        op.fail(FailureReason::EngineExited { exit_code: Some(1) }, t(15))
            .unwrap();
        assert_eq!(op.status(), &failed());
        assert_eq!(op.warnings().len(), 1);
        assert_eq!(op.duration(), Some(SignedDuration::from_secs(5)));
    }

    #[test]
    fn a_queued_operation_can_fail_before_it_starts() {
        let mut op = queued();
        op.fail(
            FailureReason::EngineFailed {
                detail: "worker not found".to_owned(),
            },
            t(1),
        )
        .unwrap();
        assert!(op.status().is_finished());
        assert_eq!(op.finished_at(), Some(t(1)));
        assert_eq!(op.duration(), None);
    }

    #[test]
    fn queued_and_running_operations_can_be_cancelled() {
        for mut op in [queued(), running()] {
            op.cancel(t(30)).unwrap();
            assert_eq!(op.status(), &OperationStatus::Cancelled);
            assert_eq!(op.finished_at(), Some(t(30)));
        }
    }

    #[test]
    fn only_a_queued_operation_can_start() {
        let mut op = running();
        let err = op.start(t(11)).unwrap_err();
        assert_eq!(err.transition, Transition::Start);
        assert_eq!(err.status, OperationStatus::Running);
        assert_eq!(op.started_at(), Some(t(10)), "the first start is kept");
    }

    #[test]
    fn a_queued_operation_can_neither_succeed_nor_record_warnings() {
        let mut op = queued();
        assert_eq!(
            op.succeed(t(1)).unwrap_err().transition,
            Transition::Succeed
        );
        assert_eq!(
            op.record_warning(unreadable("/a")).unwrap_err().transition,
            Transition::RecordWarning
        );
        assert_eq!(op, queued(), "refused transitions change nothing");
    }

    #[test]
    fn a_finished_operation_refuses_every_transition() {
        let mut succeeded = running();
        succeeded.succeed(t(20)).unwrap();
        let mut with_warnings = running();
        with_warnings.record_warning(unreadable("/a")).unwrap();
        with_warnings.succeed(t(20)).unwrap();
        let mut failed = running();
        failed
            .fail(FailureReason::EngineExited { exit_code: None }, t(20))
            .unwrap();
        let mut cancelled = running();
        cancelled.cancel(t(20)).unwrap();

        for finished in [succeeded, with_warnings, failed, cancelled] {
            assert!(finished.status().is_finished());
            let mut op = finished.clone();
            assert!(op.start(t(40)).is_err());
            assert!(op.record_warning(unreadable("/z")).is_err());
            assert!(op.succeed(t(40)).is_err());
            assert!(
                op.fail(FailureReason::EngineExited { exit_code: None }, t(40))
                    .is_err()
            );
            assert!(op.cancel(t(40)).is_err());
            assert_eq!(op, finished, "refused transitions change nothing");
        }
    }

    #[test]
    fn queued_and_running_are_not_finished() {
        assert!(!OperationStatus::Queued.is_finished());
        assert!(!OperationStatus::Running.is_finished());
    }
}
