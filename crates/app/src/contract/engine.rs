use std::assert_matches;
use std::time::Duration;

use domain::execution::{FailureReason, Warning, WarningKind};
use domain::{OperationId, SignedDuration, Uuid};
use futures::StreamExt;

use crate::ports::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, ProgressEvent,
    ProgressKind, RunningJob,
};

/// Every engine plays fake jobs identically. `step_delay` is the real time
/// each step takes; keep it small.
pub async fn run(engine: &dyn Engine, step_delay: Duration) {
    a_job_reports_progress_and_its_result(engine, step_delay).await;
    warnings_are_reported_without_ending_the_job(engine, step_delay).await;
    a_reported_failure_ends_the_job(engine, step_delay).await;
    an_engine_that_stops_without_a_result_fails_with_its_exit_code(engine, step_delay).await;
    a_job_with_no_steps_still_starts_and_finishes(engine).await;
    a_stopped_job_ends_without_a_result(engine, step_delay).await;
    stopping_a_finished_job_does_nothing(engine).await;
    a_job_past_its_deadline_fails_without_a_result(engine, step_delay).await;
    a_job_with_no_time_left_fails_right_after_starting(engine).await;
}

fn op() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(0x5eed))
}

/// The running job, or why the engine refused it. A fake job is always
/// accepted, so a refusal fails the check that asked.
async fn start(engine: &dyn Engine, job: FakeJob) -> Result<RunningJob, EngineError> {
    let started = engine.run(op(), Job::Fake(job)).await;
    assert!(
        started.is_ok(),
        "a fake job is always accepted: {:?}",
        started.as_ref().err()
    );
    started
}

/// All of a job's events after `Started`, or why the engine refused it.
/// `Started` must come first; its timestamp depends on the engine's clock,
/// so it is checked for presence only.
async fn events(engine: &dyn Engine, job: FakeJob) -> Result<Vec<EngineEvent>, EngineError> {
    let events: Vec<EngineEvent> = start(engine, job).await?.events.collect().await;
    assert_matches!(
        events.first(),
        Some(EngineEvent::Started { .. }),
        "a job starts with Started: {events:?}"
    );
    Ok(events.into_iter().skip(1).collect())
}

fn begin() -> EngineEvent {
    EngineEvent::Progress(ProgressEvent::Begin {
        key: 0,
        kind: ProgressKind::Counter,
        step: None,
    })
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
        deadline_budget: None,
    }
}

fn is_result(event: &EngineEvent) -> bool {
    matches!(event, EngineEvent::Succeeded(_))
}

async fn a_job_reports_progress_and_its_result(engine: &dyn Engine, delay: Duration) {
    let expected = vec![
        begin(),
        length(2),
        advance(),
        advance(),
        finish(),
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 2 }),
    ];
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
    let mut expected = vec![begin(), length(1)];
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
    let expected = vec![
        begin(),
        length(1),
        advance(),
        finish(),
        EngineEvent::Failed(FailureReason::EngineFailed {
            detail: detail.clone(),
        }),
    ];
    assert_eq!(
        events(engine, job(1, delay, FakeEnd::Fail { detail })).await,
        Ok(expected)
    );
}

async fn an_engine_that_stops_without_a_result_fails_with_its_exit_code(
    engine: &dyn Engine,
    delay: Duration,
) {
    let expected = vec![
        begin(),
        length(3),
        advance(),
        EngineEvent::Failed(FailureReason::EngineExited { exit_code: Some(3) }),
    ];
    let end = FakeEnd::Exit {
        after_steps: 1,
        exit_code: 3,
    };
    assert_eq!(events(engine, job(3, delay, end)).await, Ok(expected));
}

async fn a_job_with_no_steps_still_starts_and_finishes(engine: &dyn Engine) {
    let expected = vec![
        begin(),
        length(0),
        finish(),
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 }),
    ];
    assert_eq!(
        events(engine, job(0, Duration::ZERO, FakeEnd::Succeed)).await,
        Ok(expected)
    );
}

async fn a_stopped_job_ends_without_a_result(engine: &dyn Engine, delay: Duration) {
    let Ok(RunningJob {
        mut events,
        mut control,
    }) = start(engine, job(10_000, delay, FakeEnd::Succeed)).await
    else {
        return;
    };
    // Wait until it is running and has made progress.
    while let Some(event) = events.next().await {
        if event == advance() {
            break;
        }
    }
    control.stop().await;
    let rest: Vec<EngineEvent> = events.collect().await;
    assert!(
        !rest.iter().any(is_result),
        "a stopped job reported a result: {rest:?}"
    );
}

async fn stopping_a_finished_job_does_nothing(engine: &dyn Engine) {
    let Ok(RunningJob {
        events,
        mut control,
    }) = start(engine, job(0, Duration::ZERO, FakeEnd::Succeed)).await
    else {
        return;
    };
    let events: Vec<EngineEvent> = events.collect().await;
    assert!(events.last().is_some_and(is_result), "{events:?}");
    control.stop().await;
}

async fn a_job_past_its_deadline_fails_without_a_result(engine: &dyn Engine, delay: Duration) {
    let budget = SignedDuration::try_from(delay)
        .ok()
        .and_then(|d| d.checked_mul(3))
        .unwrap_or(SignedDuration::MAX);
    let job = FakeJob {
        deadline_budget: Some(budget),
        ..job(10_000, delay, FakeEnd::Succeed)
    };
    let Ok(events) = events(engine, job).await else {
        return;
    };
    assert_eq!(
        events.last(),
        Some(&EngineEvent::Failed(FailureReason::DeadlinePassed)),
        "{events:?}"
    );
    assert!(!events.iter().any(is_result), "{events:?}");
}

async fn a_job_with_no_time_left_fails_right_after_starting(engine: &dyn Engine) {
    let job = FakeJob {
        deadline_budget: Some(SignedDuration::ZERO),
        ..job(1, Duration::ZERO, FakeEnd::Succeed)
    };
    assert_eq!(
        events(engine, job).await,
        Ok(vec![EngineEvent::Failed(FailureReason::DeadlinePassed)])
    );
}
