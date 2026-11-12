//! The order a run does things in, against the news of its start and its end
//! (technical design §4.4, §9; #134, #305): a page that hears an operation
//! started or ended and then reads the history must find it there, so the
//! news of the start waits for the write that says the operation is running,
//! and the news of the end for the run's last write to the log and for the
//! hooks that come before it. The scenarios of
//! `features/execution/announcement.feature` play a page that reads the
//! history on the news; these hold the log's write for as long as they like,
//! record the order of everything a run does, and cover each way a run can
//! fail.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use api::{ErrorCode, MsgArg, UiEvent};
use app::event_bus::BroadcastEventBus;
use app::execution::{Finished, RunOperation};
use app::fakes::{FakeClock, InMemoryOperationStore};
use app::ports::{
    Engine, EngineError, EngineEvent, EngineResult, EventBus, FakeEnd, FakeJob, HistoryPage,
    HistoryQuery, HookContext, HookOutcome, HookPhase, HookRunner, HookSetup, HookStatus, Job,
    JobControl, OperationDetail, OperationStore, RunningJob, Start, StoreError,
};
use async_trait::async_trait;
use domain::execution::{FailureReason, Operation, OperationStatus, Trigger};
use domain::planning::{HookEntry, Hooks, OnFailure};
use domain::{OperationId, PlanId, RepositoryAlias, SignedDuration, Timestamp, Uuid};
use futures::future::{self, FutureExt};
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::{Notify, watch};

const ID: &str = "00000000-0000-0000-0000-000000000001";

fn t(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn id() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(1))
}

fn job(deadline_budget: Option<SignedDuration>) -> Job {
    Job::Fake(FakeJob {
        steps: 0,
        step_delay: Duration::ZERO,
        warnings: Vec::new(),
        end: FakeEnd::Succeed,
        deadline_budget,
    })
}

fn started() -> EngineEvent {
    EngineEvent::Started {
        index_read_at: t(0),
        repository: None,
    }
}

fn succeeded() -> EngineEvent {
    EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 0 })
}

fn failed_saying(detail: &str) -> EngineEvent {
    EngineEvent::Failed(FailureReason::EngineFailed {
        detail: detail.to_owned(),
    })
}

/// The news that the operation these tests run started, at `at`.
fn started_at(at: Timestamp) -> UiEvent {
    UiEvent::OperationStarted {
        operation: ID.to_owned(),
        repository: None,
        started_at: at.to_string(),
    }
}

/// The news that the operation these tests run ended well, at `at`.
fn succeeded_at(at: Timestamp) -> UiEvent {
    UiEvent::OperationSucceeded {
        operation: ID.to_owned(),
        finished_at: at.to_string(),
    }
}

/// The news that the operation these tests run failed with `code`, at `at`.
fn failed_at(at: Timestamp, code: ErrorCode, args: Vec<MsgArg>) -> UiEvent {
    UiEvent::OperationFailed {
        operation: ID.to_owned(),
        code,
        args,
        finished_at: at.to_string(),
    }
}

/// Stops nothing: the job it controls ran by itself.
struct Idle;

#[async_trait]
impl JobControl for Idle {
    async fn stop(&mut self) {}
}

/// An engine that plays `events` and then ends its stream, or leaves it
/// waiting as a job that is still working does.
struct Plays {
    events: Vec<EngineEvent>,
    ends: bool,
}

impl Plays {
    fn then_ends(events: Vec<EngineEvent>) -> Self {
        Self { events, ends: true }
    }

    fn then_waits(events: Vec<EngineEvent>) -> Self {
        Self {
            events,
            ends: false,
        }
    }
}

#[async_trait]
impl Engine for Plays {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        let played = stream::iter(self.events.clone());
        let events = if self.ends {
            played.boxed()
        } else {
            played.chain(stream::pending()).boxed()
        };
        Ok(RunningJob {
            events,
            control: Box::new(Idle),
        })
    }
}

/// An engine that refuses every job.
struct Refuses(EngineError);

#[async_trait]
impl Engine for Refuses {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Err(self.0.clone())
    }
}

/// An operation log that takes every write at once but the one it holds,
/// the start's or the end's, and holds that one until the test lets it
/// through.
struct Held {
    inner: InMemoryOperationStore,
    holds: fn(&OperationStatus) -> bool,
    arrived: Notify,
    open: watch::Sender<bool>,
}

impl Held {
    fn new(holds: fn(&OperationStatus) -> bool) -> Self {
        Self {
            inner: InMemoryOperationStore::new(),
            holds,
            arrived: Notify::new(),
            open: watch::channel(false).0,
        }
    }

    /// A log that holds the write of an operation's end.
    fn end() -> Self {
        Self::new(OperationStatus::is_finished)
    }

    /// A log that holds the write that says an operation is running.
    fn start() -> Self {
        Self::new(|status| *status == OperationStatus::Running)
    }

    /// Waits until the run has handed the log the write it holds.
    async fn arrived(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.arrived.notified())
            .await
            .expect("the run reached the write the log holds");
    }

    fn let_it_through(&self) {
        self.open.send_replace(true);
    }
}

#[async_trait]
impl OperationStore for Held {
    async fn fail_unfinished(
        &self,
        reason: &domain::execution::FailureReason,
    ) -> Result<u64, StoreError> {
        self.inner.fail_unfinished(reason).await
    }

    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        if (self.holds)(operation.status()) {
            self.arrived.notify_one();
            let _ = self.open.subscribe().wait_for(|open| *open).await;
        }
        self.inner.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.inner.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.inner.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, StoreError> {
        self.inner.ever_succeeded(plan).await
    }
}

/// What a page has heard so far, without waiting for any more.
fn heard_so_far(page: &mut BoxStream<'static, UiEvent>) -> Vec<UiEvent> {
    let mut news = Vec::new();
    while let Some(Some(event)) = page.next().now_or_never() {
        news.push(event);
    }
    news
}

/// Runs `engine`'s job to its end for a page that listens, and says how it
/// ended and what the page heard.
async fn run_heard_by_a_page(engine: impl Engine + 'static) -> (Finished, Vec<UiEvent>) {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut page = bus.subscribe().await;
    let ended = RunOperation::new(
        Arc::new(engine),
        Arc::new(FakeClock::new(t(0))),
        Arc::new(InMemoryOperationStore::new()),
    )
    .with_events(bus)
    .run(id(), Trigger::Manual, job(None), future::pending())
    .await;
    (ended, heard_so_far(&mut page))
}

/// #134's own test: a log that holds the write of the end, and a page
/// that must hear nothing of the end until that write is let through. The
/// clock is one nobody moves, so the run never gives up on the write.
#[tokio::test]
async fn the_end_is_not_announced_until_the_log_has_it() {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut page = bus.subscribe().await;
    let store = Arc::new(Held::end());
    let use_case = RunOperation::new(
        Arc::new(Plays::then_ends(vec![started(), succeeded()])),
        Arc::new(FakeClock::new(t(0))),
        store.clone(),
    )
    .with_events(bus);
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, job(None), future::pending())
            .await
    });

    store.arrived().await;
    assert_eq!(
        heard_so_far(&mut page),
        [started_at(t(0))],
        "the page heard the end before the log had it"
    );
    assert_eq!(
        store.get(id()).await.unwrap().unwrap().record.status,
        OperationStatus::Running,
        "the log has had the end"
    );

    store.let_it_through();
    let ended = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the run ended once the log took its end")
        .unwrap();
    assert_eq!(ended.operation.status(), &OperationStatus::Succeeded);
    assert_eq!(heard_so_far(&mut page), [succeeded_at(t(0))]);
    assert_eq!(
        store.get(id()).await.unwrap().unwrap().record.status,
        OperationStatus::Succeeded,
        "the page heard the end, and the log has it"
    );
}

/// #305's own test: a log that holds the write that says the operation is
/// running, and a page that must hear nothing of the start until that write
/// is let through. A page that read the history on hearing of the start
/// earlier could find the operation still queued, and, its events stream
/// open only after the start, never learn it ran.
#[tokio::test]
async fn the_start_is_not_announced_until_the_log_has_it() {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut page = bus.subscribe().await;
    let store = Arc::new(Held::start());
    let use_case = RunOperation::new(
        Arc::new(Plays::then_waits(vec![started()])),
        Arc::new(FakeClock::new(t(0))),
        store.clone(),
    )
    .with_events(bus);
    let _running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, job(None), future::pending())
            .await
    });

    store.arrived().await;
    assert_eq!(
        heard_so_far(&mut page),
        [],
        "the page heard the start before the log had it"
    );
    assert_eq!(
        store.get(id()).await.unwrap().unwrap().record.status,
        OperationStatus::Queued,
        "the log has had the start"
    );

    store.let_it_through();
    let news = tokio::time::timeout(Duration::from_secs(5), page.next())
        .await
        .expect("the page hears the start once the log has it");
    assert_eq!(news, Some(started_at(t(0))));
    assert_eq!(
        store.get(id()).await.unwrap().unwrap().record.status,
        OperationStatus::Running,
        "the page heard the start, and the log has it"
    );
}

/// A log that refuses the write that says the operation is running doesn't
/// keep the page from hearing it started: the history then shows it queued,
/// as an end the log refused shows it running, and the page never goes back
/// on a status it heard.
#[tokio::test]
async fn a_start_the_log_refuses_is_announced_all_the_same() {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut page = bus.subscribe().await;
    let store = Arc::new(RefusesTheStart(InMemoryOperationStore::new()));
    let use_case = RunOperation::new(
        Arc::new(Plays::then_waits(vec![started()])),
        Arc::new(FakeClock::new(t(0))),
        store,
    )
    .with_events(bus);
    let _running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, job(None), future::pending())
            .await
    });
    let news = tokio::time::timeout(Duration::from_secs(5), page.next())
        .await
        .expect("the page hears the start");
    assert_eq!(news, Some(started_at(t(0))));
}

/// A run cancelled while the log holds the write that says it is running
/// tells the page nothing: not that it started, which the log never said,
/// and not how it ended, as no cancelled run does.
#[tokio::test]
async fn a_run_cancelled_before_the_log_has_its_start_announces_nothing() {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut page = bus.subscribe().await;
    let store = Arc::new(Held::start());
    let use_case = RunOperation::new(
        Arc::new(Plays::then_waits(vec![started()])),
        Arc::new(FakeClock::new(t(0))),
        store.clone(),
    )
    .with_events(bus);
    let (cancel, cancelled) = futures::channel::oneshot::channel::<()>();
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, job(None), async {
                let _ = cancelled.await;
            })
            .await
    });

    store.arrived().await;
    cancel.send(()).unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the run ended on the cancel")
        .unwrap();
    assert_eq!(ended.operation.status(), &OperationStatus::Cancelled);
    assert_eq!(heard_so_far(&mut page), []);
}

/// An operation log that refuses the write that says an operation is
/// running, and takes every other.
struct RefusesTheStart(InMemoryOperationStore);

#[async_trait]
impl OperationStore for RefusesTheStart {
    async fn fail_unfinished(
        &self,
        reason: &domain::execution::FailureReason,
    ) -> Result<u64, StoreError> {
        self.0.fail_unfinished(reason).await
    }

    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        if *operation.status() == OperationStatus::Running {
            return Err(StoreError::Unavailable {
                detail: "disk I/O error".to_owned(),
            });
        }
        self.0.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.0.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.0.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.0.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, StoreError> {
        self.0.ever_succeeded(plan).await
    }
}

/// A job whose access names no repository ID, as a first job's does (its
/// entry records none yet, decision 57): the page still hears which
/// repository it started on, the one the engine says it opened.
#[tokio::test]
async fn a_first_jobs_start_names_the_repository_the_engine_opened() {
    let opened = "b".repeat(64);
    let (_, news) = run_heard_by_a_page(Plays::then_ends(vec![
        EngineEvent::Started {
            index_read_at: t(0),
            repository: Some(domain::RepositoryId::parse(&opened).unwrap()),
        },
        succeeded(),
    ]))
    .await;
    assert_eq!(
        news,
        [
            UiEvent::OperationStarted {
                operation: ID.to_owned(),
                repository: Some(opened),
                started_at: t(0).to_string(),
            },
            succeeded_at(t(0)),
        ]
    );
}

/// What a run did, in the order it did it.
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<String>>>);

impl Journal {
    fn note(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }

    fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn word(status: &OperationStatus) -> &'static str {
    match status {
        OperationStatus::Queued => "queued",
        OperationStatus::Running => "running",
        OperationStatus::Succeeded => "succeeded",
        OperationStatus::SucceededWithWarnings => "succeeded with warnings",
        OperationStatus::Failed { .. } => "failed",
        OperationStatus::Cancelled => "cancelled",
    }
}

/// A log that notes each write, as the status it holds.
struct NotesWrites {
    journal: Journal,
    inner: InMemoryOperationStore,
}

#[async_trait]
impl OperationStore for NotesWrites {
    async fn fail_unfinished(
        &self,
        reason: &domain::execution::FailureReason,
    ) -> Result<u64, StoreError> {
        self.inner.fail_unfinished(reason).await
    }

    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        self.journal
            .note(format!("write {}", word(operation.status())));
        self.inner.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.inner.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.inner.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, StoreError> {
        self.inner.ever_succeeded(plan).await
    }
}

/// Hooks that note the phase they run for, and fail in the one they are told
/// to.
struct NotesHooks {
    journal: Journal,
    fails_in: Option<HookPhase>,
}

#[async_trait]
impl HookRunner for NotesHooks {
    async fn run(&self, _entry: &HookEntry, context: &HookContext) -> HookOutcome {
        self.journal
            .note(format!("hook {}", context.phase.as_env()));
        HookOutcome {
            status: if self.fails_in == Some(context.phase) {
                HookStatus::Failed { exit_code: Some(3) }
            } else {
                HookStatus::Succeeded
            },
            stdout: Default::default(),
            stderr: Default::default(),
        }
    }
}

/// A bus that notes each start and end it is told of.
struct NotesNews {
    journal: Journal,
}

#[async_trait]
impl EventBus for NotesNews {
    async fn publish(&self, event: UiEvent) {
        match event {
            UiEvent::OperationStarted { .. } => self.journal.note("announce started"),
            UiEvent::OperationSucceeded { .. } => self.journal.note("announce succeeded"),
            UiEvent::OperationFailed { .. } => self.journal.note("announce failed"),
            _ => {}
        }
    }

    async fn subscribe(&self) -> BoxStream<'static, UiEvent> {
        stream::empty().boxed()
    }
}

/// One hook of each of the four lists, at the agent level.
fn hooks_in_every_list() -> HookSetup {
    let entry = || HookEntry::new("demo-hook", ["demo"], OnFailure::Error).unwrap();
    HookSetup {
        agent: Hooks {
            run_before: vec![entry()],
            run_after: vec![entry()],
            run_failed: vec![entry()],
            run_finally: vec![entry()],
        },
        repository: Hooks::default(),
        plan: Hooks::default(),
        plan_id: PlanId::parse("demo-plan").unwrap(),
        repository_alias: RepositoryAlias::parse("nas").unwrap(),
    }
}

/// Runs a job that plays `events` with hooks in every list, and says what
/// the run did, in order.
async fn what_a_run_does(
    events: Vec<EngineEvent>,
    hooks_fail_in: Option<HookPhase>,
) -> Vec<String> {
    let journal = Journal::default();
    RunOperation::new(
        Arc::new(Plays::then_ends(events)),
        Arc::new(FakeClock::new(t(0))),
        Arc::new(NotesWrites {
            journal: journal.clone(),
            inner: InMemoryOperationStore::new(),
        }),
    )
    .with_hooks(
        Arc::new(NotesHooks {
            journal: journal.clone(),
            fails_in: hooks_fail_in,
        }),
        hooks_in_every_list(),
    )
    .with_events(Arc::new(NotesNews {
        journal: journal.clone(),
    }))
    .run(id(), Trigger::Manual, job(None), future::pending())
    .await;
    journal.entries()
}

#[tokio::test]
async fn a_success_is_announced_after_its_hooks_and_the_write_of_its_end() {
    assert_eq!(
        what_a_run_does(vec![started(), succeeded()], None).await,
        [
            "write queued",
            "hook before",
            "write running",
            "announce started",
            "hook after",
            "hook finally",
            "write succeeded",
            "announce succeeded",
        ]
    );
}

#[tokio::test]
async fn a_failure_is_announced_after_its_hooks_and_the_write_of_its_end() {
    assert_eq!(
        what_a_run_does(vec![started(), failed_saying("repository not found")], None).await,
        [
            "write queued",
            "hook before",
            "write running",
            "announce started",
            "hook failed",
            "hook finally",
            "write failed",
            "announce failed",
        ]
    );
}

#[tokio::test]
async fn a_run_stopped_by_a_before_hook_is_announced_after_the_write_of_its_end() {
    assert_eq!(
        what_a_run_does(vec![started(), succeeded()], Some(HookPhase::Before)).await,
        [
            "write queued",
            "hook before",
            "hook finally",
            "write failed",
            "announce failed",
        ]
    );
}

// The failures a run finds out for itself, as the engine never reported them:
// each ends the operation as failed in the log, and the page is told as it is
// of one the engine reported.

#[tokio::test]
async fn an_engine_that_cannot_be_started_is_announced_as_unavailable() {
    let (ended, news) = run_heard_by_a_page(Refuses(EngineError::Unavailable {
        detail: "no such program".to_owned(),
    }))
    .await;
    assert!(ended.operation.status().is_finished());
    assert_eq!(
        news,
        [failed_at(
            t(0),
            ErrorCode::EngineUnavailable,
            vec![MsgArg::Text("no such program".to_owned())]
        )]
    );
}

#[tokio::test]
async fn an_engine_that_does_not_take_its_job_in_time_is_announced_as_failed() {
    let (_, news) = run_heard_by_a_page(Refuses(EngineError::StartTimedOut)).await;
    assert_eq!(
        news,
        [failed_at(t(0), ErrorCode::EngineStartTimedOut, vec![])]
    );
}

#[tokio::test]
async fn an_engine_that_stops_without_a_result_is_announced_as_exited() {
    let (ended, news) = run_heard_by_a_page(Plays::then_ends(vec![started()])).await;
    assert_eq!(
        ended.operation.status(),
        &OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: None }
        }
    );
    assert_eq!(
        news,
        [
            started_at(t(0)),
            failed_at(t(0), ErrorCode::EngineExited, vec![])
        ]
    );
}

#[tokio::test]
async fn an_event_out_of_order_is_announced_as_a_protocol_violation() {
    let (_, news) = run_heard_by_a_page(Plays::then_ends(vec![succeeded()])).await;
    assert_eq!(
        news,
        [failed_at(
            t(0),
            ErrorCode::ProtocolViolated,
            vec![MsgArg::Text("Succeed".to_owned())]
        )]
    );
}

#[tokio::test]
async fn a_run_stopped_by_its_deadline_is_announced_as_failed() {
    let clock = Arc::new(FakeClock::new(t(0)));
    let bus = Arc::new(BroadcastEventBus::new());
    let mut page = bus.subscribe().await;
    let budget = SignedDuration::from_secs(60);
    let use_case = RunOperation::new(
        Arc::new(Plays::then_waits(vec![started()])),
        clock.clone(),
        Arc::new(InMemoryOperationStore::new()),
    )
    .with_events(bus);
    let running = tokio::spawn(async move {
        use_case
            .run(id(), Trigger::Manual, job(Some(budget)), future::pending())
            .await
    });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        heard_so_far(&mut page),
        [started_at(t(0))],
        "the run is still working"
    );

    clock.advance(budget);
    let ended = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the run ended at its deadline")
        .unwrap();
    assert_eq!(
        ended.operation.status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
    assert_eq!(
        heard_so_far(&mut page),
        [failed_at(t(60), ErrorCode::DeadlinePassed, vec![])]
    );
}
