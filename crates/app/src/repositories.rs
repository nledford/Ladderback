//! Repository use cases (technical design §5, components.md): initialising a
//! repository where nothing is yet, testing one that is there, and connecting
//! one by saving it into the configuration once its test passes. Each one
//! resolves the repository's secrets first, so no resolved value is kept past
//! the job it was resolved for.

use std::sync::Arc;

use domain::execution::FailureReason;
use domain::planning::{AgentConfig, RepositoryConfig, RepositorySettings};
use domain::{OperationId, RepositoryAlias, RepositoryId};
use futures::StreamExt;

use crate::ports::{
    ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, Problem,
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
}

// The error of these use cases is a typed code for the caller to match; it
// is not boxed to satisfy a size rule, for none of its variants is large on
// its own.
#[allow(clippy::result_large_err)]
impl InitRepository {
    pub fn new(
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
    ) -> Self {
        Self {
            engine,
            resolve,
            config,
        }
    }

    /// Initialises the repository the settings describe, and reports its ID.
    /// Saved only when `alias` is given, by the same rule
    /// [`ConnectRepository`] saves one: an alias the configuration already
    /// holds is refused.
    pub async fn init(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
        alias: Option<RepositoryAlias>,
    ) -> Result<InitOutcome, RepositoryError> {
        let access = access(&self.resolve, settings).await?;
        let id = run(&*self.engine, op, Job::Init(access)).await?;
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
    pub fn new(engine: Arc<dyn Engine>, resolve: ResolveRepository) -> Self {
        Self { engine, resolve }
    }

    /// Opens the repository the settings describe and reports its ID. Saves
    /// nothing.
    pub async fn test(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
    ) -> Result<RepositoryId, RepositoryError> {
        let access = access(&self.resolve, settings).await?;
        run(&*self.engine, op, Job::Identify(access)).await
    }
}

#[allow(clippy::result_large_err)]
impl ConnectRepository {
    pub fn new(
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
    ) -> Self {
        Self {
            engine,
            resolve,
            config,
        }
    }

    /// Tests the repository and, once the test passed, saves it under `alias`.
    pub async fn connect(
        &self,
        op: OperationId,
        alias: RepositoryAlias,
        settings: &RepositorySettings,
    ) -> Result<RepositoryId, RepositoryError> {
        let access = access(&self.resolve, settings).await?;
        let id = run(&*self.engine, op, Job::Identify(access)).await?;
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
#[allow(clippy::result_large_err)]
async fn run(
    engine: &dyn Engine,
    op: OperationId,
    job: Job,
) -> Result<RepositoryId, RepositoryError> {
    let RunningJob { events, .. } = engine.run(op, job).await.map_err(RepositoryError::Engine)?;
    let mut events = events.fuse();
    while let Some(event) = events.next().await {
        match event {
            EngineEvent::Succeeded(EngineResult::Repository { id }) => return Ok(id),
            EngineEvent::Failed(reason) => return Err(failed(reason)),
            _ => {}
        }
    }
    // The stream ended without a result: the engine stopped without reporting.
    Err(RepositoryError::Failed(FailureReason::EngineExited {
        exit_code: None,
    }))
}

/// A failed job, as these use cases report it: the one typed refusal is a
/// repository that already exists; the rest keep the engine's reason.
fn failed(reason: FailureReason) -> RepositoryError {
    match reason {
        FailureReason::RepositoryExists => RepositoryError::Exists,
        reason => RepositoryError::Failed(reason),
    }
}

/// Saves `settings` under `alias`, refusing an alias the configuration
/// already holds. What is saved is the settings as written: a secret stays
#[allow(clippy::result_large_err)]
async fn save(
    config: &dyn ConfigStore,
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
) -> Result<(), RepositoryError> {
    let loaded = config.load().await.map_err(RepositoryError::Config)?;
    if loaded.repository(alias).is_some() {
        return Err(RepositoryError::AliasTaken(alias.clone()));
    }
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
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::FailureReason;
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, Uuid};

    use super::{ConnectRepository, InitRepository, RepositoryError, TestRepository};
    use crate::fakes::{FakeSecrets, InMemoryConfigStore};
    use crate::ports::ConfigStore;
    use crate::ports::{Engine, EngineError, EngineEvent, Job, JobControl, RunningJob};
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
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        assert_eq!(use_case.connect(op(), alias(), &settings()).await, Ok(id()));
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
            engine,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        assert!(matches!(
            use_case.connect(op(), alias(), &settings()).await,
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
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            config.clone(),
        );
        use_case
            .connect(op(), alias(), &settings())
            .await
            .unwrap_or_else(|e| panic!("the first connect: {e:?}"));
        assert!(matches!(
            use_case.connect(op(), alias(), &settings()).await,
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
    async fn initialising_under_an_alias_already_taken_is_refused_before_the_job_runs() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case = InitRepository::new(
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
