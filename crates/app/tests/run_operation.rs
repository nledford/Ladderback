//! `RunOperation` when an engine breaks its contract. The well-behaved paths
//! are specified in `features/execution/operation_outcome.feature`.

use std::sync::Arc;

use app::execution::RunOperation;
use app::fakes::FakeClock;
use app::ports::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, ProgressEvent,
};
use async_trait::async_trait;
use domain::execution::{FailureReason, OperationStatus, Transition, Warning, WarningKind};
use domain::{OperationId, Timestamp, Uuid};
use futures::stream::{self, BoxStream, StreamExt};

/// Plays a fixed list of events, whatever the job.
struct Scripted(Vec<EngineEvent>);

#[async_trait]
impl Engine for Scripted {
    async fn run(
        &self,
        _op: OperationId,
        _job: Job,
    ) -> Result<BoxStream<'static, EngineEvent>, EngineError> {
        Ok(stream::iter(self.0.clone()).boxed())
    }
}

async fn run(events: Vec<EngineEvent>) -> domain::execution::Operation {
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let job = Job::Fake(FakeJob {
        steps: 0,
        step_delay: std::time::Duration::ZERO,
        warnings: Vec::new(),
        end: FakeEnd::Succeed,
    });
    RunOperation::new(Arc::new(Scripted(events)), clock)
        .run(OperationId::from_uuid(Uuid::from_u128(1)), job)
        .await
}

fn warning() -> Warning {
    Warning {
        kind: WarningKind::Other,
        item: None,
        during: "backup".to_owned(),
        detail: "odd".to_owned(),
    }
}

#[tokio::test]
async fn a_stream_that_ends_without_a_result_fails_the_operation() {
    let op = run(vec![EngineEvent::Started]).await;
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: None }
        }
    );
}

#[tokio::test]
async fn an_empty_stream_fails_the_queued_operation() {
    let op = run(vec![]).await;
    assert!(matches!(op.status(), OperationStatus::Failed { .. }));
    assert_eq!(op.started_at(), None);
}

#[tokio::test]
async fn an_event_before_started_is_a_protocol_violation() {
    let op = run(vec![EngineEvent::Warning(warning()), EngineEvent::Started]).await;
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::ProtocolViolated {
                transition: Transition::RecordWarning
            }
        }
    );
}

#[tokio::test]
async fn progress_before_started_is_a_protocol_violation() {
    let op = run(vec![
        EngineEvent::Progress(ProgressEvent::Finish { key: 0 }),
        EngineEvent::Started,
    ])
    .await;
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::ProtocolViolated {
                transition: Transition::ReportProgress
            }
        }
    );
}

#[tokio::test]
async fn events_after_the_result_are_ignored() {
    let op = run(vec![
        EngineEvent::Started,
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 }),
        EngineEvent::Warning(warning()),
        EngineEvent::Failed(FailureReason::EngineExited { exit_code: None }),
    ])
    .await;
    assert_eq!(op.status(), &OperationStatus::Succeeded);
    assert!(op.warnings().is_empty());
}
