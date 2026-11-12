//! What a page is told when an operation ends (technical design §4.4).
//!
//! The run that records an operation tells of its end once, after its last
//! write to the log is done, so a page that reads the history on the news
//! finds the end there. The engine's own end is no news: only the run knows
//! when the ending hooks have run and the log has the end.

use api::{ErrorCode, MsgArg, UiEvent};
use domain::execution::{FailureReason, Operation, OperationStatus};

use crate::ports::EventBus;

/// The news that `operation` ended: a success or a failure, with the time it
/// finished. `None` for an operation that has not ended, and for a cancelled
/// one, which no page is told of: the page that cancelled it reads the
/// history until it settles.
pub fn end_of(operation: &Operation) -> Option<UiEvent> {
    let finished_at = operation.finished_at()?.to_string();
    let id = operation.id().as_uuid().hyphenated().to_string();
    match operation.status() {
        OperationStatus::Succeeded | OperationStatus::SucceededWithWarnings => {
            Some(UiEvent::OperationSucceeded {
                operation: id,
                finished_at,
            })
        }
        OperationStatus::Failed { reason } => {
            let (code, args) = failure(reason);
            Some(UiEvent::OperationFailed {
                operation: id,
                code,
                args,
                finished_at,
            })
        }
        OperationStatus::Queued | OperationStatus::Running | OperationStatus::Cancelled => None,
    }
}

/// Tells `events`, when a run has a bus, how `operation` ended, as
/// [`end_of`] has it. A run calls it once, after its last write to the log:
/// whether the log took that write, refused it or was given up on after
/// [`SAVE_BOUND`](crate::log_write::SAVE_BOUND), the news goes out.
pub(crate) async fn announce_end(events: Option<&dyn EventBus>, operation: &Operation) {
    if let Some(events) = events
        && let Some(news) = end_of(operation)
    {
        events.publish(news).await;
    }
}

/// A failure the runner reports, as the code and arguments the page renders.
pub fn failure(reason: &FailureReason) -> (ErrorCode, Vec<MsgArg>) {
    use FailureReason as F;
    fn with_detail(text: &str) -> Vec<MsgArg> {
        vec![MsgArg::Text(text.to_owned())]
    }
    match reason {
        F::EngineUnavailable { detail } | F::JobRejected { detail } => {
            (ErrorCode::EngineUnavailable, with_detail(detail))
        }
        F::EngineFailed { detail } => (ErrorCode::OperationFailed, with_detail(detail)),
        F::HookFailed { .. } => (ErrorCode::OperationFailed, Vec::new()),
        F::EngineExited { exit_code } => (
            ErrorCode::EngineExited,
            exit_code
                .map(|code| MsgArg::Text(code.to_string()))
                .into_iter()
                .collect(),
        ),
        F::ProtocolViolated { transition } => (
            ErrorCode::ProtocolViolated,
            vec![MsgArg::Text(format!("{transition:?}"))],
        ),
        F::DeadlinePassed => (ErrorCode::DeadlinePassed, Vec::new()),
        F::EngineStartTimedOut => (ErrorCode::EngineStartTimedOut, Vec::new()),
        F::EngineExitTimedOut => (ErrorCode::EngineExitTimedOut, Vec::new()),
        F::RepositoryExists => (ErrorCode::RepositoryExists, Vec::new()),
        F::RepositoryReplaced => (ErrorCode::RepositoryReplaced, Vec::new()),
        F::RepositoryNotFound => (ErrorCode::RepositoryNotFound, Vec::new()),
        F::SecretsTooLargeToMask { field, limit } => (
            ErrorCode::SecretsTooLargeToMask,
            std::iter::once(MsgArg::Bytes(u64::try_from(*limit).unwrap_or(u64::MAX)))
                .chain(field.as_deref().map(|field| MsgArg::Text(field.to_owned())))
                .collect(),
        ),
        F::DaemonStopped => (ErrorCode::DaemonStopped, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use domain::execution::{
        FailureReason, Operation, OperationKind, OperationStatus, Target, Transition, Trigger,
        Warning, WarningKind,
    };
    use domain::{OperationId, Timestamp, Uuid};

    use super::*;

    const ID: &str = "00000000-0000-0000-0000-000000000001";

    fn t(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).expect("the demo time is valid")
    }

    fn queued() -> Operation {
        Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(1)),
            OperationKind::Backup,
            Trigger::Manual,
            Target::default(),
            t(0),
        )
    }

    fn running() -> Operation {
        let mut operation = queued();
        operation.start(t(1)).expect("a queued operation starts");
        operation
    }

    fn warning() -> Warning {
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some("/home/demo/secret".to_owned()),
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        }
    }

    #[test]
    fn a_success_is_announced_with_the_time_it_finished() {
        let mut operation = running();
        operation.succeed(t(60)).expect("a running operation ends");
        assert_eq!(
            end_of(&operation),
            Some(UiEvent::OperationSucceeded {
                operation: ID.to_owned(),
                finished_at: t(60).to_string(),
            })
        );
    }

    #[test]
    fn a_success_with_warnings_is_announced_as_a_success() {
        let mut operation = running();
        operation
            .record_warning(warning())
            .expect("a running operation takes a warning");
        operation.succeed(t(60)).expect("a running operation ends");
        assert_eq!(operation.status(), &OperationStatus::SucceededWithWarnings);
        assert_eq!(
            end_of(&operation),
            Some(UiEvent::OperationSucceeded {
                operation: ID.to_owned(),
                finished_at: t(60).to_string(),
            })
        );
    }

    #[test]
    fn a_failure_is_announced_with_its_code_and_the_time_it_finished() {
        let mut operation = running();
        operation
            .fail(
                FailureReason::EngineFailed {
                    detail: "repository not found".to_owned(),
                },
                t(60),
            )
            .expect("a running operation ends");
        assert_eq!(
            end_of(&operation),
            Some(UiEvent::OperationFailed {
                operation: ID.to_owned(),
                code: ErrorCode::OperationFailed,
                args: vec![MsgArg::Text("repository not found".to_owned())],
                finished_at: t(60).to_string(),
            })
        );
    }

    #[test]
    fn an_operation_that_has_not_ended_is_not_announced() {
        assert_eq!(end_of(&queued()), None);
        assert_eq!(end_of(&running()), None);
    }

    #[test]
    fn a_cancelled_operation_is_not_announced() {
        let mut operation = running();
        operation.cancel(t(5)).expect("a running operation cancels");
        assert_eq!(end_of(&operation), None);
    }

    #[test]
    fn every_failure_has_the_code_and_arguments_a_page_renders() {
        use ErrorCode as C;
        use FailureReason as F;
        let text = |text: &str| MsgArg::Text(text.to_owned());
        let cases = [
            (
                F::EngineUnavailable {
                    detail: "no such program".to_owned(),
                },
                C::EngineUnavailable,
                vec![text("no such program")],
            ),
            (
                F::JobRejected {
                    detail: "bad job".to_owned(),
                },
                C::EngineUnavailable,
                vec![text("bad job")],
            ),
            (
                F::EngineFailed {
                    detail: "repository not found".to_owned(),
                },
                C::OperationFailed,
                vec![text("repository not found")],
            ),
            (
                F::HookFailed { exit_code: Some(3) },
                C::OperationFailed,
                vec![],
            ),
            (
                F::EngineExited { exit_code: Some(3) },
                C::EngineExited,
                vec![text("3")],
            ),
            (F::EngineExited { exit_code: None }, C::EngineExited, vec![]),
            (
                F::ProtocolViolated {
                    transition: Transition::Succeed,
                },
                C::ProtocolViolated,
                vec![text("Succeed")],
            ),
            (F::DeadlinePassed, C::DeadlinePassed, vec![]),
            (F::EngineStartTimedOut, C::EngineStartTimedOut, vec![]),
            (F::EngineExitTimedOut, C::EngineExitTimedOut, vec![]),
            (F::RepositoryExists, C::RepositoryExists, vec![]),
            (F::RepositoryReplaced, C::RepositoryReplaced, vec![]),
            (F::RepositoryNotFound, C::RepositoryNotFound, vec![]),
            (F::DaemonStopped, C::DaemonStopped, vec![]),
            (
                F::SecretsTooLargeToMask {
                    field: Some("HTTPS_PROXY".to_owned()),
                    limit: 262_144,
                },
                C::SecretsTooLargeToMask,
                vec![MsgArg::Bytes(262_144), text("HTTPS_PROXY")],
            ),
            (
                F::SecretsTooLargeToMask {
                    field: None,
                    limit: 262_144,
                },
                C::SecretsTooLargeToMask,
                vec![MsgArg::Bytes(262_144)],
            ),
        ];
        for (reason, code, args) in cases {
            assert_eq!(failure(&reason), (code, args), "{reason:?}");
        }
    }
}
