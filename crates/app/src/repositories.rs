//! Repository use cases (technical design §5, components.md): initialising a
//! repository where nothing is yet, testing one that is there, and connecting
//! one by saving it into the configuration once its test passes. Each one
//! resolves the repository's secrets first, so no resolved value is kept past
//! the job it was resolved for.

use std::sync::Arc;

use domain::execution::FailureReason;
use domain::execution::deadline::{Deadline, REPOSITORY_JOB_BUDGET};
use domain::planning::{AgentConfig, Maintenance, RepositoryConfig, RepositorySettings};
use domain::{OperationId, RepositoryAlias, RepositoryId};
use futures::{FutureExt, StreamExt, pin_mut, select_biased};

use crate::ports::{
    Clock, ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, Problem,
    RepositoryAccess, RunningJob,
};
use crate::secrets::{ResolveError, ResolveRepository};

/// Initialises a repository: resolves the settings, then runs a `Job::Init`
/// against the location. It initialises with default key options, both halves
/// of a hot/cold repository, and refuses when a repository already exists
/// there, with [`RepositoryError::Exists`]: it never overwrites one.
///
/// When `alias` is given, the repository is saved into the configuration
/// after the initialisation succeeded, as [`ConnectRepository`](crate::repositories::ConnectRepository)
/// saves one.
pub struct InitRepository {
    clock: Arc<dyn Clock>,
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
    config: Arc<dyn ConfigStore>,
}

/// What an initialisation gave: the new repository's ID, and whether the
/// repository was saved into the configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitOutcome {
    pub id: RepositoryId,
    pub saved: bool,
}

/// Tests a repository: resolves the settings, opens the repository with a
/// `Job::Identify`, and returns its ID. It saves nothing, and a write-permission
/// check is not yet built.
pub struct TestRepository {
    clock: Arc<dyn Clock>,
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
}

/// Connects a repository: validates the alias, resolves the settings, tests
/// the repository, and only once the test passed saves it into the
/// configuration through the [`ConfigStore`]. A second connect with an alias
/// the configuration already holds is refused with
/// [`RepositoryError::AliasTaken`]. The settings as written, references
/// included, are what is saved.
pub struct ConnectRepository {
    clock: Arc<dyn Clock>,
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
    config: Arc<dyn ConfigStore>,
}

/// Why initialising, testing or connecting a repository failed. A typed code,
/// never a sentence (technical design §8.4).
#[derive(Debug, PartialEq)]
pub enum RepositoryError {
    /// A repository already exists at the location, so it was not
    /// initialised, and nothing there was changed.
    Exists,
    /// The configuration already holds a repository under the alias.
    AliasTaken(RepositoryAlias),
    /// The repository's settings couldn't be resolved.
    Resolve(ResolveError),
    /// The engine couldn't be started or refused the job.
    Engine(EngineError),
    /// The engine ran the job and it failed: a wrong password, a location
    /// with no repository, and so on.
    Failed(FailureReason),
    /// The configuration couldn't be read or written.
    Config(ConfigError),
    /// The repository didn't answer within its time, so the worker was
    /// stopped and nothing was saved.
    TimedOut,
}

// The error of these use cases is a typed code for the caller to match; it
// is not boxed to satisfy a size rule, for none of its variants is large on
// its own.
#[allow(clippy::result_large_err)]
impl InitRepository {
    pub fn new(
        clock: Arc<dyn Clock>,
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
    ) -> Self {
        Self {
            clock,
            engine,
            resolve,
            config,
        }
    }

    /// Initialises the repository the settings describe, and reports its ID.
    /// Saved only when `alias` is given, by the same rule
    /// [`ConnectRepository`] saves one: an alias the configuration already
    /// holds is refused, and before the job runs, so a refused request has
    /// created no repository the person could not then find again.
    pub async fn init(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
        alias: Option<RepositoryAlias>,
    ) -> Result<InitOutcome, RepositoryError> {
        if let Some(alias) = &alias {
            unused(&*self.config, alias).await?;
        }
        let access = access(&self.resolve, settings).await?;
        let id = run(&*self.clock, &*self.engine, op, Job::Init(access)).await?;
        let saved = match alias {
            Some(alias) => {
                save(&*self.config, &alias, settings).await?;
                true
            }
            None => false,
        };
        Ok(InitOutcome { id, saved })
    }
}

#[allow(clippy::result_large_err)]
impl TestRepository {
    pub fn new(clock: Arc<dyn Clock>, engine: Arc<dyn Engine>, resolve: ResolveRepository) -> Self {
        Self {
            clock,
            engine,
            resolve,
        }
    }

    /// Opens the repository the settings describe and reports its ID. Saves
    /// nothing.
    pub async fn test(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
    ) -> Result<RepositoryId, RepositoryError> {
        let access = access(&self.resolve, settings).await?;
        run(&*self.clock, &*self.engine, op, Job::Identify(access)).await
    }
}

#[allow(clippy::result_large_err)]
impl ConnectRepository {
    pub fn new(
        clock: Arc<dyn Clock>,
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
    ) -> Self {
        Self {
            clock,
            engine,
            resolve,
            config,
        }
    }

    /// Tests the repository and, once the test passed, saves it under `alias`
    /// with `maintenance`, the person's choice of who maintains it (§7.3).
    pub async fn connect(
        &self,
        op: OperationId,
        alias: RepositoryAlias,
        settings: &RepositorySettings,
        maintenance: Maintenance,
    ) -> Result<RepositoryId, RepositoryError> {
        let _ = maintenance;
        let access = access(&self.resolve, settings).await?;
        let id = run(&*self.clock, &*self.engine, op, Job::Identify(access)).await?;
        save(&*self.config, &alias, settings).await?;
        Ok(id)
    }
}

/// The settings with every reference resolved, ready for a job. The resolved
#[allow(clippy::result_large_err)]
async fn access(
    resolve: &ResolveRepository,
    settings: &RepositorySettings,
) -> Result<RepositoryAccess, RepositoryError> {
    Ok(resolve
        .resolve(settings)
        .await
        .map_err(RepositoryError::Resolve)?
        .access)
}

/// Runs a repository job to its end and reports the repository's ID. Only the
/// result and a failure matter here; progress and warnings belong to the
/// operation log, which these requests don't write.
///
/// The job has [`REPOSITORY_JOB_BUDGET`] from now, engine start included. A
/// repository that hasn't answered by then is refused with
/// [`RepositoryError::TimedOut`], and dropping the job kills its worker at
/// once (no grace: there is nothing in it to finish). Its caller holds the
/// configuration lock across this call, so the bound is also how long a
/// repository that never answers can hold up every other save.
#[allow(clippy::result_large_err)]
async fn run(
    clock: &dyn Clock,
    engine: &dyn Engine,
    op: OperationId,
    job: Job,
) -> Result<RepositoryId, RepositoryError> {
    // A deadline counted from now: these jobs read no index, and the rule is
    // the one a backup's deadline is made with.
    let deadline = Deadline::after_index_read(clock.now(), REPOSITORY_JOB_BUDGET);
    let mut timer = clock.sleep_until(deadline.at()).fuse();
    let job = drive(clock, deadline, engine, op, job).fuse();
    pin_mut!(job);
    select_biased! {
        done = job => done,
        () = timer => Err(RepositoryError::TimedOut),
    }
}

/// How many events that are already waiting are read past the deadline in
/// search of a result.
const READY_EVENTS: usize = 64;

/// Plays a job's events until its result. A result stands even when it
/// arrives as the time runs out: a repository that was created is not made
/// not created by a refusal. Any other event after the deadline is refused
/// here, on the event itself, so a worker that never stops talking can't keep
/// the timer from its turn; only the events already waiting behind it, up to
/// [`READY_EVENTS`], are read first, for the result may be one of them.
#[allow(clippy::result_large_err)]
async fn drive(
    clock: &dyn Clock,
    deadline: Deadline,
    engine: &dyn Engine,
    op: OperationId,
    job: Job,
) -> Result<RepositoryId, RepositoryError> {
    // The whole job is held until this returns: dropping it is what kills
    // the worker.
    let RunningJob {
        events,
        control: _running,
    } = engine.run(op, job).await.map_err(RepositoryError::Engine)?;
    let mut events = events.fuse();
    while let Some(event) = events.next().await {
        if let Some(done) = result_of(event) {
            return done;
        }
        if deadline.has_passed(clock.now()) {
            for _ in 0..READY_EVENTS {
                match events.next().now_or_never() {
                    Some(Some(event)) => {
                        if let Some(done) = result_of(event) {
                            return done;
                        }
                    }
                    _ => break,
                }
            }
            return Err(RepositoryError::TimedOut);
        }
    }
    // The stream ended without a result: the engine stopped without reporting.
    Err(RepositoryError::Failed(FailureReason::EngineExited {
        exit_code: None,
    }))
}

/// What an event ends a job with, if it ends it: the repository's ID or the
/// job's failure. Progress and warnings end nothing.
#[allow(clippy::result_large_err)]
fn result_of(event: EngineEvent) -> Option<Result<RepositoryId, RepositoryError>> {
    match event {
        EngineEvent::Succeeded(EngineResult::Repository { id }) => Some(Ok(id)),
        EngineEvent::Failed(reason) => Some(Err(failed(reason))),
        _ => None,
    }
}

/// A failed job, as these use cases report it: the one typed refusal is a
/// repository that already exists; the rest keep the engine's reason.
fn failed(reason: FailureReason) -> RepositoryError {
    match reason {
        FailureReason::RepositoryExists => RepositoryError::Exists,
        reason => RepositoryError::Failed(reason),
    }
}

/// The configuration as it stands, when it holds no repository under `alias`:
/// otherwise the alias is refused.
#[allow(clippy::result_large_err)]
async fn unused(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
) -> Result<AgentConfig, RepositoryError> {
    let loaded = config.load().await.map_err(RepositoryError::Config)?;
    if loaded.repository(alias).is_some() {
        return Err(RepositoryError::AliasTaken(alias.clone()));
    }
    Ok(loaded)
}

/// Saves `settings` under `alias`, refusing an alias the configuration
/// already holds. What is saved is the settings as written: a secret stays
#[allow(clippy::result_large_err)]
async fn save(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
) -> Result<(), RepositoryError> {
    let loaded = unused(config, alias).await?;
    let mut repositories = loaded.repositories().clone();
    repositories.insert(
        alias.clone(),
        RepositoryConfig {
            settings: settings.clone(),
            maintenance: Default::default(),
            group_by: Default::default(),
            hooks: Default::default(),
        },
    );
    // Adding a repository can't break a rule that reaches across the parts
    // (they are about plans, which don't change), so this refusal is not one
    // the caller can reach; it is handled all the same, for nothing here
    // panics.
    let saved = AgentConfig::new(
        loaded.agent().clone(),
        loaded.hooks().clone(),
        repositories,
        loaded.plans().to_vec(),
    )
    .map_err(unwritable)?;
    config.save(&saved).await.map_err(RepositoryError::Config)
}

/// A configuration that can't be, as a configuration error: only reachable
/// through a violation that [`AgentConfig::new`] reports.
fn unwritable(violation: domain::planning::Violation) -> RepositoryError {
    RepositoryError::Config(ConfigError::Invalid {
        file: String::new(),
        key: None,
        at: None,
        problem: Problem::Violation(violation),
        detail: String::new(),
    })
}

#[cfg(all(test, feature = "fakes"))]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use async_trait::async_trait;
    use domain::SignedDuration;
    use domain::Timestamp;
    use domain::execution::FailureReason;
    use domain::execution::deadline::REPOSITORY_JOB_BUDGET;
    use domain::planning::Maintenance;
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, Uuid};
    use futures::StreamExt;

    use super::{ConnectRepository, InitRepository, RepositoryError, TestRepository};
    use crate::fakes::{FakeClock, FakeSecrets, InMemoryConfigStore};
    use crate::ports::ConfigStore;
    use crate::ports::{
        Clock, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RunningJob,
    };
    use crate::secrets::ResolveRepository;

    /// An engine that answers every repository job with the scripted outcome,
    /// and records the jobs it was given.
    struct Scripted {
        outcome: Mutex<Result<RepositoryId, FailureReason>>,
        seen: Mutex<Vec<Job>>,
    }

    struct Stop;

    #[async_trait]
    impl JobControl for Stop {
        async fn stop(&mut self) {}
    }

    #[async_trait]
    impl Engine for Scripted {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            let id = match self
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
            {
                Ok(id) => id,
                Err(reason) => {
                    return Ok(RunningJob {
                        events: Box::pin(futures::stream::iter([EngineEvent::Failed(reason)])),
                        control: Box::new(Stop),
                    });
                }
            };
            Ok(RunningJob {
                events: Box::pin(futures::stream::iter([EngineEvent::Succeeded(
                    crate::ports::EngineResult::Repository { id },
                )])),
                control: Box::new(Stop),
            })
        }
    }

    /// Where every test's clock starts.
    const START: Timestamp = Timestamp::constant(1_800_000_000, 0);

    /// A clock that jumps to the deadline of whoever sleeps on it.
    fn clock() -> Arc<dyn Clock> {
        Arc::new(FakeClock::auto_advancing(START))
    }

    /// A control that notes when its worker is dropped, which kills it.
    struct Killable(Arc<AtomicBool>);

    #[async_trait]
    impl JobControl for Killable {
        async fn stop(&mut self) {}
    }

    impl Drop for Killable {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// An engine whose jobs play `events`, one after another and then wait
    /// for ever, as a backend that stopped answering does.
    struct Playing {
        events: Mutex<Vec<EngineEvent>>,
        /// Called as each event is taken, so a test can move time with it.
        on_event: Arc<dyn Fn() + Send + Sync>,
        killed: Arc<AtomicBool>,
    }

    impl Playing {
        fn new(events: Vec<EngineEvent>, on_event: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(events),
                on_event: Arc::new(on_event),
                killed: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    #[async_trait]
    impl Engine for Playing {
        async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
            let events =
                std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner));
            let on_event = self.on_event.clone();
            Ok(RunningJob {
                events: futures::stream::iter(events)
                    .map(move |event| {
                        on_event();
                        event
                    })
                    .chain(futures::stream::pending())
                    .boxed(),
                control: Box::new(Killable(self.killed.clone())),
            })
        }
    }

    fn started() -> EngineEvent {
        EngineEvent::Started {
            index_read_at: START,
        }
    }

    fn found() -> EngineEvent {
        EngineEvent::Succeeded(EngineResult::Repository { id: id() })
    }

    /// Runs `work` against a manual clock, moving time by `steps` once the
    /// work waits on it. Gives, after each step, whether the work had ended
    /// and what `watch` said, and then what the work ended with.
    #[allow(
        clippy::panic,
        reason = "a helper outside a test body is production code to clippy"
    )]
    async fn timed<T>(
        clock: &FakeClock,
        watch: &AtomicBool,
        work: impl std::future::Future<Output = T>,
        steps: &[SignedDuration],
    ) -> (Vec<(bool, bool)>, T) {
        let finished = AtomicBool::new(false);
        let work = async {
            let done = work.await;
            finished.store(true, Ordering::SeqCst);
            done
        };
        let driver = async {
            let mut seen = Vec::new();
            for step in steps {
                // Time moves once the work waits on it, never before: a
                // deadline that isn't registered yet would be skipped.
                while clock.sleepers() == 0 {
                    tokio::task::yield_now().await;
                }
                clock.advance(*step);
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
                seen.push((
                    finished.load(Ordering::SeqCst),
                    watch.load(Ordering::SeqCst),
                ));
            }
            seen
        };
        let (done, seen) =
            tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(work, driver) })
                .await
                .unwrap_or_else(|_| panic!("the work never waited on the clock, or never ended"));
        (seen, done)
    }

    #[allow(
        clippy::panic,
        clippy::result_large_err,
        reason = "a helper outside a test body is production code to clippy; the error is the use cases' own"
    )]
    async fn within(
        work: impl std::future::Future<Output = Result<RepositoryId, RepositoryError>>,
    ) -> Result<RepositoryId, RepositoryError> {
        tokio::time::timeout(Duration::from_secs(5), work)
            .await
            .unwrap_or_else(|_| panic!("the use case waited for ever"))
    }

    #[tokio::test]
    async fn a_repository_that_never_answers_is_refused_the_moment_its_time_is_up() {
        let clock = Arc::new(FakeClock::new(START));
        let engine = Playing::new(Vec::new(), || {});
        let use_case = TestRepository::new(
            clock.clone(),
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        let one = SignedDuration::from_secs(1);
        let (finished, answer) = timed(
            &clock,
            &engine.killed,
            within(use_case.test(op(), &settings())),
            &[REPOSITORY_JOB_BUDGET - one, one],
        )
        .await;
        assert_eq!(
            finished,
            // (ended, worker killed) after each step
            [(false, false), (true, true)],
            "refused one second early, or kept waiting past the time, or the worker was \
             stopped at another moment"
        );
        assert_eq!(answer, Err(RepositoryError::TimedOut));
        assert!(
            engine.killed.load(Ordering::SeqCst),
            "the worker kept running"
        );
    }

    /// An engine whose worker never stops talking: every message is there
    /// before it is asked for, and takes a minute of the clock.
    struct Chatty {
        clock: Arc<FakeClock>,
        killed: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Engine for Chatty {
        async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
            let clock = self.clock.clone();
            Ok(RunningJob {
                events: futures::stream::repeat_with(move || {
                    clock.advance(SignedDuration::from_mins(1));
                    started()
                })
                .boxed(),
                control: Box::new(Killable(self.killed.clone())),
            })
        }
    }

    #[tokio::test]
    async fn a_worker_that_keeps_talking_cannot_hold_off_the_deadline() {
        let clock = Arc::new(FakeClock::new(START));
        let engine = Arc::new(Chatty {
            clock: clock.clone(),
            killed: Arc::new(AtomicBool::new(false)),
        });
        let use_case = TestRepository::new(
            clock,
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        assert_eq!(
            within(use_case.test(op(), &settings())).await,
            Err(RepositoryError::TimedOut)
        );
        assert!(engine.killed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_result_waiting_behind_a_message_after_the_deadline_stands() {
        // Time ran out while the daemon was not looking: the worker's last
        // two messages are both waiting, and the second is the answer.
        let clock = Arc::new(FakeClock::new(START));
        let ticking = clock.clone();
        let engine = Playing::new(vec![started(), found()], move || {
            ticking.advance(REPOSITORY_JOB_BUDGET + SignedDuration::from_secs(1));
        });
        let use_case = TestRepository::new(
            clock,
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        assert_eq!(within(use_case.test(op(), &settings())).await, Ok(id()));
    }

    #[tokio::test]
    async fn a_result_that_arrives_as_time_runs_out_stands() {
        // The repository was created: refusing now would say it was not.
        let clock = Arc::new(FakeClock::new(START));
        let ticking = clock.clone();
        let engine = Playing::new(vec![found()], move || {
            ticking.advance(REPOSITORY_JOB_BUDGET + SignedDuration::from_secs(1));
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = InitRepository::new(
            clock,
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        let outcome = use_case
            .init(op(), &settings(), Some(alias()))
            .await
            .unwrap_or_else(|e| panic!("the initialisation: {e:?}"));
        assert!(outcome.saved);
        assert_eq!(
            config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"))
                .repositories()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn a_connection_that_timed_out_saves_nothing() {
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = ConnectRepository::new(
            clock(),
            Playing::new(Vec::new(), || {}),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        assert_eq!(
            within(use_case.connect(op(), alias(), &settings(), Maintenance::None)).await,
            Err(RepositoryError::TimedOut)
        );
        assert!(
            config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"))
                .repositories()
                .is_empty()
        );
    }

    fn settings() -> domain::planning::RepositorySettings {
        domain::planning::RepositorySettings {
            repository: "/home/demo/scratch/repo".to_owned(),
            repo_hot: None,
            password: SecretRef::Inline(SecretValue::new("demo-password")),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        }
    }

    fn op() -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(0x5eed))
    }

    // Fixed demo values; the `panic` they guard never happens, and a helper
    // outside a test body is production code to clippy.
    #[allow(clippy::panic)]
    fn alias() -> RepositoryAlias {
        RepositoryAlias::try_from("demo".to_owned())
            .unwrap_or_else(|e| panic!("a valid alias: {e:?}"))
    }

    #[allow(clippy::panic)]
    fn id() -> RepositoryId {
        RepositoryId::parse(&"a".repeat(64)).unwrap_or_else(|e| panic!("a repository ID: {e:?}"))
    }

    #[tokio::test]
    async fn testing_reports_the_id_and_saves_nothing() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = TestRepository::new(
            clock(),
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        assert_eq!(use_case.test(op(), &settings()).await, Ok(id()));
        {
            let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
            let Job::Identify(_) = &seen[0] else {
                panic!("testing runs Job::Identify, got {:?}", seen[0]);
            };
        }
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert!(saved.repositories().is_empty(), "testing saves nothing");
    }

    #[tokio::test]
    async fn connecting_saves_the_settings_only_once_the_test_passed() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = ConnectRepository::new(
            clock(),
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        assert_eq!(
            use_case
                .connect(op(), alias(), &settings(), Maintenance::None)
                .await,
            Ok(id())
        );
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(saved.repositories().len(), 1);
        assert_eq!(
            saved
                .repository(&alias())
                .unwrap_or_else(|| panic!("the repository"))
                .settings,
            settings()
        );
    }

    #[tokio::test]
    async fn a_failed_test_leaves_the_configuration_unchanged() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Err(FailureReason::EngineFailed {
                detail: "no repository there".to_owned(),
            })),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = ConnectRepository::new(
            clock(),
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        assert!(matches!(
            use_case
                .connect(op(), alias(), &settings(), Maintenance::None)
                .await,
            Err(RepositoryError::Failed(FailureReason::EngineFailed { .. }))
        ));
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert!(saved.repositories().is_empty());
    }

    #[tokio::test]
    async fn an_alias_already_taken_is_refused() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = ConnectRepository::new(
            clock(),
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        use_case
            .connect(op(), alias(), &settings(), Maintenance::None)
            .await
            .unwrap_or_else(|e| panic!("the first connect: {e:?}"));
        assert!(matches!(
            use_case
                .connect(op(), alias(), &settings(), Maintenance::None)
                .await,
            Err(RepositoryError::AliasTaken(_))
        ));
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(saved.repositories().len(), 1);
    }

    #[tokio::test]
    async fn initialising_saves_only_when_an_alias_is_given() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = InitRepository::new(
            clock(),
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        let outcome = use_case.init(op(), &settings(), None).await;
        assert!(
            !outcome
                .unwrap_or_else(|e| panic!("the initialisation: {e:?}"))
                .saved
        );
        use_case
            .init(op(), &settings(), Some(alias()))
            .await
            .unwrap_or_else(|e| panic!("the initialisation with an alias: {e:?}"));
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(saved.repositories().len(), 1);
    }

    #[tokio::test]
    async fn a_connection_saves_who_the_person_chose_to_maintain_it() {
        for chosen in [Maintenance::Local, Maintenance::None] {
            let engine = Arc::new(Scripted {
                outcome: Mutex::new(Ok(id())),
                seen: Mutex::new(Vec::new()),
            });
            let config = Arc::new(InMemoryConfigStore::new());
            ConnectRepository::new(
                clock(),
                engine,
                ResolveRepository::new(Arc::new(FakeSecrets::new())),
                config.clone(),
            )
            .connect(op(), alias(), &settings(), chosen)
            .await
            .unwrap_or_else(|e| panic!("the connection: {e:?}"));
            let saved = config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
            assert_eq!(
                saved.repository(&alias()).map(|r| r.maintenance),
                Some(chosen)
            );
        }
    }

    #[tokio::test]
    async fn a_repository_created_under_a_name_is_maintained_here() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        InitRepository::new(
            clock(),
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        )
        .init(op(), &settings(), Some(alias()))
        .await
        .unwrap_or_else(|e| panic!("the initialisation: {e:?}"));
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(
            saved.repository(&alias()).map(|r| r.maintenance),
            Some(Maintenance::Local)
        );
    }

    #[tokio::test]
    async fn initialising_under_an_alias_already_taken_is_refused_before_the_job_runs() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = InitRepository::new(
            clock(),
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        use_case
            .init(op(), &settings(), Some(alias()))
            .await
            .unwrap_or_else(|e| panic!("the first initialisation: {e:?}"));
        let jobs_before = engine
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        let mut elsewhere = settings();
        elsewhere.repository = "/home/demo/scratch/other".to_owned();
        assert!(matches!(
            use_case.init(op(), &elsewhere, Some(alias())).await,
            Err(RepositoryError::AliasTaken(_))
        ));
        assert_eq!(
            engine
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len(),
            jobs_before,
            "the refused initialisation ran a job, and so created a repository"
        );
        let saved = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(
            saved.repository(&alias()).map(|r| &r.settings),
            Some(&settings())
        );
    }

    #[tokio::test]
    async fn initialising_runs_job_init() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let use_case = InitRepository::new(
            clock(),
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            Arc::new(InMemoryConfigStore::new()),
        );
        use_case
            .init(op(), &settings(), None)
            .await
            .unwrap_or_else(|e| panic!("the initialisation: {e:?}"));
        {
            let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
            let Job::Init(_) = &seen[0] else {
                panic!("initialising runs Job::Init, got {:?}", seen[0]);
            };
        }
    }
}
