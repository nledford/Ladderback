use std::assert_matches;

use super::support::*;
use super::*;
use crate::composition::repositories::new_operation;
use app::ports::ProgressEvent;

/// An operation log that takes every write at once but the one it holds,
/// an operation's start or its end, and holds that one until the test
/// lets it through.
struct Held {
    inner: Arc<dyn OperationStore>,
    holds: fn(&OperationStatus) -> bool,
    arrived: tokio::sync::Notify,
    open: tokio::sync::watch::Sender<bool>,
}

impl Held {
    /// Over `inner`, holding the write of an operation's end.
    fn end_over(inner: Arc<dyn OperationStore>) -> Self {
        Self::over(inner, OperationStatus::is_finished)
    }

    /// Over `inner`, holding the write that says an operation is running.
    fn start_over(inner: Arc<dyn OperationStore>) -> Self {
        Self::over(inner, |status| *status == OperationStatus::Running)
    }

    fn over(inner: Arc<dyn OperationStore>, holds: fn(&OperationStatus) -> bool) -> Self {
        Self {
            inner,
            holds,
            arrived: tokio::sync::Notify::new(),
            open: tokio::sync::watch::channel(false).0,
        }
    }

    /// Waits until a run has handed the log the write it holds.
    async fn arrived(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.arrived.notified())
            .await
            .expect("the run reached the write the log holds");
    }

    fn let_it_through(&self) {
        self.open.send_replace(true);
    }
}

#[async_trait]
impl OperationStore for Held {
    async fn recover(
        &self,
        reason: &FailureReason,
    ) -> Result<app::ports::Recovered, app::ports::StoreError> {
        self.inner.recover(reason).await
    }

    async fn save(
        &self,
        operation: &domain::execution::Operation,
        result: Option<&app::ports::EngineResult>,
    ) -> Result<(), app::ports::StoreError> {
        if (self.holds)(operation.status()) {
            self.arrived.notify_one();
            let _ = self.open.subscribe().wait_for(|open| *open).await;
        }
        self.inner.save(operation, result).await
    }

    async fn try_start(
        &self,
        operation: &domain::execution::Operation,
    ) -> Result<app::ports::Start, app::ports::StoreError> {
        self.inner.try_start(operation).await
    }

    async fn get(
        &self,
        id: OperationId,
    ) -> Result<Option<app::ports::OperationDetail>, app::ports::StoreError> {
        self.inner.get(id).await
    }

    async fn list(
        &self,
        query: &app::ports::HistoryQuery,
    ) -> Result<app::ports::HistoryPage, app::ports::StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, app::ports::StoreError> {
        self.inner.ever_succeeded(plan).await
    }

    async fn running_maintenance(
        &self,
        repository: Option<&domain::RepositoryId>,
    ) -> Result<Option<OperationId>, app::ports::StoreError> {
        self.inner.running_maintenance(repository).await
    }
}

/// A page that reads the history on hearing a backup ended finds the end
/// there (#134): the daemon announces it once the log has the write, not
/// when the engine says it is done.
#[tokio::test]
async fn a_backup_is_announced_to_the_pages_once_the_log_has_its_end() {
    let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
    open_hooks.send(true).expect("the gate opens");
    let log = Arc::new(Held::end_over(Arc::clone(&runner.store)));
    let mut heard = listen(&mut runner, Completes, log.clone()).await;
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");

    log.arrived().await;
    assert_eq!(
        ends_heard(&mut heard),
        [],
        "the page heard the end before the log had it"
    );

    log.let_it_through();
    let end = next_end(&mut heard).await;
    assert_matches!(&end, UiEvent::OperationSucceeded { operation, .. } if *operation == id);
    assert_eq!(
        history_of(&runner, &id).await.record.status,
        OperationStatus::Succeeded
    );
}

/// The same for maintenance: the reads that come before it (the
/// repository's identity) are not how it ended, so the page hears of one
/// end, once the log has it.
#[tokio::test]
async fn a_check_is_announced_to_the_pages_once_and_once_the_log_has_its_end() {
    let (mut runner, _entered, _open_hooks, _scratch) = runner_at_a_gate().await;
    let log = Arc::new(Held::end_over(Arc::clone(&runner.store)));
    let mut heard = listen(&mut runner, Completes, log.clone()).await;
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts");

    log.arrived().await;
    assert_eq!(
        ends_heard(&mut heard),
        [],
        "the page heard an end before the log had it"
    );

    log.let_it_through();
    let end = next_end(&mut heard).await;
    assert_matches!(&end, UiEvent::OperationSucceeded { operation, .. } if *operation == id);
    assert_eq!(
        history_of(&runner, &id).await.record.status,
        OperationStatus::Succeeded
    );
    assert_eq!(ends_heard(&mut heard), [], "the end was announced twice");
}

/// What the page has heard so far that tells of a start, without waiting
/// for any more.
fn starts_heard(heard: &mut Heard) -> Vec<UiEvent> {
    let mut starts = Vec::new();
    while let Some(Some(news)) = futures::FutureExt::now_or_never(heard.next()) {
        if matches!(news, UiEvent::OperationStarted { .. }) {
            starts.push(news);
        }
    }
    starts
}

/// A page that reads the history on hearing a backup started finds it
/// running there (#305): the daemon announces the start once the log
/// has the write that says so, not when the engine says it started. A
/// page that heard it earlier and read a history that still had the
/// backup queued, its events stream opening only after the news, never
/// learned it ran.
#[tokio::test]
async fn a_backup_is_announced_to_the_pages_as_started_once_the_log_has_it_running() {
    let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
    open_hooks.send(true).expect("the gate opens");
    let log = Arc::new(Held::start_over(Arc::clone(&runner.store)));
    let mut heard = listen(&mut runner, Completes, log.clone()).await;
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");

    log.arrived().await;
    assert_eq!(
        starts_heard(&mut heard),
        [],
        "the page heard the start before the log had it"
    );
    assert_eq!(
        history_of(&runner, &id).await.record.status,
        OperationStatus::Queued
    );

    log.let_it_through();
    let start = tokio::time::timeout(std::time::Duration::from_secs(10), heard.next())
        .await
        .expect("the page hears the start once the log has it");
    assert_matches!(
        &start,
        Some(UiEvent::OperationStarted { operation, .. }) if *operation == id
    );
    assert_ne!(
        history_of(&runner, &id).await.record.status,
        OperationStatus::Queued,
        "the page heard the start, and the log has it"
    );
}

/// A maintenance run is told to have started once, when it has: not by
/// the reads that come before it (the repository's identity), which
/// name the run's operation before the log has it at all.
#[tokio::test]
async fn a_check_is_announced_to_the_pages_as_started_once() {
    let (mut runner, _entered, _open_hooks, _scratch) = runner_at_a_gate().await;
    let log = Arc::clone(&runner.store);
    let mut heard = listen(&mut runner, Completes, log).await;
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts");

    let mut news = Vec::new();
    let end = async {
        while let Some(event) = heard.next().await {
            let ended = is_end(&event);
            news.push(event);
            if ended {
                return;
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), end)
        .await
        .expect("the page hears how the check ended");
    let starts: Vec<&UiEvent> = news
        .iter()
        .filter(|news| matches!(news, UiEvent::OperationStarted { .. }))
        .collect();
    assert_matches!(
        starts.as_slice(),
        [UiEvent::OperationStarted { operation, .. }] if *operation == id,
        "the page hears one start, the run's: {news:?}"
    );
}

/// The engine's own start and end are no news to a page: whoever records
/// the operation announces them, once the log has them. The runner still
/// sees every event.
#[tokio::test]
async fn the_engine_tells_the_pages_neither_how_a_job_started_nor_how_it_ended() {
    let ends = [
        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
        EngineEvent::Failed(FailureReason::EngineFailed {
            detail: "repository not found".to_owned(),
        }),
    ];
    for end in ends {
        let bus = Arc::new(BroadcastEventBus::new());
        let mut heard = bus.subscribe().await;
        let engine = EventEngine {
            inner: Arc::new(Answers(vec![started(), end.clone()])),
            bus,
            clock: Arc::new(SystemClock),
        };
        let job = Job::Fake(app::ports::FakeJob {
            steps: 1,
            step_delay: std::time::Duration::ZERO,
            warnings: Vec::new(),
            end: app::ports::FakeEnd::Succeed,
            deadline_budget: None,
        });
        let operation = OperationId::from_uuid(Uuid::from_u128(7));
        let running = engine
            .run(operation, job)
            .await
            .expect("the engine takes the job");
        let reported: Vec<EngineEvent> = running.events.collect().await;
        assert_eq!(reported, [started(), end.clone()], "the runner's events");

        let mut news = Vec::new();
        while let Some(Some(event)) = futures::FutureExt::now_or_never(heard.next()) {
            news.push(event);
        }
        assert_eq!(news, [], "the page's news of {end:?}");
    }
}

/// An engine that starts every job, tells of a progress bar beginning and
/// of a warning, then answers it as [`Empty`] does, and counts the jobs
/// it was asked.
struct TellsThenEmpty(Arc<std::sync::atomic::AtomicUsize>);

#[async_trait]
impl Engine for TellsThenEmpty {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let empty = Empty.run(op, job).await?;
        let news = [
            started(),
            EngineEvent::Progress(ProgressEvent::Begin {
                key: 1,
                kind: app::ports::ProgressKind::Counter,
                step: Some(app::ports::OperationStep::ReadingIndex),
            }),
            EngineEvent::Warning(domain::execution::Warning {
                kind: WarningKind::SourceUnreadable,
                item: Some("/home/demo/locked".to_owned()),
                during: "reading".to_owned(),
                detail: "permission denied".to_owned(),
            }),
        ];
        Ok(RunningJob {
            events: futures::stream::iter(news).chain(empty.events).boxed(),
            control: empty.control,
        })
    }
}

/// All the page has heard so far, without waiting for any more.
fn heard_now(heard: &mut Heard) -> Vec<UiEvent> {
    let mut news = Vec::new();
    while let Some(Some(one)) = futures::FutureExt::now_or_never(heard.next()) {
        news.push(one);
    }
    news
}

/// A read that is no operation tells a page nothing (#308): the snapshot
/// listing, the groups and the previews of a forget and a prune each
/// start a job, and none of them is a row in the history. A page told of
/// their starts would count them running for ever, and their progress
/// and warnings belong to no row it shows.
#[tokio::test]
async fn reads_that_are_no_operation_tell_the_pages_nothing() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let log = Arc::clone(&runner.store);
    let mut heard = listen(&mut runner, TellsThenEmpty(Arc::clone(&asked)), log).await;
    let catalog = ConfigCatalog {
        config: Arc::clone(&runner.config),
        engine: Arc::clone(&runner.engine),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        leases: None,
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    let mut jobs = 0;
    let mut nothing_from = |read: &str, heard: &mut Heard| {
        let now = asked.load(std::sync::atomic::Ordering::SeqCst);
        assert!(now > jobs, "the {read} reached the engine");
        jobs = now;
        assert_eq!(heard_now(heard), [], "the {read}");
    };

    catalog
        .snapshots("demo".to_owned())
        .await
        .expect("the listing");
    nothing_from("listing", &mut heard);
    runner.groups("demo".to_owned()).await.expect("the groups");
    nothing_from("groups", &mut heard);
    for request in [
        http_adapter::MaintenanceRequest::Forget,
        http_adapter::MaintenanceRequest::Prune,
    ] {
        // What a preview finds is not the point: that it read is.
        let _ = runner.preview("demo".to_owned(), request.clone()).await;
        nothing_from(&format!("preview of {request:?}"), &mut heard);
    }
}

/// Which of the engine's jobs tell the pages their progress and warnings
/// (#308): an operation's, which the history has a row for, and none of
/// the reads', nor the initialising of a repository, which answers its
/// request and has no row.
#[tokio::test]
async fn only_an_operations_job_tells_the_pages_its_progress_and_warnings() {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut heard = bus.subscribe().await;
    let engine = EventEngine {
        inner: Arc::new(TellsThenEmpty(Arc::default())),
        bus,
        clock: Arc::new(SystemClock),
    };
    let access = app::ports::RepositoryAccess {
        repository: "/srv/demo/repo".to_owned(),
        repo_hot: None,
        password: domain::secrets::SecretValue::new("demo-password"),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
        secrets: Vec::new(),
        id: None,
        entry: None,
    };
    let plan = app::ports::PruneJob {
        repository: access.clone(),
        keep_delete: domain::execution::deadline::KeepDelete::MIN,
    };
    let tags = PlanTags::new(
        domain::PlanId::try_from("home".to_owned()).expect("a plan ID"),
        domain::AgentId::from_uuid(uuid::Uuid::from_u128(1)),
    );
    let operations = [
        Job::Fake(app::ports::FakeJob {
            steps: 0,
            step_delay: std::time::Duration::ZERO,
            warnings: Vec::new(),
            end: app::ports::FakeEnd::Succeed,
            deadline_budget: None,
        }),
        Job::Backup(BackupJob {
            repository: access.clone(),
            sources: vec!["/home/demo".to_owned()],
            tags: tags.clone(),
            extra_tags: Vec::new(),
            label: None,
            host: None,
            filter: Default::default(),
            left_out: Vec::new(),
            deadline_budget: domain::SignedDuration::from_hours(1),
        }),
        Job::Forget(app::ports::ForgetJob {
            repository: access.clone(),
            ids: Vec::new(),
        }),
        Job::Rewrite(app::ports::RewriteJob {
            repository: access.clone(),
            ids: Vec::new(),
            tags,
        }),
        Job::Prune(plan.clone()),
        Job::Check(app::ports::CheckJob {
            repository: access.clone(),
            read_data: false,
        }),
    ];
    let reads = [
        Job::Identify(access.clone()),
        Job::Init(access.clone()),
        Job::Ls(access.clone()),
        Job::PrunePlan(plan),
    ];
    for (job, operation) in operations
        .into_iter()
        .map(|job| (job, true))
        .chain(reads.into_iter().map(|job| (job, false)))
    {
        let said = format!("{job:?}");
        let running = engine
            .run(new_operation(), job)
            .await
            .expect("the job runs");
        running.events.collect::<Vec<_>>().await;
        let news = heard_now(&mut heard);
        if operation {
            assert_matches!(
                news.as_slice(),
                [
                    UiEvent::OperationProgress { .. },
                    UiEvent::OperationWarning { .. }
                ],
                "{said}: {news:?}"
            );
        } else {
            assert_eq!(news, [], "{said}");
        }
    }
}

/// A page hears each progress report with the instant the daemon's clock
/// read as it heard it, so the page can say when a run without a total last
/// reported in the clock the run's start is in (§4.5).
#[tokio::test]
async fn a_page_hears_each_progress_report_with_the_instant_the_daemon_heard_it() {
    let bus = Arc::new(BroadcastEventBus::new());
    let mut heard = bus.subscribe().await;
    let at = domain::Timestamp::from_second(1_800_000_000).expect("the demo time is valid");
    let engine = EventEngine {
        inner: Arc::new(TellsThenEmpty(Arc::default())),
        bus,
        clock: Arc::new(app::fakes::FakeClock::new(at)),
    };
    let job = Job::Fake(app::ports::FakeJob {
        steps: 0,
        step_delay: std::time::Duration::ZERO,
        warnings: Vec::new(),
        end: app::ports::FakeEnd::Succeed,
        deadline_budget: None,
    });
    let running = engine
        .run(new_operation(), job)
        .await
        .expect("the job runs");
    running.events.collect::<Vec<_>>().await;
    let news = heard_now(&mut heard);
    let Some(UiEvent::OperationProgress { at: told, .. }) = news.first() else {
        panic!("the page hears the progress first: {news:?}");
    };
    assert_eq!(told.as_deref(), Some("2027-01-15T08:00:00Z"));
}
