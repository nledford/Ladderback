//! Steps for the `connect.feature` scenarios that run against fakes: the use
//! cases decide what is saved and what is refused, so the scenarios drive
//! them with an engine scripted to identify or fail, and the in-memory config
//! store stands in for the configuration file.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::FailureReason;
use domain::planning::{AgentConfig, RepositoryConfig, RepositorySettings};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, RepositoryId, Uuid};
use futures::StreamExt;

use app::fakes::{FakeSecrets, InMemoryConfigStore};
use app::ports::{
    ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RunningJob,
};
use app::repositories::{ConnectRepository, InitOutcome, RepositoryError, TestRepository};
use app::secrets::ResolveRepository;

struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}

/// An engine that answers every repository job with the scripted outcome.
struct Scripted {
    outcome: Mutex<Result<RepositoryId, FailureReason>>,
}

/// What a use case gave: the ID it reported, or how it was refused.
#[derive(Debug)]
enum Outcome {
    Id(RepositoryId),
    Saved(InitOutcome),
    Refused(RepositoryError),
}

/// What the connect scenarios set up and what the use cases gave.
pub struct RepositoriesWorld {
    engine: Arc<Scripted>,
    store: Arc<InMemoryConfigStore>,
    settings: RepositorySettings,
    id: RepositoryId,
    outcome: Option<Outcome>,
}

impl Default for RepositoriesWorld {
    fn default() -> Self {
        Self {
            engine: Arc::new(Scripted {
                outcome: Mutex::new(Ok(id())),
            }),
            store: Arc::new(InMemoryConfigStore::new()),
            settings: RepositorySettings {
                repository: "/home/demo/scratch/cold".to_owned(),
                repo_hot: Some("/home/demo/scratch/hot".to_owned()),
                password: SecretRef::Inline(SecretValue::new("demo-password")),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            id: id(),
            outcome: None,
        }
    }
}

fn id() -> RepositoryId {
    RepositoryId::parse(&"a".repeat(64)).expect("a repository ID")
}

fn op() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(0x5eed))
}

/// The configuration holding `settings` under `name`, as a connect would
/// leave it.
fn config_with(name: &str, settings: &RepositorySettings) -> AgentConfig {
    let repository = RepositoryConfig {
        settings: settings.clone(),
        maintenance: Default::default(),
        group_by: Default::default(),
        hooks: Default::default(),
    };
    AgentConfig::new(
        Default::default(),
        Default::default(),
        BTreeMap::from([(alias(name), repository)]),
        Vec::new(),
    )
    .expect("a valid configuration")
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::try_from(name.to_owned()).expect("a valid alias")
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        let event = match self.outcome.lock().expect("the outcome").clone() {
            Ok(id) => EngineEvent::Succeeded(EngineResult::Repository { id }),
            Err(reason) => EngineEvent::Failed(reason),
        };
        Ok(RunningJob {
            events: Box::pin(futures::stream::iter([event])),
            control: Box::new(Stop),
        })
    }
}

#[given("a repository the engine identifies")]
fn identifies(agent: &mut super::Agent) {
    *agent
        .repositories
        .engine
        .outcome
        .lock()
        .expect("the outcome") = Ok(agent.repositories.id.clone());
}

#[given("a repository the engine cannot open")]
fn cannot_open(agent: &mut super::Agent) {
    *agent
        .repositories
        .engine
        .outcome
        .lock()
        .expect("the outcome") = Err(FailureReason::EngineFailed {
        detail: "no repository at the location".to_owned(),
    });
}

#[given(expr = "the agent has the repository {string} configured with its settings")]
async fn already_configured(agent: &mut super::Agent, name: String) {
    let config = config_with(&name, &agent.repositories.settings);
    agent
        .repositories
        .store
        .save(&config)
        .await
        .expect("the configuration saves");
}

#[when("the agent tests the repository")]
async fn tests(agent: &mut super::Agent) {
    let use_case = TestRepository::new(
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
    );
    agent.repositories.outcome = Some(
        match use_case.test(op(), &agent.repositories.settings).await {
            Ok(id) => Outcome::Id(id),
            Err(error) => Outcome::Refused(error),
        },
    );
}

#[when(expr = "the agent connects it as {string}")]
async fn connects(agent: &mut super::Agent, name: String) {
    let use_case = ConnectRepository::new(
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        agent.repositories.store.clone(),
    );
    agent.repositories.outcome = Some(
        match use_case
            .connect(op(), alias(&name), &agent.repositories.settings)
            .await
        {
            Ok(id) => Outcome::Id(id),
            Err(error) => Outcome::Refused(error),
        },
    );
}

#[when("the agent initialises the repository")]
async fn initialises(agent: &mut super::Agent) {
    initialise(agent, None).await;
}

#[when(expr = "the agent initialises the repository as {string}")]
async fn initialises_as(agent: &mut super::Agent, name: String) {
    initialise(agent, Some(name)).await;
}

async fn initialise(agent: &mut super::Agent, name: Option<String>) {
    let use_case = app::repositories::InitRepository::new(
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
    );
    agent.repositories.outcome = Some(
        match use_case
            .init(
                op(),
                &agent.repositories.settings,
                name.map(|name| alias(&name)),
            )
            .await
        {
            Ok(outcome) => Outcome::Saved(outcome),
            Err(error) => Outcome::Refused(error),
        },
    );
}

async fn saved(agent: &super::Agent) -> AgentConfig {
    agent
        .repositories
        .store
        .load()
        .await
        .expect("the configuration loads")
}

#[then("the test reports the repository's ID")]
fn test_reports_id(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Id(id)) => assert_eq!(*id, agent.repositories.id),
        other => panic!("the test did not report an ID: {other:?}"),
    }
}

#[then("the initialisation reports the repository's ID")]
fn init_reports_id(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Saved(outcome)) => assert_eq!(outcome.id, agent.repositories.id),
        other => panic!("the initialisation did not report an ID: {other:?}"),
    }
}

#[then("the agent has no repository configured")]
async fn none_configured(agent: &mut super::Agent) {
    let config = saved(agent).await;
    assert!(config.repositories().is_empty(), "nothing was saved");
}

#[then("the agent has one repository configured")]
async fn one_configured(agent: &mut super::Agent) {
    let config = saved(agent).await;
    assert_eq!(config.repositories().len(), 1, "the configuration changed");
}

#[then(expr = "the agent has the repository {string} configured with its settings")]
async fn configured_with_settings(agent: &mut super::Agent, name: String) {
    let config = saved(agent).await;
    let repository = config
        .repository(&alias(&name))
        .unwrap_or_else(|| panic!("no repository {name} was saved"));
    assert_eq!(repository.settings, agent.repositories.settings);
}

#[then("the connection is refused because the alias is taken")]
fn alias_taken(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Refused(RepositoryError::AliasTaken(_))) => {}
        other => panic!("the connect was not refused for a taken alias: {other:?}"),
    }
}

#[then("the connection fails")]
fn connection_fails(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Refused(RepositoryError::Failed(_))) => {}
        other => panic!("the connect did not fail: {other:?}"),
    }
}
