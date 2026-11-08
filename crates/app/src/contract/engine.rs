use std::time::Duration;

use domain::execution::{FailureReason, Warning, WarningKind};
use domain::{OperationId, Uuid};
use futures::StreamExt;

use crate::ports::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, ProgressEvent,
    ProgressKind,
};

/// Every engine plays fake jobs identically. `step_delay` is the real time
/// each step takes; keep it small.
pub async fn run(engine: &dyn Engine, step_delay: Duration) {
    a_job_reports_progress_and_its_result(engine, step_delay).await;
    warnings_are_reported_without_ending_the_job(engine, step_delay).await;
    a_reported_failure_ends_the_job(engine, step_delay).await;
    an_engine_that_stops_without_a_result_fails_with_its_exit_code(engine, step_delay).await;
    a_job_with_no_steps_still_starts_and_finishes(engine).await;
}

fn op() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(0x5eed))
}

/// What the engine plays for `job`, or why it refused it: a fake job is always
/// accepted, so a refusal fails the comparison with its detail.
async fn events(engine: &dyn Engine, job: FakeJob) -> Result<Vec<EngineEvent>, EngineError> {
    Ok(engine.run(op(), Job::Fake(job)).await?.collect().await)
}

fn begin() -> [EngineEvent; 2] {
    [
        EngineEvent::Started,
        EngineEvent::Progress(ProgressEvent::Begin {
            key: 0,
            kind: ProgressKind::Counter,
            prefix: "fake".to_owned(),
        }),
    ]
}

fn length(len: u64) -> EngineEvent {
    EngineEvent::Progress(ProgressEvent::Length { key: 0, len })
}

fn advance() -> EngineEvent {
    EngineEvent::Progress(ProgressEvent::Advance { key: 0, by: 1 })
}

fn finish() -> EngineEvent {
    EngineEvent::Progress(ProgressEvent::Finish { key: 0 })
}

fn job(steps: u32, step_delay: Duration, end: FakeEnd) -> FakeJob {
    FakeJob {
        steps,
        step_delay,
        warnings: Vec::new(),
        end,
    }
}

async fn a_job_reports_progress_and_its_result(engine: &dyn Engine, delay: Duration) {
    let mut expected = begin().to_vec();
    expected.extend([length(2), advance(), advance(), finish()]);
    expected.push(EngineEvent::Succeeded(EngineResult::Fake {
        steps_completed: 2,
    }));
    assert_eq!(
        events(engine, job(2, delay, FakeEnd::Succeed)).await,
        Ok(expected)
    );
}

async fn warnings_are_reported_without_ending_the_job(engine: &dyn Engine, delay: Duration) {
    let warnings = vec![
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some("/home/demo/locked.txt".to_owned()),
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        },
        Warning {
            kind: WarningKind::Other,
            item: None,
            during: "backup".to_owned(),
            detail: "something new".to_owned(),
        },
    ];
    let mut expected = begin().to_vec();
    expected.push(length(1));
    expected.extend(warnings.iter().cloned().map(EngineEvent::Warning));
    expected.extend([
        advance(),
        finish(),
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
    ]);
    let job = FakeJob {
        warnings,
        ..job(1, delay, FakeEnd::Succeed)
    };
    assert_eq!(events(engine, job).await, Ok(expected));
}

async fn a_reported_failure_ends_the_job(engine: &dyn Engine, delay: Duration) {
    let detail = "repository not found".to_owned();
    let mut expected = begin().to_vec();
    expected.extend([
        length(1),
        advance(),
        finish(),
        EngineEvent::Failed(FailureReason::EngineFailed {
            detail: detail.clone(),
        }),
    ]);
    assert_eq!(
        events(engine, job(1, delay, FakeEnd::Fail { detail })).await,
        Ok(expected)
    );
}

async fn an_engine_that_stops_without_a_result_fails_with_its_exit_code(
    engine: &dyn Engine,
    delay: Duration,
) {
    let mut expected = begin().to_vec();
    expected.extend([
        length(3),
        advance(),
        EngineEvent::Failed(FailureReason::EngineExited { exit_code: Some(3) }),
    ]);
    let end = FakeEnd::Exit {
        after_steps: 1,
        exit_code: 3,
    };
    assert_eq!(events(engine, job(3, delay, end)).await, Ok(expected));
}

async fn a_job_with_no_steps_still_starts_and_finishes(engine: &dyn Engine) {
    let mut expected = begin().to_vec();
    expected.extend([
        length(0),
        finish(),
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 }),
    ]);
    assert_eq!(
        events(engine, job(0, Duration::ZERO, FakeEnd::Succeed)).await,
        Ok(expected)
    );
}
