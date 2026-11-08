//! `RunOperation` when an engine breaks its contract, and when an operation
//! is cancelled or passes its deadline. The well-behaved paths are specified
//! in `features/execution/*.feature`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use app::execution::RunOperation;
use app::fakes::FakeClock;
use app::ports::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, JobControl,
    ProgressEvent, RunningJob,
};
use async_trait::async_trait;
use domain::execution::{
    FailureReason, Operation, OperationStatus, Transition, Warning, WarningKind,
};
use domain::{OperationId, SignedDuration, Timestamp, Uuid};
use futures::channel::{mpsc, oneshot};
use futures::future::{self, FutureExt};
use futures::stream::{self, StreamExt};

fn t(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn id() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(1))
}

fn fake_job(deadline_budget: Option<SignedDuration>) -> Job {
    Job::Fake(FakeJob {
        steps: 0,
        step_delay: std::time::Duration::ZERO,
        warnings: Vec::new(),
        end: FakeEnd::Succeed,
        deadline_budget,
    })
}

fn started() -> EngineEvent {
    EngineEvent::Started {
        index_read_at: t(0),
    }
}

/// Counts `stop` calls, and drops without a stop (kills); stopping ends the
/// event stream.
struct Control {
    stops: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
    stopped: bool,
    events: Option<mpsc::UnboundedSender<EngineEvent>>,
}

#[async_trait]
impl JobControl for Control {
    async fn stop(&mut self) {
        self.stopped = true;
        self.stops.fetch_add(1, Ordering::SeqCst);
        if let Some(events) = &self.events {
            events.close_channel();
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if !self.stopped {
            self.kills.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// Plays a fixed list of events, whatever the job, then ends.
struct Scripted {
    events: Vec<EngineEvent>,
    stops: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
}

impl Scripted {
    fn new(events: Vec<EngineEvent>) -> Self {
        Self {
            events,
            stops: Arc::default(),
            kills: Arc::default(),
        }
    }
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Ok(RunningJob {
            events: stream::iter(self.events.clone()).boxed(),
            control: Box::new(Control {
                stops: self.stops.clone(),
                kills: self.kills.clone(),
                stopped: false,
                events: None,
            }),
        })
    }
}

/// Hands out events the test sends, one job at a time; stopping the job
/// closes its stream.
struct Live {
    events: std::sync::Mutex<Option<mpsc::UnboundedReceiver<EngineEvent>>>,
    sender: mpsc::UnboundedSender<EngineEvent>,
    stops: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
}

impl Live {
    fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded();
        Self {
            events: std::sync::Mutex::new(Some(receiver)),
            sender,
            stops: Arc::default(),
            kills: Arc::default(),
        }
    }

    fn send(&self, event: EngineEvent) {
        self.sender.unbounded_send(event).unwrap();
    }

    fn stops(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }

    fn kills(&self) -> usize {
        self.kills.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Engine for Live {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        let events = self
            .events
            .lock()
            .unwrap()
            .take()
            .expect("one job per test");
        Ok(RunningJob {
            events: events.boxed(),
            control: Box::new(Control {
                stops: self.stops.clone(),
                kills: self.kills.clone(),
                stopped: false,
                events: Some(self.sender.clone()),
            }),
        })
    }
}

async fn run(events: Vec<EngineEvent>) -> Operation {
    let clock = Arc::new(FakeClock::new(t(0)));
    RunOperation::new(Arc::new(Scripted::new(events)), clock)
        .run(id(), fake_job(None), future::pending())
        .await
}

/// Lets spawned work run until it waits on something.
async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

/// The spawned run's operation. Fails rather than hangs if the run never
/// ends: a missed cancel or deadline would otherwise wait forever.
async fn finished(running: tokio::task::JoinHandle<Operation>) -> Operation {
    tokio::time::timeout(std::time::Duration::from_secs(5), running)
        .await
        .expect("the operation ended")
        .unwrap()
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
    let op = run(vec![started()]).await;
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
    let op = run(vec![EngineEvent::Warning(warning()), started()]).await;
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
        started(),
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
        started(),
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 }),
        EngineEvent::Warning(warning()),
        EngineEvent::Failed(FailureReason::EngineExited { exit_code: None }),
    ])
    .await;
    assert_eq!(op.status(), &OperationStatus::Succeeded);
    assert!(op.warnings().is_empty());
}

#[tokio::test]
async fn cancelling_a_running_operation_stops_the_engine() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let (cancel, cancelled) = oneshot::channel::<()>();
    let use_case = RunOperation::new(engine.clone(), clock.clone());
    let running = tokio::spawn(async move {
        use_case
            .run(id(), fake_job(None), cancelled.map(|_| ()))
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_secs(5));
    cancel.send(()).unwrap();
    let op = finished(running).await;
    assert_eq!(op.status(), &OperationStatus::Cancelled);
    assert_eq!(op.finished_at(), Some(t(5)));
    assert_eq!(engine.stops(), 1);
}

#[tokio::test]
async fn cancelling_before_the_engine_starts_cancels_the_queued_operation() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock);
    let running =
        tokio::spawn(async move { use_case.run(id(), fake_job(None), future::ready(())).await });
    let op = finished(running).await;
    assert_eq!(op.status(), &OperationStatus::Cancelled);
    assert_eq!(op.started_at(), None);
    assert_eq!(engine.stops(), 1);
}

#[tokio::test]
async fn an_operation_running_at_its_deadline_is_killed_and_fails() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock.clone());
    let budget = Some(SignedDuration::from_secs(60));
    let running = tokio::spawn(async move {
        use_case
            .run(id(), fake_job(budget), future::pending())
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_secs(59));
    settle().await;
    assert_eq!(engine.kills(), 0, "killed before the deadline");
    clock.advance(SignedDuration::from_secs(1));
    let op = finished(running).await;
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
    assert_eq!(op.finished_at(), Some(t(60)));
    // Past the deadline there is no time for a graceful stop (§4.1).
    assert_eq!((engine.stops(), engine.kills()), (0, 1));
}

/// Events that keep arriving can't hold off the deadline: it is checked on
/// every one, not only when the timer gets a turn.
#[tokio::test]
async fn events_arriving_past_the_deadline_fail_it_before_a_later_result() {
    let budget = Some(SignedDuration::from_secs(60));
    let mut events = vec![EngineEvent::Started {
        index_read_at: t(-120),
    }];
    events.extend(std::iter::repeat_n(EngineEvent::Warning(warning()), 100));
    events.push(EngineEvent::Succeeded(EngineResult::Fake {
        steps_completed: 0,
    }));
    let engine = Arc::new(Scripted::new(events));
    let op = RunOperation::new(engine.clone(), Arc::new(FakeClock::new(t(0))))
        .run(id(), fake_job(budget), future::pending())
        .await;
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
    assert_eq!(engine.kills.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_deadline_counts_from_the_index_read() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock.clone());
    let budget = Some(SignedDuration::from_secs(60));
    let running = tokio::spawn(async move {
        use_case
            .run(id(), fake_job(budget), future::pending())
            .await
    });
    engine.send(EngineEvent::Started {
        index_read_at: t(-30),
    });
    settle().await;
    clock.advance(SignedDuration::from_secs(30));
    let op = finished(running).await;
    assert_eq!(op.finished_at(), Some(t(30)));
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
}

#[tokio::test]
async fn an_engine_that_stops_itself_after_its_deadline_failed_the_deadline() {
    let budget = Some(SignedDuration::from_secs(60));
    let engine = Scripted::new(vec![EngineEvent::Started {
        index_read_at: t(-120),
    }]);
    let op = RunOperation::new(Arc::new(engine), Arc::new(FakeClock::new(t(0))))
        .run(id(), fake_job(budget), future::pending())
        .await;
    assert_eq!(
        op.status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
}

#[tokio::test]
async fn a_result_before_the_deadline_stands() {
    let budget = Some(SignedDuration::from_secs(60));
    let engine = Arc::new(Scripted::new(vec![
        started(),
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 }),
    ]));
    let op = RunOperation::new(engine.clone(), Arc::new(FakeClock::new(t(0))))
        .run(id(), fake_job(budget), future::pending())
        .await;
    assert_eq!(op.status(), &OperationStatus::Succeeded);
    assert_eq!(engine.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn without_a_budget_an_operation_has_no_deadline() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock.clone());
    let running =
        tokio::spawn(async move { use_case.run(id(), fake_job(None), future::pending()).await });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_hours(24 * 365));
    settle().await;
    assert_eq!(engine.stops(), 0);
    engine.send(EngineEvent::Succeeded(EngineResult::Fake {
        steps_completed: 0,
    }));
    assert_eq!(
        finished(running).await.status(),
        &OperationStatus::Succeeded
    );
}
