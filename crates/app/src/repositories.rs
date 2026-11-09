//! Repository use cases (technical design §5, components.md): initialising a
//! repository where nothing is yet, testing one that is there, and connecting
//! one by saving it into the configuration once its test passes. Each one
//! resolves the repository's secrets first, so no resolved value is kept past
//! the job it was resolved for.

use std::sync::Arc;

use domain::execution::FailureReason;
use domain::planning::RepositorySettings;
use domain::{OperationId, RepositoryAlias, RepositoryId};

use crate::ports::{ConfigError, ConfigStore, Engine, EngineError};
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

impl InitRepository {
    pub fn new(engine: Arc<dyn Engine>, resolve: ResolveRepository) -> Self {
        Self { engine, resolve }
    }

    /// Initialises the repository the settings describe, and reports its ID.
    /// Saved only when `alias` is given.
    pub async fn init(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
        alias: Option<RepositoryAlias>,
    ) -> Result<InitOutcome, RepositoryError> {
        todo!("phase 2 of the repositories slice")
    }
}

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
        todo!("phase 2 of the repositories slice")
    }
}

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
        todo!("phase 2 of the repositories slice")
    }
}

#[cfg(all(test, feature = "fakes"))]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use domain::execution::FailureReason;
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, Uuid};
    use futures::StreamExt;

    use super::{ConnectRepository, InitRepository, RepositoryError, TestRepository};
    use crate::fakes::{FakeSecrets, InMemoryConfigStore};
    use crate::ports::ConfigStore;
    use crate::ports::{
        Engine, EngineError, EngineEvent, Job, JobControl, RepositoryAccess, RunningJob,
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
            self.seen.lock().expect("the job log").push(job);
            let id = match self.outcome.lock().expect("the outcome").clone() {
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

    fn alias() -> RepositoryAlias {
        RepositoryAlias::try_from("demo".to_owned()).expect("a valid alias")
    }

    fn id() -> RepositoryId {
        RepositoryId::parse(&"a".repeat(64)).expect("a repository ID")
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
        let seen = engine.seen.lock().expect("the job log");
        let Job::Identify(_) = &seen[0] else {
            panic!("testing runs Job::Identify, got {:?}", seen[0]);
        };
        drop(seen);
        let saved = config.load().await.expect("the configuration loads");
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
        let saved = config.load().await.expect("the configuration loads");
        assert_eq!(saved.repositories().len(), 1);
        assert_eq!(
            saved.repository(&alias()).expect("the repository").settings,
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
            Err(RepositoryError::Failed(_))
        ));
        let saved = config.load().await.expect("the configuration loads");
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
            .expect("the first connect");
        assert!(matches!(
            use_case.connect(op(), alias(), &settings()).await,
            Err(RepositoryError::AliasTaken(_))
        ));
        let saved = config.load().await.expect("the configuration loads");
        assert_eq!(saved.repositories().len(), 1);
    }

    #[tokio::test]
    async fn initialising_saves_only_when_an_alias_is_given() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(id())),
            seen: Mutex::new(Vec::new()),
        });
        let config = Arc::new(InMemoryConfigStore::new());
        let use_case =
            InitRepository::new(engine, ResolveRepository::new(Arc::new(FakeSecrets::new())));
        let outcome = use_case.init(op(), &settings(), None).await;
        assert_eq!(outcome.expect("the initialisation").saved, false);
        use_case
            .init(op(), &settings(), Some(alias()))
            .await
            .expect("the initialisation with an alias");
        let saved = config.load().await.expect("the configuration loads");
        assert_eq!(saved.repositories().len(), 1);
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
        );
        use_case
            .init(op(), &settings(), None)
            .await
            .expect("the initialisation");
        let seen = engine.seen.lock().expect("the job log");
        let Job::Init(_) = &seen[0] else {
            panic!("initialising runs Job::Init, got {:?}", seen[0]);
        };
    }
}
