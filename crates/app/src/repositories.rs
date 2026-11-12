//! Repository use cases (technical design §5, components.md): initialising a
//! repository where nothing is yet, testing one that is there, and connecting
//! one by saving it into the configuration once its test passes. Each one
//! resolves the repository's secrets first, so no resolved value is kept past
//! the job it was resolved for.

mod create_offers;
mod record_opened;

pub use create_offers::CreateOffers;
pub use record_opened::{ConfigWrites, RecordOpened, Recording};

use std::sync::Arc;

use domain::execution::FailureReason;
use domain::execution::deadline::{Deadline, REPOSITORY_JOB_BUDGET};
use domain::planning::{AgentConfig, Maintenance, RepositoryConfig, RepositorySettings};
use domain::{OperationId, RepositoryAlias, RepositoryId};
use futures::{FutureExt, StreamExt, pin_mut, select_biased};

use crate::ports::{
    Clock, ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, Problem,
    RepositoryAccess, RunningJob, StoreError,
};
use crate::secrets::{ResolveError, ResolveRepository};

/// Initialises a repository: resolves the settings, then runs a `Job::Init`
/// against the location. It initialises with default key options, both halves
/// of a hot/cold repository, and refuses when a repository already exists
/// there, with [`RepositoryError::Exists`]: it never overwrites one.
///
/// When `alias` is given, the repository is saved into the configuration
/// after the initialisation succeeded, as [`ConnectRepository`](crate::repositories::ConnectRepository)
/// saves one. A repository the configuration already holds, such as one an
/// imported profile added, is initialised from its own entry by
/// [`InitRepository::init_saved`], which records only the new ID in it.
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

/// What initialising a saved repository tried: the settings of its entry,
/// which a refusal is redacted with, and how the initialisation went.
#[derive(Debug, PartialEq)]
pub struct SavedInit {
    pub settings: RepositorySettings,
    pub created: Result<RepositoryId, RepositoryError>,
}

/// Why a saved repository's entry couldn't be found, or whether it may be
/// created couldn't be told, so nothing ran. Every failure once both are
/// known is [`SavedInit::created`]'s, refused with the entry's settings.
#[derive(Debug, PartialEq)]
pub enum SavedEntryError {
    /// The configuration holds no repository under the alias.
    Unknown(RepositoryAlias),
    /// The configuration couldn't be read.
    Config(ConfigError),
    /// The operation log, which says whether the row offers Create,
    /// couldn't be read.
    Log(StoreError),
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
/// configuration through the [`ConfigStore`], with the repository ID the test
/// read. A second connect with an alias the configuration already holds is
/// refused with [`RepositoryError::AliasTaken`], and a repository the
/// configuration already holds under another alias, by its location or its
/// ID, with [`RepositoryError::AlreadyConfigured`] (§7.3). The settings as
/// written, references included, are what is saved.
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
    /// The repository's row doesn't offer to create it: its entry records
    /// no ID, and no job found the location empty since nothing reached it;
    /// or the engine, looking just before Create, found no folder with no
    /// repository in it where the entry says (decision 53). Nothing says
    /// the location is reachable and empty, so nothing was created.
    CreateNotOffered,
    /// The configuration already holds a repository under the alias.
    AliasTaken(RepositoryAlias),
    /// The configuration already holds this repository, under the alias
    /// given: a repository has one alias (§7.3).
    AlreadyConfigured(RepositoryAlias),
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
    /// [`ConnectRepository`] saves one, with the new repository's ID: an
    /// alias the configuration already holds is refused, and so is a location
    /// it already holds under another alias, both before the job runs, so a
    /// refused request has created no repository the person could not then
    /// find again. A repository created here asks this agent to maintain it
    /// (`maintenance = "local"`, §7.3): nothing else knows it yet, so no
    /// other party maintains it.
    pub async fn init(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
        alias: Option<RepositoryAlias>,
    ) -> Result<InitOutcome, RepositoryError> {
        if let Some(alias) = &alias {
            unused(&*self.config, alias, settings, None).await?;
        }
        let access = access(&self.resolve, settings).await?;
        let id = run(&*self.clock, &*self.engine, op, Job::Init(access)).await?;
        let saved = match alias {
            Some(alias) => {
                save(&*self.config, &alias, settings, &id, Maintenance::Local).await?;
                true
            }
            None => false,
        };
        Ok(InitOutcome { id, saved })
    }

    /// Initialises the repository the configuration holds under `alias`,
    /// where its entry says it is and with the entry's own references, so a
    /// password the configuration already names is never asked for again.
    /// Only the new repository's ID is recorded in the entry, as [`init`]
    /// records it; the rest stays as it was, `maintenance` included: who
    /// maintains a saved repository was decided when it was saved (§7.3,
    /// decision 53). An entry that records an ID names a repository
    /// Ladderback has opened, so it is refused with [`RepositoryError::Exists`];
    /// any other whose row `offers` doesn't offer Create, with
    /// [`RepositoryError::CreateNotOffered`]: the request may come from a page
    /// shown before the log changed, or from no page at all. Both are refused
    /// before anything runs. An alias the configuration doesn't hold is
    /// refused with [`SavedEntryError::Unknown`], and a log that can't be
    /// read with [`SavedEntryError::Log`], before anything runs.
    ///
    /// [`init`]: Self::init
    pub async fn init_saved(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        offers: &CreateOffers,
    ) -> Result<SavedInit, SavedEntryError> {
        let loaded = self.config.load().await.map_err(SavedEntryError::Config)?;
        let Some(entry) = loaded.repository(alias) else {
            return Err(SavedEntryError::Unknown(alias.clone()));
        };
        let settings = entry.settings.clone();
        let refused = if entry.id.is_some() {
            Some(RepositoryError::Exists)
        } else if offers
            .offers(&loaded, alias)
            .await
            .map_err(SavedEntryError::Log)?
        {
            None
        } else {
            Some(RepositoryError::CreateNotOffered)
        };
        if let Some(refused) = refused {
            return Ok(SavedInit {
                settings,
                created: Err(refused),
            });
        }
        let created = async {
            let access = access(&self.resolve, &settings).await?;
            found_empty(&*self.clock, &*self.engine, op, &access).await?;
            let id = run(&*self.clock, &*self.engine, op, Job::Init(access)).await?;
            record(&*self.config, alias, &settings, &id).await?;
            Ok(id)
        }
        .await;
        Ok(SavedInit { settings, created })
    }
}

/// Whether the engine, looking at the location now, finds a folder there
/// holding nothing of a repository: the only place Create may make one. The
/// row's offer comes from the log, which doesn't say where its jobs ran, so
/// the entry may since name another location, or the drive be unplugged; an
/// initialisation there would make the folder (decision 53). A repository
/// that opens is refused as one already there, and any other failure to find
/// one as not offered; a refusal of the look itself (a timeout, an engine
/// that didn't start) is its own.
#[allow(clippy::result_large_err)]
async fn found_empty(
    clock: &dyn Clock,
    engine: &dyn Engine,
    op: OperationId,
    access: &RepositoryAccess,
) -> Result<(), RepositoryError> {
    match run(clock, engine, op, Job::Identify(access.clone())).await {
        Err(RepositoryError::Failed(FailureReason::RepositoryNotFound)) => Ok(()),
        Ok(_) => Err(RepositoryError::Exists),
        Err(RepositoryError::Failed(_)) => Err(RepositoryError::CreateNotOffered),
        Err(other) => Err(other),
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
    /// with the ID the test read and with `maintenance`, the person's choice
    /// of who maintains it (§7.3): a repository that exists may already be
    /// maintained by another party, and only one may do it (I1).
    pub async fn connect(
        &self,
        op: OperationId,
        alias: RepositoryAlias,
        settings: &RepositorySettings,
        maintenance: Maintenance,
    ) -> Result<RepositoryId, RepositoryError> {
        let access = access(&self.resolve, settings).await?;
        let id = run(&*self.clock, &*self.engine, op, Job::Identify(access)).await?;
        save(&*self.config, &alias, settings, &id, maintenance).await?;
        Ok(id)
    }
}

/// The settings with every reference resolved, ready for a job. The resolver's
/// warnings about how each secret was read are dropped: like progress, they
/// belong to the operation log, which these requests don't write (see `run`).
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

/// The configuration as it stands, when it holds no repository under `alias`
/// and holds the repository `settings` describe, with the ID `id` when it is
/// known, under no alias ([`AgentConfig::alias_of`]): a taken alias is
/// refused first, then a repository already configured, which has one alias
/// (§7.3).
#[allow(clippy::result_large_err)]
async fn unused(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    id: Option<&RepositoryId>,
) -> Result<AgentConfig, RepositoryError> {
    let loaded = config.load().await.map_err(RepositoryError::Config)?;
    if loaded.repository(alias).is_some() {
        return Err(RepositoryError::AliasTaken(alias.clone()));
    }
    if let Some(configured) = loaded.alias_of(settings, id) {
        return Err(RepositoryError::AlreadyConfigured(configured.clone()));
    }
    Ok(loaded)
}

/// Saves `settings` under `alias`, with the repository's `id` and with
/// `maintenance` as its request of who maintains it, refusing what [`unused`]
/// refuses. What is saved is the settings as written: a secret stays a
/// reference.
#[allow(clippy::result_large_err)]
async fn save(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    id: &RepositoryId,
    maintenance: Maintenance,
) -> Result<(), RepositoryError> {
    let loaded = unused(config, alias, settings, Some(id)).await?;
    let mut repositories = loaded.repositories().clone();
    repositories.insert(
        alias.clone(),
        RepositoryConfig {
            id: Some(id.clone()),
            settings: settings.clone(),
            maintenance,
            group_by: Default::default(),
            hooks: Default::default(),
        },
    );
    // [`unused`] has refused a second alias for the repository, and the
    // other rules are about plans, which don't change, so this refusal is not
    // one the caller can reach; it is handled all the same, for nothing here
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

/// Records `id` as the ID of the repository saved under `alias`, the rest of
/// the configuration as it is now, while that entry still names the location
/// `settings` describe, the one the repository was created at. An entry
/// removed or moved meanwhile, by a hand edit or the CLI's import, which
/// don't take the daemon's lock, records nothing: the ID is not its
/// repository's (decision 53).
#[allow(clippy::result_large_err)]
async fn record(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    id: &RepositoryId,
) -> Result<(), RepositoryError> {
    let loaded = config.load().await.map_err(RepositoryError::Config)?;
    let mut repositories = loaded.repositories().clone();
    match repositories.get_mut(alias) {
        Some(entry) if entry.settings.same_location(settings) => entry.id = Some(id.clone()),
        _ => return Ok(()),
    }
    let recorded = AgentConfig::new(
        loaded.agent().clone(),
        loaded.hooks().clone(),
        repositories,
        loaded.plans().to_vec(),
    )
    .map_err(unwritable)?;
    config
        .save(&recorded)
        .await
        .map_err(RepositoryError::Config)
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

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use async_trait::async_trait;
    use domain::SignedDuration;
    use domain::Timestamp;
    use domain::execution::deadline::REPOSITORY_JOB_BUDGET;
    use domain::execution::{FailureReason, Operation, OperationKind, Target, Trigger};
    use domain::planning::Maintenance;
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, Uuid};
    use futures::StreamExt;

    use super::{ConnectRepository, CreateOffers, InitRepository, RepositoryError, TestRepository};
    use crate::fakes::{FakeClock, FakeSecrets, InMemoryConfigStore, InMemoryOperationStore};
    use crate::ports::{
        Clock, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RunningJob,
    };
    use crate::ports::{ConfigStore, OperationStore};
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
            let outcome = self
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            Ok(ended(outcome))
        }
    }

    /// A job that ends at once with `outcome`: the repository's ID, or the
    /// failure.
    fn ended(outcome: Result<RepositoryId, FailureReason>) -> RunningJob {
        let event = match outcome {
            Ok(id) => EngineEvent::Succeeded(crate::ports::EngineResult::Repository { id }),
            Err(reason) => EngineEvent::Failed(reason),
        };
        RunningJob {
            events: Box::pin(futures::stream::iter([event])),
            control: Box::new(Stop),
        }
    }

    /// An engine that answers a `Job::Identify` with `found`, what it finds
    /// at the location now, and every other job as `job` does, keeping every
    /// job it was given in `job.seen`.
    struct Located {
        found: Result<RepositoryId, FailureReason>,
        job: Scripted,
    }

    #[async_trait]
    impl Engine for Located {
        async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            if !matches!(job, Job::Identify(_)) {
                return self.job.run(op, job).await;
            }
            self.job
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            Ok(ended(self.found.clone()))
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
            repository: None,
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

    #[test]
    fn a_worker_that_keeps_talking_cannot_hold_off_the_deadline() {
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
        // The worker's messages are always ready, so a use case that never
        // sees its time run out loops without handing its thread back, and a
        // timeout on that thread never gets its turn. The use case runs on a
        // thread of its own instead, and this one waits for it, bounded; a
        // thread left looping ends with the test process.
        let (tell, told) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("a runtime");
            let _ = tell.send(runtime.block_on(use_case.test(op(), &settings())));
        });
        assert_eq!(
            told.recv_timeout(Duration::from_secs(5))
                .expect("the use case waited for ever"),
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

    /// The plan that backs up to `alias()` in [`saved_entry`]'s
    /// configuration.
    #[allow(clippy::panic)]
    fn home() -> domain::PlanId {
        domain::PlanId::parse("home").unwrap_or_else(|e| panic!("a plan ID: {e:?}"))
    }

    /// A configuration holding `settings` under `alias()`, as an imported
    /// profile leaves it: with `recorded` as its repository ID, asking
    /// nobody to maintain it, and backed up to by the plan [`home`]; and the
    /// engine and the use case to initialise it with. The engine finds no
    /// repository at the location, and ends an initialisation with
    /// `outcome`.
    async fn saved_entry(
        settings: &domain::planning::RepositorySettings,
        recorded: Option<RepositoryId>,
        secrets: FakeSecrets,
        outcome: Result<RepositoryId, FailureReason>,
    ) -> (Arc<Located>, Arc<InMemoryConfigStore>, InitRepository) {
        saved_entry_finding(
            settings,
            recorded,
            secrets,
            Err(FailureReason::RepositoryNotFound),
            outcome,
        )
        .await
    }

    /// [`saved_entry`], with an engine that finds `found` at the location.
    #[allow(clippy::panic)]
    async fn saved_entry_finding(
        settings: &domain::planning::RepositorySettings,
        recorded: Option<RepositoryId>,
        secrets: FakeSecrets,
        found: Result<RepositoryId, FailureReason>,
        outcome: Result<RepositoryId, FailureReason>,
    ) -> (Arc<Located>, Arc<InMemoryConfigStore>, InitRepository) {
        let engine = Arc::new(Located {
            found,
            job: Scripted {
                outcome: Mutex::new(outcome),
                seen: Mutex::new(Vec::new()),
            },
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let entry = domain::planning::RepositoryConfig {
            settings: settings.clone(),
            id: recorded,
            maintenance: Maintenance::None,
            group_by: Default::default(),
            hooks: Default::default(),
        };
        let source = domain::planning::Source::parse("/home/demo")
            .unwrap_or_else(|e| panic!("a source: {e:?}"));
        let saved = domain::planning::AgentConfig::new(
            Default::default(),
            Default::default(),
            BTreeMap::from([(alias(), entry)]),
            vec![domain::planning::Plan::new(home(), alias(), vec![source])],
        )
        .unwrap_or_else(|e| panic!("a valid configuration: {e:?}"));
        config
            .save(&saved)
            .await
            .unwrap_or_else(|e| panic!("the entry is saved: {e:?}"));
        let resolve = ResolveRepository::new(Arc::new(secrets));
        let use_case = InitRepository::new(clock(), engine.clone(), resolve, config.clone());
        (engine, config, use_case)
    }

    /// The offers over an operation log holding a backup of [`home`] for
    /// each of `ends`, oldest first, each succeeded (`Ok`) or failed.
    #[allow(clippy::panic)]
    async fn log_of(ends: &[Result<(), FailureReason>]) -> CreateOffers {
        let store = Arc::new(InMemoryOperationStore::new());
        for (n, end) in (1u128..).zip(ends) {
            let mut backup = Operation::queue(
                OperationId::from_uuid(Uuid::from_u128(n)),
                OperationKind::Backup,
                Trigger::Manual,
                Target {
                    plan: Some(home()),
                    repository: None,
                },
                START,
            );
            backup
                .start(START)
                .and_then(|()| match end {
                    Ok(()) => backup.succeed(START),
                    Err(reason) => backup.fail(reason.clone(), START),
                })
                .unwrap_or_else(|e| panic!("{end:?}: {e:?}"));
            store
                .save(&backup, None)
                .await
                .unwrap_or_else(|e| panic!("the backup is recorded: {e:?}"));
        }
        CreateOffers::new(store)
    }

    /// The offers over a log whose only backup of [`home`] found no
    /// repository at its location: what makes its row offer Create
    /// (decision 53).
    async fn found_none() -> CreateOffers {
        log_of(&[Err(FailureReason::RepositoryNotFound)]).await
    }

    #[tokio::test]
    async fn a_saved_repository_is_initialised_from_its_entry_which_records_only_its_new_id() {
        let mut entry = settings();
        entry.repo_hot = Some("/home/demo/scratch/hot".to_owned());
        entry.password = SecretRef::Env("DEMO_PASSWORD".to_owned());
        let secrets = FakeSecrets::new();
        secrets.set_env("DEMO_PASSWORD", "from-the-environment");
        let (engine, config, use_case) = saved_entry(&entry, None, secrets, Ok(id())).await;
        let before = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));

        let tried = use_case
            .init_saved(op(), &alias(), &found_none().await)
            .await
            .unwrap_or_else(|e| panic!("the entry is found: {e:?}"));

        assert_eq!(
            tried,
            super::SavedInit {
                settings: entry.clone(),
                created: Ok(id()),
            }
        );
        {
            let seen = engine
                .job
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let [Job::Identify(looked), Job::Init(access)] = seen.as_slice() else {
                panic!("creating looks at the location, then runs one Job::Init, got {seen:?}");
            };
            for access in [looked, access] {
                assert_eq!(access.repository, entry.repository);
                assert_eq!(access.repo_hot, entry.repo_hot);
                assert_eq!(access.password.expose(), "from-the-environment");
            }
        }
        let after = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        let mut expected = before.repositories().clone();
        if let Some(saved) = expected.get_mut(&alias()) {
            saved.id = Some(id());
        }
        assert_eq!(
            after.repositories(),
            &expected,
            "only the new repository's ID is recorded, its maintenance unchanged"
        );
        assert_eq!(after.plans(), before.plans());
        assert_eq!(after.agent(), before.agent());
        assert_eq!(after.hooks(), before.hooks());
    }

    /// An entry that records a repository ID names a repository Ladderback
    /// has opened: it was there, so it is not created again, even once a job
    /// found none at its location, and nothing runs (decision 53).
    #[tokio::test]
    async fn a_saved_repository_whose_entry_records_an_id_is_refused_before_anything_runs() {
        let recorded = RepositoryId::parse(&"b".repeat(64))
            .unwrap_or_else(|e| panic!("a repository ID: {e:?}"));
        let (engine, config, use_case) =
            saved_entry(&settings(), Some(recorded), FakeSecrets::new(), Ok(id())).await;
        let before = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(
            use_case
                .init_saved(op(), &alias(), &found_none().await)
                .await,
            Ok(super::SavedInit {
                settings: settings(),
                created: Err(RepositoryError::Exists),
            })
        );
        assert!(
            engine
                .job
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty(),
            "a job ran for a repository already reached"
        );
        assert_eq!(
            config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}")),
            before
        );
    }

    /// A row that doesn't offer Create is refused it as not offered, not as
    /// one already there, whatever page the request came from, and nothing
    /// runs: nothing on record (a new import, or a drive that isn't plugged
    /// in), a backup that once succeeded, or a latest job that found the
    /// repository (decision 53).
    #[tokio::test]
    async fn a_saved_repository_whose_row_offers_no_create_is_refused_as_not_offered() {
        let histories: [&[Result<(), FailureReason>]; 3] = [
            &[],
            &[Ok(()), Err(FailureReason::RepositoryNotFound)],
            &[Err(FailureReason::RepositoryNotFound), Ok(())],
        ];
        for history in histories {
            let (engine, config, use_case) =
                saved_entry(&settings(), None, FakeSecrets::new(), Ok(id())).await;
            let before = config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
            assert_eq!(
                use_case
                    .init_saved(op(), &alias(), &log_of(history).await)
                    .await,
                Ok(super::SavedInit {
                    settings: settings(),
                    created: Err(RepositoryError::CreateNotOffered),
                }),
                "{history:?}"
            );
            assert!(
                engine
                    .job
                    .seen
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_empty(),
                "a job ran after {history:?}"
            );
            assert_eq!(
                config
                    .load()
                    .await
                    .unwrap_or_else(|e| panic!("the configuration loads: {e:?}")),
                before,
                "{history:?}"
            );
        }
    }

    /// An engine that, while it initialises, moves the entry saved under
    /// `alias()` to `to`, as a hand edit of the configuration or a second
    /// import may; each job itself is the scripted one.
    struct MovedMeanwhile {
        job: Located,
        config: Arc<InMemoryConfigStore>,
        to: domain::planning::RepositorySettings,
    }

    #[async_trait]
    impl Engine for MovedMeanwhile {
        #[allow(clippy::panic)]
        async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            let loaded = self
                .config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
            let mut repositories = loaded.repositories().clone();
            if let Some(entry) = repositories.get_mut(&alias()) {
                entry.settings = self.to.clone();
            }
            let moved = domain::planning::AgentConfig::new(
                loaded.agent().clone(),
                loaded.hooks().clone(),
                repositories,
                loaded.plans().to_vec(),
            )
            .unwrap_or_else(|e| panic!("a valid configuration: {e:?}"));
            if matches!(job, Job::Init(_)) {
                self.config
                    .save(&moved)
                    .await
                    .unwrap_or_else(|e| panic!("the moved entry is saved: {e:?}"));
            }
            self.job.run(op, job).await
        }
    }

    /// The ID belongs to the repository created where the entry was: an
    /// entry that names another location by the time the job ends is left
    /// without one, so it is not taken for that repository (decision 53).
    #[tokio::test]
    async fn a_saved_entry_moved_while_its_repository_is_created_records_no_id() {
        let (_, config, _) = saved_entry(&settings(), None, FakeSecrets::new(), Ok(id())).await;
        let mut elsewhere = settings();
        elsewhere.repository = "/home/demo/scratch/elsewhere".to_owned();
        let engine = Arc::new(MovedMeanwhile {
            job: Located {
                found: Err(FailureReason::RepositoryNotFound),
                job: Scripted {
                    outcome: Mutex::new(Ok(id())),
                    seen: Mutex::new(Vec::new()),
                },
            },
            config: config.clone(),
            to: elsewhere.clone(),
        });
        let use_case = InitRepository::new(
            clock(),
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );

        assert_eq!(
            use_case
                .init_saved(op(), &alias(), &found_none().await)
                .await,
            Ok(super::SavedInit {
                settings: settings(),
                created: Ok(id()),
            }),
            "the repository was created where the entry was"
        );
        let after = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        let entry = after
            .repository(&alias())
            .unwrap_or_else(|| panic!("the entry is still saved"));
        assert_eq!(entry.settings, elsewhere, "the move is kept");
        assert_eq!(entry.id, None, "an ID was recorded for another location");
    }

    #[tokio::test]
    async fn a_saved_repository_already_there_is_refused_with_its_entrys_settings() {
        let (_, config, use_case) = saved_entry(
            &settings(),
            None,
            FakeSecrets::new(),
            Err(FailureReason::RepositoryExists),
        )
        .await;
        let before = config
            .load()
            .await
            .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
        assert_eq!(
            use_case
                .init_saved(op(), &alias(), &found_none().await)
                .await,
            Ok(super::SavedInit {
                settings: settings(),
                created: Err(RepositoryError::Exists),
            })
        );
        assert_eq!(
            config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}")),
            before,
            "a refused creation records nothing"
        );
    }

    /// The row's offer is a hint from the log, which doesn't say where its
    /// jobs ran: a backup found no repository at one location, and the entry
    /// now names another, whose folder isn't there, as on a drive that isn't
    /// plugged in. The engine looks at the entry's location first, and
    /// anything but a folder that is there and holds nothing of a repository is
    /// refused before anything is created: no folder and nothing at all as
    /// not offered, a repository it opens as one already there (decision 53).
    #[tokio::test]
    async fn a_saved_repository_is_created_only_where_the_engine_finds_a_folder_with_no_repository()
    {
        let mut elsewhere = settings();
        elsewhere.repository = "/media/demo-drive/repo".to_owned();
        let cases = [
            (
                Err(FailureReason::EngineFailed {
                    detail: "the repository folder is not there".to_owned(),
                }),
                RepositoryError::CreateNotOffered,
            ),
            (Ok(id()), RepositoryError::Exists),
        ];
        for (found, refused) in cases {
            let said = format!("{found:?}");
            let (engine, config, use_case) =
                saved_entry_finding(&elsewhere, None, FakeSecrets::new(), found, Ok(id())).await;
            let before = config
                .load()
                .await
                .unwrap_or_else(|e| panic!("the configuration loads: {e:?}"));
            assert_eq!(
                use_case
                    .init_saved(op(), &alias(), &found_none().await)
                    .await,
                Ok(super::SavedInit {
                    settings: elsewhere.clone(),
                    created: Err(refused),
                }),
                "{said}"
            );
            {
                let seen = engine
                    .job
                    .seen
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let [Job::Identify(looked)] = seen.as_slice() else {
                    panic!("only a look at the location ran after {said}: {seen:?}");
                };
                assert_eq!(looked.repository, elsewhere.repository, "{said}");
            }
            assert_eq!(
                config
                    .load()
                    .await
                    .unwrap_or_else(|e| panic!("the configuration loads: {e:?}")),
                before,
                "{said}"
            );
        }
    }

    #[tokio::test]
    async fn a_repository_the_configuration_does_not_hold_is_refused_before_anything_runs() {
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
        assert_eq!(
            use_case
                .init_saved(op(), &alias(), &found_none().await)
                .await,
            Err(super::SavedEntryError::Unknown(alias()))
        );
        assert!(
            engine
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty(),
            "a job ran for a repository nobody saved"
        );
    }
}
