//! `RunOperation` when an engine breaks its contract, and when an operation
//! is cancelled or passes its deadline; and what a run writes to the
//! operation log. The well-behaved paths are specified in
//! `features/execution/*.feature`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use app::execution::{Finished, RunOperation};
use app::fakes::{FakeClock, InMemoryOperationStore};
use app::ports::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, HistoryPage, HistoryQuery,
    Job, JobControl, OperationDetail, OperationRecord, OperationStore, ProgressEvent,
    RepositoryAccess, RunningJob, Start, StoreError,
};
use async_trait::async_trait;
use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Transition, Trigger, Warning,
    WarningKind,
};
use domain::secrets::SecretValue;
use domain::{OperationId, RepositoryId, SignedDuration, Timestamp, Uuid};
use futures::channel::{mpsc, oneshot};
use futures::future::{self, BoxFuture, FutureExt};
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

fn repository() -> RepositoryId {
    RepositoryId::parse(&"ab".repeat(32)).unwrap()
}

/// A job that opens the demo repository, whose ID is already known.
fn identify_job() -> Job {
    Job::Identify(RepositoryAccess {
        repository: "/srv/demo/repo".to_owned(),
        password: SecretValue::new("demo-password"),
        options: BTreeMap::new(),
        secrets: Vec::new(),
        id: Some(repository()),
    })
}

fn started() -> EngineEvent {
    EngineEvent::Started {
        index_read_at: t(0),
    }
}

fn succeeded() -> EngineEvent {
    EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 })
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
    events: Mutex<Option<mpsc::UnboundedReceiver<EngineEvent>>>,
    sender: mpsc::UnboundedSender<EngineEvent>,
    stops: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
}

impl Live {
    fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded();
        Self {
            events: Mutex::new(Some(receiver)),
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

/// Takes its time to accept a job: `run` waits for the test to let it go,
/// then plays `then`.
struct Gated {
    release: Mutex<Option<oneshot::Receiver<()>>>,
    then: Scripted,
}

#[async_trait]
impl Engine for Gated {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let release = self
            .release
            .lock()
            .unwrap()
            .take()
            .expect("one job per test");
        let _ = release.await;
        self.then.run(op, job).await
    }
}

/// Refuses every job.
struct Refusing(EngineError);

#[async_trait]
impl Engine for Refusing {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Err(self.0.clone())
    }
}

fn log() -> Arc<InMemoryOperationStore> {
    Arc::new(InMemoryOperationStore::new())
}

/// An operation log that remembers each write it was asked for, in order,
/// and otherwise keeps what it is given.
#[derive(Default)]
struct Journal {
    inner: InMemoryOperationStore,
    writes: Mutex<Vec<OperationStatus>>,
}

impl Journal {
    fn writes(&self) -> Vec<OperationStatus> {
        self.writes.lock().unwrap().clone()
    }
}

#[async_trait]
impl OperationStore for Journal {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        self.writes.lock().unwrap().push(operation.status().clone());
        self.inner.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.writes.lock().unwrap().push(operation.status().clone());
        self.inner.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.inner.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.inner.list(query).await
    }
}

/// An operation log that refuses its first `refuse` writes, then works.
struct Flaky {
    refuse: usize,
    writes: AtomicUsize,
    error: StoreError,
    inner: InMemoryOperationStore,
}

impl Flaky {
    fn refusing(refuse: usize, error: StoreError) -> Self {
        Self {
            refuse,
            writes: AtomicUsize::new(0),
            error,
            inner: InMemoryOperationStore::new(),
        }
    }

    /// A log that is broken for good.
    fn broken(error: StoreError) -> Self {
        Self::refusing(usize::MAX, error)
    }
}

#[async_trait]
impl OperationStore for Flaky {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        if self.writes.fetch_add(1, Ordering::SeqCst) < self.refuse {
            return Err(self.error.clone());
        }
        self.inner.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        if self.writes.fetch_add(1, Ordering::SeqCst) < self.refuse {
            return Err(self.error.clone());
        }
        self.inner.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.inner.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.inner.list(query).await
    }
}

fn disk_error() -> StoreError {
    StoreError::Unavailable {
        detail: "disk I/O error".to_owned(),
    }
}

/// Runs a job on an engine that plays `events`, writing to `store`.
async fn scripted(events: Vec<EngineEvent>, store: Arc<dyn OperationStore>) -> Finished {
    RunOperation::new(
        Arc::new(Scripted::new(events)),
        Arc::new(FakeClock::new(t(0))),
        store,
    )
    .run(id(), Trigger::Manual, fake_job(None), future::pending())
    .await
}

/// The operation an engine that plays `events` leaves behind.
async fn run(events: Vec<EngineEvent>) -> Operation {
    scripted(events, log()).await.operation
}

/// Runs a job that is cancelled five seconds after the engine started it.
async fn cancelled_while_running(store: Arc<dyn OperationStore>) -> Finished {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let (cancel, cancelled) = oneshot::channel::<()>();
    let use_case = RunOperation::new(engine.clone(), clock.clone(), store);
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(None), cancelled.map(|_| ()))
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_secs(5));
    cancel.send(()).unwrap();
    finished(running).await
}

/// Runs a job whose deadline, a minute after it read the index, passes while
/// the engine is still running it.
async fn killed_at_its_deadline(store: Arc<dyn OperationStore>) -> Finished {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock.clone(), store);
    let budget = Some(SignedDuration::from_secs(60));
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(budget), future::pending())
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_secs(60));
    finished(running).await
}

/// Runs a job on an engine that refuses it.
async fn refused(error: EngineError, store: Arc<dyn OperationStore>) -> Finished {
    RunOperation::new(
        Arc::new(Refusing(error)),
        Arc::new(FakeClock::new(t(0))),
        store,
    )
    .run(id(), Trigger::Manual, fake_job(None), future::pending())
    .await
}

/// Lets spawned work run until it waits on something.
async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

/// The spawned run's outcome. Fails rather than hangs if the run never
/// ends: a missed cancel or deadline would otherwise wait forever.
async fn finished(running: tokio::task::JoinHandle<Finished>) -> Finished {
    tokio::time::timeout(std::time::Duration::from_secs(5), running)
        .await
        .expect("the operation ended")
        .unwrap()
}

/// What the log holds for the operation these tests run.
async fn logged(store: &InMemoryOperationStore) -> OperationDetail {
    store
        .get(id())
        .await
        .unwrap()
        .expect("the operation is in the log")
}

/// The log holds the operation exactly as the run ended it.
async fn assert_logged_as_it_ended(store: &InMemoryOperationStore, ended: &Finished) {
    let detail = logged(store).await;
    assert_eq!(
        detail.record,
        OperationRecord::of(&ended.operation, ended.result.as_ref())
    );
    assert_eq!(detail.warnings, ended.operation.warnings());
    assert!(
        ended.recording_errors.is_empty(),
        "{:?}",
        ended.recording_errors
    );
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
    let use_case = RunOperation::new(engine.clone(), clock.clone(), log());
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(None), cancelled.map(|_| ()))
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_secs(5));
    cancel.send(()).unwrap();
    let op = finished(running).await.operation;
    assert_eq!(op.status(), &OperationStatus::Cancelled);
    assert_eq!(op.finished_at(), Some(t(5)));
    assert_eq!(engine.stops(), 1);
}

#[tokio::test]
async fn cancelling_before_the_engine_starts_cancels_the_queued_operation() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock, log());
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(None), future::ready(()))
            .await
    });
    let op = finished(running).await.operation;
    assert_eq!(op.status(), &OperationStatus::Cancelled);
    assert_eq!(op.started_at(), None);
    assert_eq!(engine.stops(), 1);
}

#[tokio::test]
async fn an_operation_running_at_its_deadline_is_killed_and_fails() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock.clone(), log());
    let budget = Some(SignedDuration::from_secs(60));
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(budget), future::pending())
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_secs(59));
    settle().await;
    assert_eq!(engine.kills(), 0, "killed before the deadline");
    clock.advance(SignedDuration::from_secs(1));
    let op = finished(running).await.operation;
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
    let op = RunOperation::new(engine.clone(), Arc::new(FakeClock::new(t(0))), log())
        .run(id(), Trigger::Manual, fake_job(budget), future::pending())
        .await
        .operation;
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
    let use_case = RunOperation::new(engine.clone(), clock.clone(), log());
    let budget = Some(SignedDuration::from_secs(60));
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(budget), future::pending())
            .await
    });
    engine.send(EngineEvent::Started {
        index_read_at: t(-30),
    });
    settle().await;
    clock.advance(SignedDuration::from_secs(30));
    let op = finished(running).await.operation;
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
    let op = RunOperation::new(Arc::new(engine), Arc::new(FakeClock::new(t(0))), log())
        .run(id(), Trigger::Manual, fake_job(budget), future::pending())
        .await
        .operation;
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
    let op = RunOperation::new(engine.clone(), Arc::new(FakeClock::new(t(0))), log())
        .run(id(), Trigger::Manual, fake_job(budget), future::pending())
        .await
        .operation;
    assert_eq!(op.status(), &OperationStatus::Succeeded);
    assert_eq!(engine.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn without_a_budget_an_operation_has_no_deadline() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine.clone(), clock.clone(), log());
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(None), future::pending())
            .await
    });
    engine.send(started());
    settle().await;
    clock.advance(SignedDuration::from_hours(24 * 365));
    settle().await;
    assert_eq!(engine.stops(), 0);
    engine.send(EngineEvent::Succeeded(EngineResult::Fake {
        steps_completed: 0,
    }));
    assert_eq!(
        finished(running).await.operation.status(),
        &OperationStatus::Succeeded
    );
}

// The operation log. Every run writes it as it goes (§9): queued before the
// engine is asked to run anything, running once the engine says so, and as it
// finished.

#[tokio::test]
async fn an_operation_is_in_the_log_as_queued_before_the_engine_accepts_its_job() {
    let (release, held) = oneshot::channel();
    let engine = Gated {
        release: Mutex::new(Some(held)),
        then: Scripted::new(vec![
            started(),
            EngineEvent::Succeeded(EngineResult::Repository { id: repository() }),
        ]),
    };
    let clock = Arc::new(FakeClock::new(t(0)));
    let store = log();
    let use_case = RunOperation::new(Arc::new(engine), clock.clone(), store.clone());
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Schedule, identify_job(), future::pending())
            .await
    });
    settle().await;

    let waiting = logged(&store).await.record;
    assert_eq!(waiting.status, OperationStatus::Queued);
    assert_eq!(waiting.queued_at, t(0));
    assert_eq!((waiting.started_at, waiting.finished_at), (None, None));
    assert_eq!(waiting.kind, OperationKind::Identify);
    assert_eq!(waiting.trigger, Trigger::Schedule);

    release.send(()).unwrap();
    let ended = finished(running).await;
    assert_eq!(ended.operation.status(), &OperationStatus::Succeeded);
    assert_logged_as_it_ended(&store, &ended).await;
}

#[tokio::test]
async fn the_log_shows_the_operation_running_as_soon_as_the_engine_has_started_it() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let engine = Arc::new(Live::new());
    let store = log();
    let use_case = RunOperation::new(engine.clone(), clock.clone(), store.clone());
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, fake_job(None), future::pending())
            .await
    });
    // Let the run queue the operation before time moves.
    settle().await;
    clock.advance(SignedDuration::from_secs(3));
    engine.send(started());
    settle().await;

    let record = logged(&store).await.record;
    assert_eq!(record.status, OperationStatus::Running);
    assert_eq!(record.queued_at, t(0));
    assert_eq!(record.started_at, Some(t(3)));
    assert_eq!(record.finished_at, None);
    assert_eq!(record.result, None);

    clock.advance(SignedDuration::from_secs(4));
    engine.send(EngineEvent::Succeeded(EngineResult::Fake {
        steps_completed: 2,
    }));
    let ended = finished(running).await;
    let record = logged(&store).await.record;
    assert_eq!(record.status, OperationStatus::Succeeded);
    assert_eq!(record.finished_at, Some(t(7)));
    assert_eq!(
        record.result,
        Some(EngineResult::Fake { steps_completed: 2 })
    );
    assert_logged_as_it_ended(&store, &ended).await;
}

#[tokio::test]
async fn every_way_a_run_can_end_is_in_the_log_as_it_ended() {
    let cases: Vec<(&str, Vec<EngineEvent>, OperationStatus)> = vec![
        (
            "a success",
            vec![started(), succeeded()],
            OperationStatus::Succeeded,
        ),
        (
            "a success with warnings",
            vec![
                started(),
                EngineEvent::Warning(warning()),
                EngineEvent::Warning(warning()),
                succeeded(),
            ],
            OperationStatus::SucceededWithWarnings,
        ),
        (
            "a failure the engine reports",
            vec![
                started(),
                EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "no space left on device".to_owned(),
                }),
            ],
            OperationStatus::Failed {
                reason: FailureReason::EngineFailed {
                    detail: "no space left on device".to_owned(),
                },
            },
        ),
        (
            "an engine that vanishes",
            vec![started()],
            OperationStatus::Failed {
                reason: FailureReason::EngineExited { exit_code: None },
            },
        ),
        (
            "an event out of order",
            vec![EngineEvent::Warning(warning()), started()],
            OperationStatus::Failed {
                reason: FailureReason::ProtocolViolated {
                    transition: Transition::RecordWarning,
                },
            },
        ),
    ];
    for (name, events, status) in cases {
        let store = log();
        let ended = scripted(events, store.clone()).await;
        assert_eq!(ended.operation.status(), &status, "{name}");
        assert_logged_as_it_ended(&store, &ended).await;
        let record = logged(&store).await.record;
        assert_eq!(record.status, status, "{name}");
        assert_eq!(record.finished_at, Some(t(0)), "{name}");
    }
}

#[tokio::test]
async fn a_cancelled_operation_is_in_the_log_as_cancelled() {
    let store = log();
    let ended = cancelled_while_running(store.clone()).await;
    assert_eq!(ended.operation.status(), &OperationStatus::Cancelled);
    assert_logged_as_it_ended(&store, &ended).await;
    let record = logged(&store).await.record;
    assert_eq!(record.status, OperationStatus::Cancelled);
    assert_eq!(record.started_at, Some(t(0)));
    assert_eq!(record.finished_at, Some(t(5)));
}

#[tokio::test]
async fn an_operation_cancelled_before_it_starts_is_in_the_log_as_cancelled() {
    let store = log();
    let engine = Arc::new(Live::new());
    let use_case = RunOperation::new(engine, Arc::new(FakeClock::new(t(0))), store.clone());
    let ended = use_case
        .run(id(), Trigger::Manual, fake_job(None), future::ready(()))
        .await;
    assert_logged_as_it_ended(&store, &ended).await;
    let record = logged(&store).await.record;
    assert_eq!(record.status, OperationStatus::Cancelled);
    assert_eq!(record.started_at, None);
}

#[tokio::test]
async fn an_operation_killed_at_its_deadline_is_in_the_log_as_failed() {
    let store = log();
    let ended = killed_at_its_deadline(store.clone()).await;
    assert_logged_as_it_ended(&store, &ended).await;
    let record = logged(&store).await.record;
    assert_eq!(
        record.status,
        OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
    assert_eq!(record.finished_at, Some(t(60)));
}

#[tokio::test]
async fn what_the_engine_reported_on_success_is_logged_and_returned() {
    let first = EngineResult::Fake { steps_completed: 7 };
    let store = log();
    let ended = scripted(
        vec![
            started(),
            EngineEvent::Succeeded(first.clone()),
            EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 8 }),
        ],
        store.clone(),
    )
    .await;
    assert_eq!(ended.result, Some(first.clone()));
    assert_eq!(logged(&store).await.record.result, Some(first));
}

#[tokio::test]
async fn a_result_the_operation_refused_is_neither_logged_nor_returned() {
    // A result before the engine has started: the operation fails instead.
    let store = log();
    let ended = scripted(vec![succeeded(), started()], store.clone()).await;
    assert_eq!(
        ended.operation.status(),
        &OperationStatus::Failed {
            reason: FailureReason::ProtocolViolated {
                transition: Transition::Succeed
            }
        }
    );
    assert_eq!(ended.result, None);
    assert_eq!(logged(&store).await.record.result, None);
}

#[tokio::test]
async fn an_operation_is_logged_with_its_trigger_and_what_its_job_targets() {
    let result = EngineResult::Repository { id: repository() };
    let store = log();
    let ended = RunOperation::new(
        Arc::new(Scripted::new(vec![
            started(),
            EngineEvent::Succeeded(result.clone()),
        ])),
        Arc::new(FakeClock::new(t(0))),
        store.clone(),
    )
    .run(id(), Trigger::Retry, identify_job(), future::pending())
    .await;
    assert_logged_as_it_ended(&store, &ended).await;
    let record = logged(&store).await.record;
    assert_eq!(record.kind, OperationKind::Identify);
    assert_eq!(record.trigger, Trigger::Retry);
    assert_eq!(
        record.target,
        Target {
            plan: None,
            repository: Some(repository())
        }
    );
    assert_eq!(record.result, Some(result));
}

#[tokio::test]
async fn a_busy_run_writes_the_log_when_its_state_changes_not_for_every_event() {
    let mut events = vec![started()];
    events.extend((0..20).map(|key| EngineEvent::Progress(ProgressEvent::Finish { key })));
    events.extend(std::iter::repeat_n(EngineEvent::Warning(warning()), 20));
    events.push(succeeded());
    let journal = Arc::new(Journal::default());
    let ended = scripted(events, journal.clone()).await;
    assert_eq!(
        ended.operation.status(),
        &OperationStatus::SucceededWithWarnings
    );
    assert_eq!(
        journal.writes(),
        [
            OperationStatus::Queued,
            OperationStatus::Running,
            OperationStatus::SucceededWithWarnings
        ]
    );
}

#[tokio::test]
async fn an_engine_that_never_took_the_job_fails_the_queued_operation() {
    let store = log();
    let ended = refused(EngineError::StartTimedOut, store.clone()).await;
    assert_eq!(
        ended.operation.status(),
        &OperationStatus::Failed {
            reason: FailureReason::EngineStartTimedOut
        }
    );
    assert_eq!(ended.operation.started_at(), None);
    assert_logged_as_it_ended(&store, &ended).await;
    assert_eq!(
        logged(&store).await.record.status,
        OperationStatus::Failed {
            reason: FailureReason::EngineStartTimedOut
        }
    );
}

#[tokio::test]
async fn an_engine_that_cannot_start_is_logged_with_the_platforms_own_message() {
    let detail = "No such file or directory (os error 2)".to_owned();
    let store = log();
    let ended = refused(
        EngineError::Unavailable {
            detail: detail.clone(),
        },
        store.clone(),
    )
    .await;
    assert_logged_as_it_ended(&store, &ended).await;
    assert_eq!(
        logged(&store).await.record.status,
        OperationStatus::Failed {
            reason: FailureReason::EngineUnavailable { detail }
        }
    );
}

/// Backups never depend on the database (§9): a log that can't be written
/// is reported and nothing else.
#[tokio::test]
async fn a_log_that_fails_never_changes_how_an_operation_ends() {
    type Ending = fn(Arc<dyn OperationStore>) -> BoxFuture<'static, Finished>;
    let endings: Vec<(&str, Ending)> = vec![
        ("a success", |store| {
            scripted(vec![started(), succeeded()], store).boxed()
        }),
        ("a success with warnings", |store| {
            scripted(
                vec![started(), EngineEvent::Warning(warning()), succeeded()],
                store,
            )
            .boxed()
        }),
        ("an engine that vanishes", |store| {
            scripted(vec![started()], store).boxed()
        }),
        ("an event out of order", |store| {
            scripted(vec![succeeded(), started()], store).boxed()
        }),
        ("a cancel", |store| cancelled_while_running(store).boxed()),
        ("a deadline", |store| killed_at_its_deadline(store).boxed()),
        ("an engine that never took the job", |store| {
            refused(EngineError::StartTimedOut, store).boxed()
        }),
    ];
    for (name, ending) in endings {
        let journal = Arc::new(Journal::default());
        let healthy = ending(journal.clone()).await;
        assert!(healthy.recording_errors.is_empty(), "{name}");

        let broken = ending(Arc::new(Flaky::broken(disk_error()))).await;
        assert_eq!(broken.operation, healthy.operation, "{name}");
        assert_eq!(broken.result, healthy.result, "{name}");
        assert_eq!(
            broken.recording_errors,
            vec![disk_error(); journal.writes().len()],
            "{name}: one error for each write the log refused"
        );
    }
}

#[tokio::test]
async fn a_log_that_missed_a_write_is_made_good_by_the_next() {
    let store = Arc::new(Flaky::refusing(1, disk_error()));
    let ended = scripted(vec![started(), succeeded()], store.clone()).await;
    assert_eq!(ended.recording_errors, [disk_error()]);
    assert_eq!(ended.operation.status(), &OperationStatus::Succeeded);
    // Each write is the whole operation as it stands, so the history of a run
    // whose first write failed is complete.
    let detail = logged(&store.inner).await;
    assert_eq!(
        detail.record,
        OperationRecord::of(&ended.operation, ended.result.as_ref())
    );
}
