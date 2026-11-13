//! Steps for the `connect.feature` scenarios that run against fakes: the use
//! cases decide what is saved and what is refused, so the scenarios drive
//! them with an engine scripted to identify or fail, and the in-memory config
//! store stands in for the configuration file.

use std::collections::{BTreeMap, BTreeSet};
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
    Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl,
    OperationStore, RunningJob,
};
use app::repositories::{
    ConnectRepository, InitOutcome, RepositoryError, SavedEntryError, SavedInit, TestRepository,
};
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
/// never answers, and keeps the jobs it was given. A `Job::Identify` is
/// answered with `found` instead once a scenario sets it: what the engine
/// finds at a location it doesn't initialise.
struct Scripted {
    outcome: Mutex<Result<RepositoryId, FailureReason>>,
    found: Mutex<Option<Result<RepositoryId, FailureReason>>>,
    hangs: AtomicBool,
    killed: Arc<AtomicBool>,
    seen: Mutex<Vec<Job>>,
}

/// What a use case gave: the ID it reported, or how it was refused.
#[derive(Debug)]
enum Outcome {
    Id(RepositoryId),
    Saved(InitOutcome),
    /// What creating a saved repository tried.
    Created(SavedInit),
    Refused(RepositoryError),
    /// Creating a saved repository found no entry to create it from.
    NoEntry(SavedEntryError),
    /// The use case was still waiting after more real time than a fake
    /// clock needs: it would have waited for ever.
    Hung,
}

impl From<RepositoryError> for Outcome {
    fn from(error: RepositoryError) -> Self {
        Self::Refused(error)
    }
}

impl From<SavedEntryError> for Outcome {
    fn from(error: SavedEntryError) -> Self {
        Self::NoEntry(error)
    }
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
    /// The saved repositories the agent offered to create, once it decided.
    offered: Option<BTreeSet<RepositoryAlias>>,
    /// A job made from a saved entry that hasn't opened its repository yet.
    read: Option<Job>,
    /// What the last job that opened a saved repository reported.
    ended: Option<Vec<EngineEvent>>,
    /// The engine the last backup a scenario ran went through.
    runs: Option<Arc<Runs>>,
}

impl Default for RepositoriesWorld {
    fn default() -> Self {
        Self {
            engine: Arc::new(Scripted {
                outcome: Mutex::new(Ok(id())),
                found: Mutex::new(None),
                hangs: AtomicBool::new(false),
                killed: Arc::new(AtomicBool::new(false)),
                seen: Mutex::new(Vec::new()),
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
            offered: None,
            read: None,
            ended: None,
            runs: None,
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
    config_recording(name, settings, None)
}

/// The configuration holding `settings` under `name`, its entry recording
/// `recorded` as the repository's ID.
fn config_recording(
    name: &str,
    settings: &RepositorySettings,
    recorded: Option<RepositoryId>,
) -> AgentConfig {
    let repository = RepositoryConfig {
        locations: Default::default(),
        id: recorded,
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

/// The ID of a repository other than the one the engine identifies, as an
/// entry records once something reached it there.
fn other_id() -> RepositoryId {
    RepositoryId::parse(&"b".repeat(64)).expect("a repository ID")
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::try_from(name.to_owned()).expect("a valid alias")
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let found = match &job {
            Job::Identify(_) => self.found.lock().expect("what is found").clone(),
            _ => None,
        };
        self.seen.lock().expect("the jobs").push(job);
        if self.hangs.load(Ordering::SeqCst) {
            return Ok(RunningJob {
                events: Box::pin(futures::stream::pending()),
                control: Box::new(Killable(self.killed.clone())),
            });
        }
        let outcome = found.unwrap_or_else(|| self.outcome.lock().expect("the outcome").clone());
        let event = match outcome {
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

/// A location with a folder that holds nothing of a repository: the
/// engine finds no repository there, and initialises one.
#[given("a location the engine finds empty")]
fn finds_empty(agent: &mut super::Agent) {
    *agent
        .repositories
        .engine
        .found
        .lock()
        .expect("what is found") = Some(Err(FailureReason::RepositoryNotFound));
}

/// A location whose folder isn't there, as on a drive that isn't plugged
/// in: the engine opens nothing there, and says so in its own words.
#[given("a location whose folder is not there")]
fn folder_not_there(agent: &mut super::Agent) {
    *agent
        .repositories
        .engine
        .found
        .lock()
        .expect("what is found") = Some(Err(FailureReason::EngineFailed {
        detail: "the repository folder is not there".to_owned(),
    }));
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
async fn within<T, E: Into<Outcome>>(
    agent: &mut super::Agent,
    run: impl Future<Output = Result<T, E>>,
    wrap: impl FnOnce(T) -> Outcome,
) {
    agent.repositories.began = Some(agent.clock.now());
    agent.repositories.outcome = Some(
        match tokio::time::timeout(Duration::from_secs(5), run).await {
            Ok(Ok(done)) => wrap(done),
            Ok(Err(error)) => error.into(),
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

#[given(expr = "the agent has the repository {string} configured with another repository's ID")]
async fn configured_with_other_id(agent: &mut super::Agent, name: String) {
    let config = config_recording(&name, &agent.repositories.settings, Some(other_id()));
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

#[given(expr = "the agent connected it as {string}")]
async fn connected(agent: &mut super::Agent, name: String) {
    connects(agent, name).await;
    if let Some(Outcome::Refused(error)) = &agent.repositories.outcome {
        panic!("the first connect was refused: {error:?}");
    }
}

/// The same repository, reached by another location: the engine identifies
/// it by the same ID.
#[when(expr = "the agent connects it at another location as {string}")]
async fn connects_elsewhere(agent: &mut super::Agent, name: String) {
    agent.repositories.settings.repository = "/Volumes/demo/scratch/cold".to_owned();
    connects(agent, name).await;
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

#[when(expr = "the agent creates the saved repository {string}")]
async fn creates_saved(agent: &mut super::Agent, name: String) {
    let use_case = app::repositories::InitRepository::new(
        agent.clock.clone(),
        agent.repositories.engine.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        agent.repositories.store.clone(),
    );
    let alias = alias(&name);
    let offers = app::repositories::CreateOffers::new(agent.store.clone());
    within(
        agent,
        use_case.init_saved(op(), &alias, &offers),
        Outcome::Created,
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

#[then("the creation reports the repository's ID")]
fn creation_reports_id(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Created(tried)) => {
            assert_eq!(tried.created, Ok(agent.repositories.id.clone()))
        }
        other => panic!("the creation did not report an ID: {other:?}"),
    }
}

#[then(
    "the engine looked at the location, then initialised the repository there, with its saved settings"
)]
fn initialised_with_saved_settings(agent: &mut super::Agent) {
    let seen = agent.repositories.engine.seen.lock().expect("the jobs");
    let [Job::Identify(looked), Job::Init(access)] = seen.as_slice() else {
        panic!("the engine was not given a look, then one initialisation: {seen:?}");
    };
    let settings = &agent.repositories.settings;
    let SecretRef::Inline(password) = &settings.password else {
        panic!("the scenarios' password is written inline");
    };
    for access in [looked, access] {
        assert_eq!(access.repository, settings.repository);
        assert_eq!(access.repo_hot, settings.repo_hot);
        assert_eq!(access.password.expose(), password.expose());
    }
}

#[then("the engine initialised no repository")]
fn initialised_none(agent: &mut super::Agent) {
    let seen = agent.repositories.engine.seen.lock().expect("the jobs");
    assert!(
        !seen.iter().any(|job| matches!(job, Job::Init(_))),
        "the engine ran {seen:?}"
    );
}

#[then(expr = "the creation is refused because the agent has no repository {string}")]
fn creation_refused_unknown(agent: &mut super::Agent, name: String) {
    match &agent.repositories.outcome {
        Some(Outcome::NoEntry(SavedEntryError::Unknown(unknown))) => {
            assert_eq!(*unknown, alias(&name))
        }
        other => panic!("the creation was not refused as unknown: {other:?}"),
    }
}

#[then("the creation is refused because a repository already exists")]
fn creation_refused_exists(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Created(tried)) => assert_eq!(tried.created, Err(RepositoryError::Exists)),
        other => panic!("the creation was not refused as existing: {other:?}"),
    }
}

#[then("the creation is refused because its row doesn't offer it")]
fn creation_refused_not_offered(agent: &mut super::Agent) {
    match &agent.repositories.outcome {
        Some(Outcome::Created(tried)) => {
            assert_eq!(tried.created, Err(RepositoryError::CreateNotOffered))
        }
        other => panic!("the creation was not refused as not offered: {other:?}"),
    }
}

#[then("the engine ran no job")]
fn ran_no_job(agent: &mut super::Agent) {
    let seen = agent.repositories.engine.seen.lock().expect("the jobs");
    assert!(seen.is_empty(), "the engine ran {seen:?}");
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

#[then(expr = "the request is refused because the repository is configured as {string}")]
fn already_configured_as(agent: &mut super::Agent, name: String) {
    match &agent.repositories.outcome {
        Some(Outcome::Refused(RepositoryError::AlreadyConfigured(configured))) => {
            assert_eq!(configured, &alias(&name));
        }
        other => panic!("the request was not refused for the repository: {other:?}"),
    }
}

#[then(expr = "the agent has the repository {string} configured with the repository's ID")]
async fn configured_with_id(agent: &mut super::Agent, name: String) {
    let config = saved(agent).await;
    let repository = config
        .repository(&alias(&name))
        .unwrap_or_else(|| panic!("no repository {name} was saved"));
    assert_eq!(repository.id.as_ref(), Some(&agent.repositories.id));
}

#[then(expr = "the agent has the repository {string} configured with no repository ID")]
async fn configured_without_id(agent: &mut super::Agent, name: String) {
    let config = saved(agent).await;
    let repository = config
        .repository(&alias(&name))
        .unwrap_or_else(|| panic!("no repository {name} was saved"));
    assert_eq!(repository.id, None);
}

#[then(expr = "the agent has the repository {string} configured with another repository's ID")]
async fn still_configured_with_other_id(agent: &mut super::Agent, name: String) {
    let config = saved(agent).await;
    let repository = config
        .repository(&alias(&name))
        .unwrap_or_else(|| panic!("no repository {name} was saved"));
    assert_eq!(repository.id, Some(other_id()));
    assert_eq!(repository.settings, agent.repositories.settings);
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

#[given(
    expr = "the agent has the repository {string} configured with its settings, backed up to by the plan {string}"
)]
async fn configured_with_plan(agent: &mut super::Agent, name: String, plan: String) {
    let only = config_with(&name, &agent.repositories.settings);
    save_with_plan(agent, only, &name, &plan).await;
}

#[given(
    expr = "the agent has the repository {string} configured with another repository's ID, backed up to by the plan {string}"
)]
async fn configured_with_other_id_and_plan(agent: &mut super::Agent, name: String, plan: String) {
    let only = config_recording(&name, &agent.repositories.settings, Some(other_id()));
    save_with_plan(agent, only, &name, &plan).await;
}

/// Saves `only`'s repositories with the plan `plan` backing up to `name`.
async fn save_with_plan(agent: &super::Agent, only: AgentConfig, name: &str, plan: &str) {
    let source = domain::planning::Source::parse("/home/demo").expect("a source");
    let config = AgentConfig::new(
        Default::default(),
        Default::default(),
        only.repositories().clone(),
        vec![domain::planning::Plan::new(
            domain::PlanId::parse(plan).expect("a plan ID"),
            alias(name),
            vec![source],
        )],
    )
    .expect("a valid configuration");
    agent
        .repositories
        .store
        .save(&config)
        .await
        .expect("the configuration saves");
}

/// Records the latest backup of `plan` in the agent's operation log, ended as
/// `status` says.
async fn record_latest(
    agent: &super::Agent,
    plan: &str,
    status: domain::execution::OperationStatus,
) {
    record_backup(agent, op(), plan, status).await;
}

/// Records backup `id` of `plan` in the agent's operation log, ended as
/// `status` says; a lower `id` is an older backup.
async fn record_backup(
    agent: &super::Agent,
    id: OperationId,
    plan: &str,
    status: domain::execution::OperationStatus,
) {
    use domain::execution::{Operation, OperationKind, OperationStatus, Target, Trigger};
    let at = agent.clock.now();
    let mut operation = Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Manual,
        Target {
            plan: Some(domain::PlanId::parse(plan).expect("a plan ID")),
            repository: None,
        },
        at,
    );
    operation.start(at).expect("a queued operation starts");
    match status {
        OperationStatus::Running => {}
        OperationStatus::Failed { reason } => operation.fail(reason, at).expect("it fails"),
        _ => operation.succeed(at).expect("it succeeds"),
    }
    agent
        .store
        .save(&operation, None)
        .await
        .expect("the operation is recorded");
}

#[given(expr = "no operation of the plan {string} is on record")]
fn no_operation(_agent: &mut super::Agent, _plan: String) {}

#[given(expr = "the latest operation of the plan {string} failed because no repository is there")]
async fn latest_found_none(agent: &mut super::Agent, plan: String) {
    let reason = FailureReason::RepositoryNotFound;
    record_latest(
        agent,
        &plan,
        domain::execution::OperationStatus::Failed { reason },
    )
    .await;
}

#[given(expr = "the latest operation of the plan {string} failed because the daemon stopped")]
async fn latest_daemon_stopped(agent: &mut super::Agent, plan: String) {
    let reason = FailureReason::DaemonStopped;
    record_latest(
        agent,
        &plan,
        domain::execution::OperationStatus::Failed { reason },
    )
    .await;
}

#[given(
    expr = "a backup of the plan {string} succeeded, then the latest failed because no repository is there"
)]
async fn succeeded_then_found_none(agent: &mut super::Agent, plan: String) {
    let earlier = OperationId::from_uuid(Uuid::from_u128(0x5eec));
    record_backup(
        agent,
        earlier,
        &plan,
        domain::execution::OperationStatus::Succeeded,
    )
    .await;
    let reason = FailureReason::RepositoryNotFound;
    record_latest(
        agent,
        &plan,
        domain::execution::OperationStatus::Failed { reason },
    )
    .await;
}

#[given(expr = "the latest operation of the plan {string} succeeded")]
async fn latest_succeeded(agent: &mut super::Agent, plan: String) {
    record_latest(agent, &plan, domain::execution::OperationStatus::Succeeded).await;
}

#[given(expr = "the latest operation of the plan {string} is still running")]
async fn latest_running(agent: &mut super::Agent, plan: String) {
    record_latest(agent, &plan, domain::execution::OperationStatus::Running).await;
}

#[when("the agent decides which saved repositories to offer to create")]
async fn decides_offers(agent: &mut super::Agent) {
    let config = saved(agent).await;
    let offered = app::repositories::CreateOffers::new(agent.store.clone())
        .offered(&config)
        .await
        .expect("the operation log is read");
    agent.repositories.offered = Some(offered);
}

#[then(expr = "the repository {string} is offered to be created")]
fn is_offered(agent: &mut super::Agent, name: String) {
    let offered = agent
        .repositories
        .offered
        .as_ref()
        .expect("the agent decided");
    assert!(offered.contains(&alias(&name)), "offered only {offered:?}");
}

#[then(expr = "the repository {string} is not offered to be created")]
fn is_not_offered(agent: &mut super::Agent, name: String) {
    let offered = agent
        .repositories
        .offered
        .as_ref()
        .expect("the agent decided");
    assert!(!offered.contains(&alias(&name)), "offered {offered:?}");
}

/// An engine that, for every job, opens the repository whose ID it holds
/// and ends the job well.
struct Opens(RepositoryId);

impl Opens {
    fn events(&self) -> Vec<EngineEvent> {
        vec![
            EngineEvent::Started {
                index_read_at: Timestamp::from_second(1_800_000_000).expect("a time"),
                repository: Some(self.0.clone()),
            },
            EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
        ]
    }
}

#[async_trait]
impl Engine for Opens {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let mut events = self.events();
        // An identify reports the ID it found, as the worker's does.
        if let (Job::Identify(_), Some(EngineEvent::Succeeded(result))) = (&job, events.last_mut())
        {
            *result = EngineResult::Repository { id: self.0.clone() };
        }
        Ok(RunningJob {
            events: Box::pin(futures::stream::iter(events)),
            control: Box::new(Stop),
        })
    }
}

#[given(
    expr = "the agent has the repository {string} configured with its settings, and {string} at another location"
)]
async fn configured_twice(agent: &mut super::Agent, name: String, other: String) {
    let settings = agent.repositories.settings.clone();
    let mut elsewhere = settings.clone();
    elsewhere.repository = "/media/demo/cold".to_owned();
    elsewhere.repo_hot = Some("/media/demo/hot".to_owned());
    let entry = |settings: RepositorySettings| RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings,
        maintenance: Default::default(),
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let config = AgentConfig::new(
        Default::default(),
        Default::default(),
        BTreeMap::from([
            (alias(&name), entry(settings)),
            (alias(&other), entry(elsewhere)),
        ]),
        Vec::new(),
    )
    .expect("a valid configuration");
    agent
        .repositories
        .store
        .save(&config)
        .await
        .expect("the configuration saves");
}

#[given(expr = "the repository {string} was set by hand to be maintained from this agent")]
async fn maintained_by_hand(agent: &mut super::Agent, name: String) {
    let config = saved(agent).await;
    let mut repositories = config.repositories().clone();
    repositories
        .get_mut(&alias(&name))
        .expect("the entry")
        .maintenance = Maintenance::Local;
    let edited = AgentConfig::new(
        Default::default(),
        Default::default(),
        repositories,
        config.plans().to_vec(),
    )
    .expect("a valid configuration");
    agent
        .repositories
        .store
        .save(&edited)
        .await
        .expect("the configuration saves");
}

/// The job of `kind` the agent makes from the saved entry `name`, as it is
/// stored now.
async fn job_of(agent: &super::Agent, kind: &str, name: &str) -> Job {
    use app::ports::{BackupJob, CheckJob, PruneJob};
    use domain::execution::deadline::KeepDelete;
    use domain::execution::plan_tags::PlanTags;
    let entry = saved(agent)
        .await
        .repository(&alias(name))
        .cloned()
        .expect("the entry");
    let repository = ResolveRepository::new(Arc::new(FakeSecrets::new()))
        .resolve_saved(&alias(name), &entry)
        .await
        .expect("the entry's settings resolve")
        .access;
    match kind {
        "backup" => Job::Backup(BackupJob {
            repository,
            sources: vec!["/home/demo".to_owned()],
            tags: PlanTags::new(
                domain::PlanId::parse("demo-home").expect("a plan ID"),
                domain::AgentId::from_uuid(Uuid::from_u128(1)),
            ),
            extra_tags: Vec::new(),
            label: None,
            host: None,
            filter: Default::default(),
            deadline_budget: domain::SignedDuration::from_hours(1),
        }),
        "check" => Job::Check(CheckJob {
            repository,
            read_data: false,
        }),
        "prune" => Job::Prune(PruneJob {
            repository,
            keep_delete: KeepDelete::MIN,
        }),
        "snapshot listing" => Job::Ls(repository),
        other => panic!("no job {other:?}"),
    }
}

/// Runs `job` through the engine the agent records opened repositories
/// with, to its end.
async fn open(agent: &mut super::Agent, job: Job) {
    use app::repositories::{ConfigWrites, RecordOpened};
    use futures::StreamExt;
    let engine = RecordOpened::new(
        Arc::new(Opens(agent.repositories.id.clone())),
        agent.repositories.store.clone(),
        Arc::new(ConfigWrites::new(())),
        Arc::new(|_| {}),
    );
    let events = engine
        .run(op(), job)
        .await
        .expect("the engine takes the job")
        .events
        .collect()
        .await;
    agent.repositories.ended = Some(events);
}

#[when(
    regex = r#"^an? (backup|check|prune|snapshot listing) of the repository "([^"]+)" opens it$"#
)]
async fn job_opens(agent: &mut super::Agent, kind: String, name: String) {
    let job = job_of(agent, &kind, &name).await;
    open(agent, job).await;
}

#[given(expr = "a check of the repository {string} opened it")]
async fn check_opened(agent: &mut super::Agent, name: String) {
    let job = job_of(agent, "check", &name).await;
    open(agent, job).await;
}

#[given(expr = "a check of the repository {string} read its entry")]
async fn check_read(agent: &mut super::Agent, name: String) {
    agent.repositories.read = Some(job_of(agent, "check", &name).await);
}

#[when("that check opens the repository")]
async fn that_check_opens(agent: &mut super::Agent) {
    let job = agent
        .repositories
        .read
        .take()
        .expect("a check read its entry");
    open(agent, job).await;
}

#[then(regex = r"^the (backup|check|prune|snapshot listing) ended as the engine reported it$")]
fn ended_as_reported(agent: &mut super::Agent, _kind: String) {
    let reported = Opens(agent.repositories.id.clone()).events();
    assert_eq!(
        agent.repositories.ended.as_ref(),
        Some(&reported),
        "the job's events changed on the way"
    );
}

/// An engine for a backup's whole run, which keeps the jobs it is given. An
/// identify finds what the scenario set the engine to find, or else the
/// repository `id`; every other job opens `id` and ends well.
struct Runs {
    id: RepositoryId,
    found: Option<Result<RepositoryId, FailureReason>>,
    seen: Mutex<Vec<Job>>,
}

impl Runs {
    fn seen(&self) -> Vec<Job> {
        self.seen.lock().expect("the jobs").clone()
    }
}

#[async_trait]
impl Engine for Runs {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let identify = matches!(job, Job::Identify(_));
        self.seen.lock().expect("the jobs").push(job);
        let started = EngineEvent::Started {
            index_read_at: Timestamp::from_second(1_800_000_000).expect("a time"),
            repository: Some(self.id.clone()),
        };
        let events = match (identify, &self.found) {
            // A worker that can't open the repository fails before it starts.
            (true, Some(Err(reason))) => vec![EngineEvent::Failed(reason.clone())],
            (true, _) => vec![
                started,
                EngineEvent::Succeeded(EngineResult::Repository {
                    id: self.id.clone(),
                }),
            ],
            (false, _) => vec![
                started,
                EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
            ],
        };
        Ok(RunningJob {
            events: Box::pin(futures::stream::iter(events)),
            control: Box::new(Stop),
        })
    }
}

/// Runs a backup made from the saved entry `name` to its end, as the daemon
/// runs one: through the engine that records opened repositories, into the
/// operation log.
#[when(expr = "a backup of the repository {string} runs")]
async fn backup_runs(agent: &mut super::Agent, name: String) {
    use app::execution::RunOperation;
    use app::repositories::{ConfigWrites, RecordOpened};
    use domain::execution::Trigger;
    let runs = Arc::new(Runs {
        id: agent.repositories.id.clone(),
        found: agent
            .repositories
            .engine
            .found
            .lock()
            .expect("what is found")
            .clone(),
        seen: Mutex::new(Vec::new()),
    });
    let engine = RecordOpened::new(
        Arc::clone(&runs) as Arc<dyn Engine>,
        agent.repositories.store.clone(),
        Arc::new(ConfigWrites::new(())),
        Arc::new(|_| {}),
    );
    let job = job_of(agent, "backup", &name).await;
    let finished = RunOperation::new(Arc::new(engine), agent.clock.clone(), agent.store.clone())
        .run(op(), Trigger::Manual, job, futures::future::pending())
        .await;
    agent.finished = Some(finished);
    agent.repositories.runs = Some(runs);
}

fn ran(agent: &super::Agent) -> Vec<Job> {
    agent
        .repositories
        .runs
        .as_ref()
        .expect("a backup ran")
        .seen()
}

#[then("the repository was identified first, and the backup ran with the repository's ID")]
fn identified_then_backed_up(agent: &mut super::Agent) {
    let seen = ran(agent);
    let [Job::Identify(looked), Job::Backup(backup)] = seen.as_slice() else {
        panic!("the engine ran {seen:?}");
    };
    assert_eq!(looked.id, None, "the identify looked for the ID");
    assert_eq!(backup.repository.id, Some(agent.repositories.id.clone()));
}

#[then("the repository was identified, and nothing else ran")]
fn identified_alone(agent: &mut super::Agent) {
    let seen = ran(agent);
    assert!(
        matches!(seen.as_slice(), [Job::Identify(_)]),
        "the engine ran {seen:?}"
    );
}

#[then("the operation log names the repository the engine opened")]
async fn log_names_the_repository(agent: &mut super::Agent) {
    let logged = agent
        .store
        .get(op())
        .await
        .expect("the log answers")
        .expect("the log holds the backup");
    assert_eq!(
        logged.record.status,
        domain::execution::OperationStatus::Succeeded
    );
    assert_eq!(
        logged.record.target.repository,
        Some(agent.repositories.id.clone())
    );
}

#[then("the backup ended as failed because the repository folder is not there")]
fn backup_failed_as_identified(agent: &mut super::Agent) {
    let finished = agent.finished.as_ref().expect("a backup ran");
    assert_eq!(
        finished.operation.status(),
        &domain::execution::OperationStatus::Failed {
            reason: FailureReason::EngineFailed {
                detail: "the repository folder is not there".to_owned(),
            },
        }
    );
}
