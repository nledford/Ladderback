use std::assert_matches;

use super::support::*;
use super::*;

/// Within a few seconds the repository takes another maintenance
/// operation: whatever ran on it has ended, or was never recorded.
async fn assert_the_repository_takes_another_check(runner: &PlanRunner, what: &str) {
    let now = domain::Timestamp::from_second(1_800_000_100).expect("a timestamp");
    let mut check = domain::execution::Operation::queue(
        OperationId::from_uuid(Uuid::now_v7()),
        OperationKind::Check,
        Trigger::Manual,
        domain::execution::Target {
            plan: None,
            repository: Some(demo_repository()),
        },
        now,
    );
    check.start(now).expect("a queued operation starts");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match runner.store.try_start(&check).await {
            Ok(app::ports::Start::Started) => return,
            Ok(app::ports::Start::Refused { running }) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "{what}: the repository is still held by {}, which nothing ends",
                    running.as_uuid().hyphenated()
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(error) => panic!("{what}: the log refused the check's start: {error}"),
        }
    }
}

/// Where a test cuts a maintenance start short, each reached whatever the
/// threads do. The route's own future waits at two points, its reads and
/// the one-shot channel the task that starts the run answers on; the cut
/// at the channel is made twice, before the log has committed the start
/// and after.
#[derive(Clone, Copy, Debug)]
enum StartCut {
    /// While it reads the repository, before anything is spawned or
    /// stored: the reads take no slot.
    DuringItsReads,
    /// Once the start has reached the operation log, whose write of it
    /// can't commit: another connection holds the database's write lock.
    BeforeTheLogStoresIt,
    /// After the log has committed the start, before the route has heard
    /// so.
    AfterTheLogStoredIt,
}

impl std::fmt::Display for StartCut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DuringItsReads => "during its reads",
            Self::BeforeTheLogStoresIt => "before the log stored it",
            Self::AfterTheLogStoredIt => "after the log stored it",
        })
    }
}

/// An engine that says when a read began and never answers it.
#[derive(Default)]
struct HeldRead {
    reading: tokio::sync::Notify,
}

impl HeldRead {
    /// Waits until a read has begun.
    async fn read_begun(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.reading.notified())
            .await
            .expect("the start reads the repository");
    }
}

#[async_trait]
impl Engine for HeldRead {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        self.reading.notify_one();
        futures::future::pending().await
    }
}

/// Starts a check of the demo repository on `runner`, whose log is the
/// database at `database`, and drops the route at `cut`, as a client
/// that hangs up drops it. A route that answers first fails the test, as
/// does a start cut at the channel that the log then never stored: the
/// route took it with it.
async fn cut_a_start_short(runner: &mut PlanRunner, database: &std::path::Path, cut: StartCut) {
    use sqlx::{ConnectOptions, Connection};
    let check = || http_adapter::MaintenanceRequest::Check;
    match cut {
        StartCut::DuringItsReads => {
            let engine = Arc::new(HeldRead::default());
            runner.engine = engine.clone();
            let route = runner.start_maintenance("demo".into(), check(), None);
            tokio::select! {
                answered = route => panic!("the route answered during its reads: {answered:?}"),
                () = engine.read_begun() => {}
            }
        }
        StartCut::BeforeTheLogStoresIt => {
            runner.engine = Arc::new(Completes);
            let log = Arc::new(HeldAnswer::over(Arc::clone(&runner.store)));
            log.let_the_answer_through();
            runner.store = log.clone();
            let mut lock = sqlx::sqlite::SqliteConnectOptions::new()
                .filename(database)
                .connect()
                .await
                .expect("a connection of the test's own to the log");
            sqlx::query("BEGIN IMMEDIATE")
                .execute(&mut lock)
                .await
                .expect("the test takes the write lock");
            let route = runner.start_maintenance("demo".into(), check(), None);
            tokio::select! {
                answered = route => panic!("the route answered before the log stored the start: {answered:?}"),
                () = log.start_reached() => {}
            }
            sqlx::query("ROLLBACK")
                .execute(&mut lock)
                .await
                .expect("the test gives the write lock back");
            lock.close().await.expect("the test's connection closes");
            // The start goes on without the route: the log stores it
            // before anything else asks for the repository.
            tokio::time::timeout(std::time::Duration::from_secs(10), log.stored.notified())
                .await
                .expect("the log never answered the start: dropping the route cut it");
            log.assert_it_started();
        }
        StartCut::AfterTheLogStoredIt => {
            runner.engine = Arc::new(Completes);
            let log = Arc::new(HeldAnswer::over(Arc::clone(&runner.store)));
            runner.store = log.clone();
            let route = runner.start_maintenance("demo".into(), check(), None);
            tokio::select! {
                answered = route => panic!("the route answered before the log did: {answered:?}"),
                () = log.start_stored() => {}
            }
            log.assert_it_started();
            log.let_the_answer_through();
        }
    }
}

/// A client that hangs up in the middle of its request drops the handler
/// wherever it is waiting. A maintenance start dropped there must not
/// leave the repository held by a row nothing ends (I7): once SQLite has
/// committed the start, only the run itself ends it, and a handler that
/// was dropped before it learned of the commit has no one left to run it
/// (#135). Each [`StartCut`] lands at its point whatever the threads do,
/// the start cut at the channel is stored and then ended by its run, and
/// the start that nothing cuts is answered with its run.
///
/// This proves the daemon's level: wherever the route is dropped, the
/// start is left whole or never made. The cuts are the route's await
/// points as it has them; one it gains needs a cut of its own. That the
/// log's own call, cut after its commit, leaves the repository held,
/// which is why the start runs on a task the route can't drop, the store
/// proves at its level
/// (`pool_tests::a_start_cut_after_its_commit_holds_the_repository`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_maintenance_start_cut_short_at_any_await_point_leaves_the_repository_free() {
    for cut in [
        StartCut::DuringItsReads,
        StartCut::BeforeTheLogStoresIt,
        StartCut::AfterTheLogStoredIt,
    ] {
        let (mut runner, _entered, _gate, scratch) = runner_at_a_gate().await;
        let (_, data) = own_folders(scratch.path());
        cut_a_start_short(&mut runner, &data.join("ladderback.db"), cut).await;
        assert_the_repository_takes_another_check(&runner, &format!("after a start cut {cut}"))
            .await;
    }
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(Completes);
    let answer = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await;
    assert!(
        answer.is_ok(),
        "the start that nothing cut was refused: {answer:?}"
    );
    assert_the_repository_takes_another_check(&runner, "after a start nothing cut").await;
}

/// An operation log that says when a start reached it, stores the start
/// and then keeps its answer until the test lets it through: the moment a
/// client that hangs up cuts the route, after the log has the start and
/// before the route has learned so. It keeps what it decided for the
/// first start.
struct HeldAnswer {
    inner: Arc<dyn OperationStore>,
    reached: tokio::sync::Notify,
    stored: tokio::sync::Notify,
    first: std::sync::OnceLock<Result<app::ports::Start, app::ports::StoreError>>,
    open: tokio::sync::watch::Sender<bool>,
}

impl HeldAnswer {
    fn over(inner: Arc<dyn OperationStore>) -> Self {
        Self {
            inner,
            reached: tokio::sync::Notify::new(),
            stored: tokio::sync::Notify::new(),
            first: std::sync::OnceLock::new(),
            open: tokio::sync::watch::channel(false).0,
        }
    }

    /// Waits until a start has reached the log, which may not have
    /// stored it yet.
    async fn start_reached(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.reached.notified())
            .await
            .expect("the start reached the log");
    }

    /// Waits until the log has stored a start.
    async fn start_stored(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.stored.notified())
            .await
            .expect("the log stored the start");
    }

    /// The log took the first start, as running: it holds the
    /// repository.
    fn assert_it_started(&self) {
        assert_eq!(
            self.first.get(),
            Some(&Ok(app::ports::Start::Started)),
            "the log took the start"
        );
    }

    fn let_the_answer_through(&self) {
        self.open.send_replace(true);
    }
}

#[async_trait]
impl OperationStore for HeldAnswer {
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
        self.inner.save(operation, result).await
    }

    async fn try_start(
        &self,
        operation: &domain::execution::Operation,
    ) -> Result<app::ports::Start, app::ports::StoreError> {
        self.reached.notify_one();
        let decided = self.inner.try_start(operation).await;
        let _ = self.first.set(decided.clone());
        self.stored.notify_one();
        let _ = self.open.subscribe().wait_for(|open| *open).await;
        decided
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

/// How operations have ended as far as the page has heard, once the task
/// that runs them is done. That task holds a clone of the cancel registry
/// until it has told the pages and the log how the run ended, so all the
/// news is in when the clone is dropped.
async fn ends_heard_when_done(runner: &PlanRunner, heard: &mut Heard) -> Vec<UiEvent> {
    let done = async {
        while Arc::strong_count(&runner.registry) > 1 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), done)
        .await
        .expect("the run is done");
    ends_heard(heard)
}

/// The daemon said once, at info level, that the run of `operation`
/// ended, and how. The run is done, so the line is in the log already.
fn assert_said_once_that_it_ended(operation: &str, how: &str) {
    let said: Vec<String> = captured_log::infos()
        .into_iter()
        .filter(|line| line.contains(operation) && line.contains(" ended: "))
        .collect();
    let [line] = said.as_slice() else {
        panic!("one line says the run ended: {said:?}");
    };
    assert!(line.ends_with(how), "{line}");
}

/// A maintenance run that ends is told to the pages once, by the run,
/// and said once in the daemon's log, by the task that ran it, however
/// fast the route answered.
#[tokio::test]
async fn a_maintenance_run_that_ends_is_announced_once_and_logged_once() {
    captured_log::capture();
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let log = Arc::clone(&runner.store);
    let mut heard = listen(&mut runner, Completes, log).await;
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts");

    let ends = ends_heard_when_done(&runner, &mut heard).await;
    assert_matches!(
        ends.as_slice(),
        [UiEvent::OperationSucceeded { operation, .. }] if *operation == id,
        "the page hears one end, the run's: {ends:?}"
    );
    assert_said_once_that_it_ended(&id, "Succeeded");
}

/// The same for a backup, which the daemon runs on a task of its own too.
#[tokio::test]
async fn a_backup_that_ends_is_announced_once_and_logged_once() {
    captured_log::capture();
    let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
    open_hooks.send(true).expect("the gate opens");
    let log = Arc::clone(&runner.store);
    let mut heard = listen(&mut runner, Completes, log).await;
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");

    let ends = ends_heard_when_done(&runner, &mut heard).await;
    assert_matches!(
        ends.as_slice(),
        [UiEvent::OperationSucceeded { operation, .. }] if *operation == id,
        "the page hears one end, the run's: {ends:?}"
    );
    assert_said_once_that_it_ended(&id, "Succeeded");
}

/// A maintenance run cancelled after its start is announced to no page
/// (the page that cancelled it reads the history until it settles), and
/// the daemon's log says once that it ended cancelled.
#[tokio::test]
async fn a_maintenance_run_canceled_after_its_start_is_announced_to_no_page_and_logged_once() {
    captured_log::capture();
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let log = Arc::clone(&runner.store);
    let mut heard = listen(&mut runner, IdentifiesThenParks, log).await;
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts");
    runner
        .cancel(id.clone(), http_adapter::Canceller::Session)
        .await
        .expect("a cancel straight after the answer is accepted");

    assert_eq!(
        ends_heard_when_done(&runner, &mut heard).await,
        [],
        "the page was told how a cancelled run ended"
    );
    let operation = operation_of(&id).expect("the answer is an operation ID");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
    assert_said_once_that_it_ended(&id, "Canceled");
}

/// A maintenance start refused because a run holds the repository began
/// no operation, so no page is told how one ended; the run that holds
/// the repository goes on undisturbed, until its cancel.
#[tokio::test]
async fn a_maintenance_start_refused_for_a_busy_repository_is_announced_to_no_page() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let log = Arc::clone(&runner.store);
    let mut heard = listen(&mut runner, IdentifiesThenParks, log).await;
    let running = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the first check starts");
    let refused = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await;
    assert_eq!(
        refused,
        Err(MaintenanceRefusal::Busy {
            running: running.clone()
        })
    );
    assert_eq!(
        ends_heard(&mut heard),
        [],
        "the refused start was announced, or the run it was refused for ended"
    );
    runner
        .cancel(running, http_adapter::Canceller::Session)
        .await
        .expect("the run that holds the repository is cancelled");

    assert_eq!(
        ends_heard_when_done(&runner, &mut heard).await,
        [],
        "the page was told how a run ended: the refused one began none, and the one cancelled is no news"
    );
}

/// A client that hangs up after the log has stored its maintenance start
/// has no ID to ask for the run by, but the run goes on, and the pages
/// are told how it ended, once, and the log says so: the task that
/// started it ends it, not the route that was dropped.
#[tokio::test]
async fn a_maintenance_run_whose_client_hung_up_is_still_announced_once_and_logged_once() {
    captured_log::capture();
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let log = Arc::new(HeldAnswer::over(Arc::clone(&runner.store)));
    let mut heard = listen(&mut runner, Completes, log.clone()).await;
    let route =
        runner.start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None);
    tokio::select! {
        answered = route => panic!("the route answered before the log did: {answered:?}"),
        () = log.start_stored() => {}
    }
    log.let_the_answer_through();

    let ends = ends_heard_when_done(&runner, &mut heard).await;
    let [UiEvent::OperationSucceeded { operation, .. }] = ends.as_slice() else {
        panic!("the page hears how the run ended, once: {ends:?}");
    };
    assert_eq!(
        history_of(&runner, operation).await.record.status,
        OperationStatus::Succeeded
    );
    assert_said_once_that_it_ended(operation, "Succeeded");
}
