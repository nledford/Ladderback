//! Steps for the `connect.feature` scenarios that run against fakes: the use
//! cases decide what is saved and what is refused, so the scenarios drive
//! them with an engine scripted to identify or fail, and the in-memory config
//! store stands in for the configuration file.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::FailureReason;
use domain::execution::deadline::REPOSITORY_JOB_BUDGET;
use domain::planning::{AgentConfig, Maintenance, RepositoryConfig, RepositorySettings};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, RepositoryId, Timestamp, Uuid};

use app::fakes::{FakeSecrets, InMemoryConfigStore};
use app::ports::{
    Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RunningJob,
};
use app::repositories::{ConnectRepository, InitOutcome, RepositoryError, TestRepository};
use app::secrets::ResolveRepository;

struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}

/// A job's control that notes when its worker is dropped, which kills it.
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

/// An engine that answers every repository job with the scripted outcome, or
/// never answers.
struct Scripted {
    outcome: Mutex<Result<RepositoryId, FailureReason>>,
    hangs: AtomicBool,
    killed: Arc<AtomicBool>,
}

/// What a use case gave: the ID it reported, or how it was refused.
#[derive(Debug)]
enum Outcome {
    Id(RepositoryId),
    Saved(InitOutcome),
    Refused(RepositoryError),
    /// The use case was still waiting after more real time than a fake
    /// clock needs: it would have waited for ever.
    Hung,
}

/// What the connect scenarios set up and what the use cases gave.
pub struct RepositoriesWorld {
    engine: Arc<Scripted>,
    store: Arc<InMemoryConfigStore>,
    settings: RepositorySettings,
    id: RepositoryId,
    outcome: Option<Outcome>,
    /// When the request began, on the agent's clock.
    began: Option<Timestamp>,
}

impl Default for RepositoriesWorld {
    fn default() -> Self {
        Self {
            engine: Arc::new(Scripted {
                outcome: Mutex::new(Ok(id())),
                hangs: AtomicBool::new(false),
                killed: Arc::new(AtomicBool::new(false)),
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
            began: None,
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
        if self.hangs.load(Ordering::SeqCst) {
            return Ok(RunningJob {
                events: Box::pin(futures::stream::pending()),
                control: Box::new(Killable(self.killed.clone())),
            });
        }
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

#[given("a repository the engine never answers")]
fn never_answers(agent: &mut super::Agent) {
    agent
        .repositories
        .engine
        .hangs
        .store(true, Ordering::SeqCst);
}

/// Runs a use case to its end, and notes when it began. A use case that
/// waits for ever is a failure of the scenario, not a stuck run.
async fn within<T>(
    agent: &mut super::Agent,
    run: impl Future<Output = Result<T, RepositoryError>>,
    wrap: impl FnOnce(T) -> Outcome,
) {
    agent.repositories.began = Some(agent.clock.now());
    agent.repositories.outcome = Some(
        match tokio::time::timeout(Duration::from_secs(5), run).await {
            Ok(Ok(done)) => wrap(done),
            Ok(Err(error)) => Outcome::Refused(error),
            Err(_) => Outcome::Hung,
        },
    );
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
        agent.clock.clone(),
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
    );
    let settings = agent.repositories.settings.clone();
    within(agent, use_case.test(op(), &settings), Outcome::Id).await;
}

#[when(expr = "the agent connects it as {string}")]
async fn connects(agent: &mut super::Agent, name: String) {
    connect(agent, name, Maintenance::None).await;
}

#[when(expr = "the agent connects it as {string} to maintain it from this agent")]
async fn connects_to_maintain(agent: &mut super::Agent, name: String) {
    connect(agent, name, Maintenance::Local).await;
}

async fn connect(agent: &mut super::Agent, name: String, maintenance: Maintenance) {
    let use_case = ConnectRepository::new(
        agent.clock.clone(),
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        agent.repositories.store.clone(),
    );
    let settings = agent.repositories.settings.clone();
    within(
        agent,
        use_case.connect(op(), alias(&name), &settings, maintenance),
        Outcome::Id,
    )
    .await;
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
        agent.clock.clone(),
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        agent.repositories.store.clone(),
    );
    let settings = agent.repositories.settings.clone();
    within(
        agent,
        use_case.init(op(), &settings, name.map(|name| alias(&name))),
        Outcome::Saved,
    )
    .await;
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

/// The maintenance the saved entry of `name` asks for.
async fn maintenance_of(agent: &super::Agent, name: &str) -> Maintenance {
    saved(agent)
        .await
        .repository(&alias(name))
        .unwrap_or_else(|| panic!("no repository {name} was saved"))
        .maintenance
}

#[then(expr = "the repository {string} asks this agent to maintain it")]
async fn asks_this_agent(agent: &mut super::Agent, name: String) {
    assert_eq!(maintenance_of(agent, &name).await, Maintenance::Local);
}

#[then(expr = "the repository {string} asks nobody to maintain it")]
async fn asks_nobody(agent: &mut super::Agent, name: String) {
    assert_eq!(maintenance_of(agent, &name).await, Maintenance::None);
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

#[then("the request is refused because the repository did not answer in time")]
fn timed_out(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Refused(RepositoryError::TimedOut)) => {}
        other => panic!("the request was not refused as timed out: {other:?}"),
    }
}

#[then("the repository's worker was stopped")]
fn worker_stopped(agent: &mut super::Agent) {
    assert!(
        agent.repositories.engine.killed.load(Ordering::SeqCst),
        "the worker of a repository that never answered was left running"
    );
}

#[then("the request gave up when the repository job's time was up")]
fn gave_up_on_time(agent: &mut super::Agent) {
    let began = agent.repositories.began.expect("the request began");
    assert_eq!(
        agent.clock.now(),
        began
            .checked_add(REPOSITORY_JOB_BUDGET)
            .expect("a time within range"),
        "the request waited for another time than the job's"
    );
}
