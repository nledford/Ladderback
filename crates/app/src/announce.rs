//! What a page is told when an operation ends (technical design §4.4).

use api::{ErrorCode, MsgArg, UiEvent};
use domain::execution::{FailureReason, Operation};

/// The news that `operation` ended: a success or a failure, with the time it
/// finished. `None` for an operation that has not ended, and for a cancelled
/// one, which no page is told of: the page that cancelled it reads the
/// history until it settles.
#[allow(clippy::todo, reason = "red test: the news is built next")]
pub fn end_of(_operation: &Operation) -> Option<UiEvent> {
    todo!("the news of an operation's end")
}

/// A failure as the code and arguments a page renders (§8.4).
#[allow(clippy::todo, reason = "red test: the code moves here next")]
pub fn failure(_reason: &FailureReason) -> (ErrorCode, Vec<MsgArg>) {
    todo!("a failure as the code a page renders")
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
            (F::DaemonStopped, C::DaemonStopped, vec![]),
        ];
        for (reason, code, args) in cases {
            assert_eq!(failure(&reason), (code, args), "{reason:?}");
        }
    }
}
