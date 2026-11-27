//! Steps for `features/maintenance/queue.feature`: they start prunes and
//! checks through the maintenance use case and its queue, on two
//! repositories maintained here, against an engine that holds each prune or
//! check until the scenario lets it finish, and read what each run did from
//! the operation log the world's `store` holds.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use app::fakes::{FakeRetention, FakeSecrets, InMemoryConfigStore};
use app::maintenance::{MaintenanceError, MaintenanceKind, Queued, RunMaintenance};
use app::ports::{
    CheckSummary, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl,
    NormalizeSources, OperationStore, PruneSummary, RunningJob, SourcesError,
};
use app::queue::MaintenanceQueue;
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::OperationStatus;
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, GroupBy, Hooks, Maintenance, RepositoryConfig,
    RepositorySettings, Source,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, RepositoryId, Timestamp, Uuid};
use futures::channel::oneshot;
use futures::{StreamExt, stream};
use tokio::sync::Notify;

use super::Agent;

/// How many times a step lets the runs go on before it gives up waiting for
/// what it expects.
const PATIENCE: usize = 2_000;

/// The scenario's repositories, its queues and its runs.
pub struct MaintenanceQueueWorld {
    config: Arc<InMemoryConfigStore>,
    repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
    limit: Concurrency,
    queue: Option<Arc<MaintenanceQueue>>,
    engine: Arc<Held>,
    /// Each run started, by what it is and its repository.
    runs: BTreeMap<(String, String), OperationId>,
    cancels: BTreeMap<OperationId, oneshot::Sender<()>>,
    /// The run whose start was refused, and why.
    refused: Option<(OperationId, MaintenanceError)>,
    made: u128,
}

impl Default for MaintenanceQueueWorld {
    fn default() -> Self {
        Self {
            config: Arc::new(InMemoryConfigStore::new()),
            repositories: BTreeMap::new(),
            limit: Concurrency::default(),
            queue: None,
            engine: Arc::new(Held::default()),
            runs: BTreeMap::new(),
            cancels: BTreeMap::new(),
            refused: None,
            made: 0,
        }
    }
}

impl MaintenanceQueueWorld {
    fn queue(&mut self) -> Arc<MaintenanceQueue> {
        Arc::clone(
            self.queue
                .get_or_insert_with(|| Arc::new(MaintenanceQueue::new())),
        )
    }

    async fn save(&self) {
        let config = AgentConfig::new(
            AgentSettings {
                maintenance_concurrency: self.limit,
                ..AgentSettings::default()
            },
            Hooks::default(),
            self.repositories.clone(),
            Vec::new(),
        )
        .expect("the draft is valid");
        self.config.save(&config).await.expect("the draft saves");
    }

    fn run(&self, kind: &str, repository: &str) -> OperationId {
        *self
            .runs
            .get(&(kind.to_owned(), repository.to_owned()))
            .unwrap_or_else(|| panic!("no {kind} of {repository} was started"))
    }
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("an alias")
}

fn location(name: &str) -> String {
    format!("/srv/{name}/repo")
}

/// The ID the engine reads from the repository at `location`.
fn id_at(location: &str) -> RepositoryId {
    let n = location.bytes().fold(0_u64, |sum, byte| {
        sum.wrapping_mul(31).wrapping_add(u64::from(byte))
    });
    RepositoryId::parse(&format!("{n:064x}")).expect("64 hex digits are a repository ID")
}

fn kind_of(kind: &str) -> MaintenanceKind {
    match kind {
        "prune" => MaintenanceKind::Prune,
        "check" => MaintenanceKind::Check,
        other => panic!("the scenario starts a {other}, which these steps don't"),
    }
}

/// An engine that reads each repository's ID from its location, lists no
/// snapshots, and holds each prune and check until its repository is let
/// go.
#[derive(Default)]
struct Held {
    releases: Mutex<BTreeMap<String, Arc<Notify>>>,
    ran: Mutex<Vec<(String, String)>>,
}

impl Held {
    fn release_of(&self, location: &str) -> Arc<Notify> {
        let mut releases = self.releases.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(releases.entry(location.to_owned()).or_default())
    }

    fn ran(&self, kind: &str, location: &str) -> bool {
        self.ran
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|(ran, at)| ran == kind && at == location)
    }
}

struct Nothing;

#[async_trait]
impl JobControl for Nothing {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Held {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let (result, held) = match &job {
            Job::Identify(access) => (
                EngineResult::Repository {
                    id: id_at(&access.repository),
                },
                None,
            ),
            Job::Ls(_) => (EngineResult::Snapshots(Vec::new()), None),
            Job::Prune(job) => (
                EngineResult::Prune(PruneSummary {
                    repository: id_at(&job.repository.repository),
                    packs_marked: 0,
                    packs_deleted: 0,
                    packs_recovered: 0,
                }),
                Some(("prune", job.repository.repository.clone())),
            ),
            Job::Check(job) => (
                EngineResult::Check(CheckSummary {
                    repository: id_at(&job.repository.repository),
                    errors: 0,
                    warnings: 0,
                }),
                Some(("check", job.repository.repository.clone())),
            ),
            _ => {
                return Err(EngineError::Unavailable {
                    detail: "these steps run reads, prunes and checks only".to_owned(),
                });
            }
        };
        let release = held.map(|(kind, at)| {
            self.ran
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((kind.to_owned(), at.clone()));
            self.release_of(&at)
        });
        let started = EngineEvent::Started {
            index_read_at: Timestamp::from_second(1_800_000_000).expect("a time"),
            repository: None,
        };
        let end = async move {
            if let Some(release) = release {
                release.notified().await;
            }
            EngineEvent::Succeeded(result)
        };
        Ok(RunningJob {
            events: stream::iter([started]).chain(stream::once(end)).boxed(),
            control: Box::new(Nothing),
        })
    }
}

struct Unused;

#[async_trait]
impl NormalizeSources for Unused {
    async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
        Ok(sources.to_vec())
    }
}

fn use_case(agent: &mut Agent) -> Arc<RunMaintenance> {
    let queue = agent.maintenance_queue.queue();
    let world = &agent.maintenance_queue;
    Arc::new(
        RunMaintenance::new(
            Arc::clone(&world.engine) as Arc<dyn Engine>,
            agent.clock.clone(),
            agent.store.clone(),
            app::secrets::ResolveRepository::new(Arc::new(FakeSecrets::default())),
            Arc::new(FakeRetention),
            Arc::clone(&world.config) as Arc<dyn ConfigStore>,
            Arc::default(),
            super::forget_steps::self_agent(),
            super::forget_steps::MACHINE.to_owned(),
            Arc::new(Unused),
        )
        .with_queue(queue),
    )
}

/// Lets the runs go on until `done` holds of the run `id` as the log has
/// it, or fails the step.
async fn until(
    agent: &Agent,
    id: OperationId,
    what: &str,
    done: impl Fn(&OperationStatus) -> bool,
) {
    for _ in 0..PATIENCE {
        let status = agent
            .store
            .get(id)
            .await
            .expect("the log answers")
            .map(|detail| detail.record.status);
        if status.as_ref().is_some_and(&done) {
            return;
        }
        tokio::task::yield_now().await;
    }
    let status = agent.store.get(id).await.expect("the log answers");
    panic!(
        "the run is not {what}: {:?}",
        status.map(|detail| detail.record.status)
    );
}

/// Lets the runs go on for a while, for a step that checks that something
/// didn't happen.
async fn settle() {
    for _ in 0..PATIENCE {
        tokio::task::yield_now().await;
    }
}

#[given(expr = "the repositories {string} and {string} are maintained by this agent")]
async fn two_repositories(agent: &mut Agent, first: String, second: String) {
    let world = &mut agent.maintenance_queue;
    for name in [first, second] {
        world.repositories.insert(
            alias(&name),
            RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    repository: location(&name),
                    repo_hot: None,
                    password: SecretRef::Inline(SecretValue::new("demo-password")),
                    options: Default::default(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                },
                maintenance: Maintenance::Local,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        );
    }
    world.save().await;
}

#[given(expr = "the agent runs {int} maintenance operation(s) at a time")]
async fn maintenance_limit(agent: &mut Agent, limit: i64) {
    let world = &mut agent.maintenance_queue;
    world.limit = Concurrency::new(limit).expect("a limit");
    world.save().await;
}

#[when(expr = "a {word} of the repository {string} is started")]
async fn started(agent: &mut Agent, kind: String, repository: String) {
    let use_case = use_case(agent);
    let world = &mut agent.maintenance_queue;
    world.made += 1;
    let id = OperationId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 + world.made,
    ));
    let assessment = use_case
        .assess(id, &alias(&repository), kind_of(&kind))
        .await
        .expect("the run is assessed");
    match use_case.queue(assessment).await {
        Ok(Queued::Turn(turn)) => {
            let prepared = use_case
                .start_turn(turn)
                .await
                .expect("the run whose turn came starts");
            tokio::spawn(async move {
                use_case
                    .execute(prepared, futures::future::pending::<()>())
                    .await;
            });
        }
        Ok(Queued::Waiting(waiting)) => {
            let (cancel, canceled) = oneshot::channel::<()>();
            world.cancels.insert(id, cancel);
            tokio::spawn(async move {
                use_case
                    .run_waiting(waiting, async move {
                        let _ = canceled.await;
                    })
                    .await;
            });
        }
        Err(error) => {
            world.refused = Some((id, error));
            return;
        }
    }
    world.runs.insert((kind, repository), id);
}

#[when(expr = "the {word} of the repository {string} finishes")]
async fn finishes(agent: &mut Agent, kind: String, repository: String) {
    let id = agent.maintenance_queue.run(&kind, &repository);
    agent
        .maintenance_queue
        .engine
        .release_of(&location(&repository))
        .notify_one();
    until(agent, id, "finished", OperationStatus::is_finished).await;
}

#[when(expr = "the {word} of the repository {string} is canceled")]
async fn canceled(agent: &mut Agent, kind: String, repository: String) {
    let id = agent.maintenance_queue.run(&kind, &repository);
    let cancel = agent
        .maintenance_queue
        .cancels
        .remove(&id)
        .expect("the run waits for its turn");
    let _ = cancel.send(());
    until(agent, id, "canceled", |status| {
        *status == OperationStatus::Canceled
    })
    .await;
}

#[when(expr = "this agent stops maintaining the repository {string}")]
async fn stops_maintaining(agent: &mut Agent, repository: String) {
    let world = &mut agent.maintenance_queue;
    world
        .repositories
        .get_mut(&alias(&repository))
        .expect("the scenario has the repository")
        .maintenance = Maintenance::None;
    world.save().await;
}

#[when("a restart holds the maintenance queue")]
fn restart_holds(agent: &mut Agent) {
    agent.maintenance_queue.queue().hold();
}

#[then(expr = "the {word} of the repository {string} is running")]
async fn is_running(agent: &mut Agent, kind: String, repository: String) {
    let id = agent.maintenance_queue.run(&kind, &repository);
    until(agent, id, "running", |status| {
        *status == OperationStatus::Running
    })
    .await;
}

#[then(expr = "the {word} of the repository {string} is waiting for its turn")]
async fn is_waiting(agent: &mut Agent, kind: String, repository: String) {
    let id = agent.maintenance_queue.run(&kind, &repository);
    settle().await;
    let status = agent
        .store
        .get(id)
        .await
        .expect("the log answers")
        .map(|detail| detail.record.status);
    assert_eq!(status, Some(OperationStatus::Queued));
    assert!(
        !agent
            .maintenance_queue
            .engine
            .ran(&kind, &location(&repository)),
        "the engine ran the waiting {kind}"
    );
}

#[then(expr = "the history shows the {word} of the repository {string} as {word}")]
async fn history_shows(agent: &mut Agent, kind: String, repository: String, status: String) {
    let id = agent.maintenance_queue.run(&kind, &repository);
    let expected = match status.as_str() {
        "queued" => OperationStatus::Queued,
        "succeeded" => OperationStatus::Succeeded,
        "canceled" => OperationStatus::Canceled,
        other => panic!("the scenario expects a status these steps don't know: {other}"),
    };
    until(agent, id, &status, |found| *found == expected).await;
}

#[then(
    expr = "the history shows the {word} of the repository {string} as failed with the code {string}"
)]
async fn history_shows_failed(agent: &mut Agent, kind: String, repository: String, code: String) {
    let id = agent.maintenance_queue.run(&kind, &repository);
    until(
        agent,
        id,
        "failed",
        |status| matches!(status, OperationStatus::Failed { reason } if reason.code() == code),
    )
    .await;
}

#[then(expr = "the engine never ran a {word} of the repository {string}")]
async fn never_ran(agent: &mut Agent, kind: String, repository: String) {
    settle().await;
    assert!(
        !agent
            .maintenance_queue
            .engine
            .ran(&kind, &location(&repository)),
        "the engine ran the {kind}"
    );
}

#[then(expr = "the start is refused, naming the {word} of the repository {string}")]
fn refused_naming(agent: &mut Agent, kind: String, repository: String) {
    let going = agent.maintenance_queue.run(&kind, &repository);
    let (_, error) = agent
        .maintenance_queue
        .refused
        .as_ref()
        .expect("a start was refused");
    assert_eq!(*error, MaintenanceError::Busy { running: going });
}

#[then("the history doesn't list the refused prune")]
async fn refused_unlisted(agent: &mut Agent) {
    let (id, _) = agent
        .maintenance_queue
        .refused
        .as_ref()
        .expect("a start was refused");
    let id = *id;
    assert!(
        agent
            .store
            .get(id)
            .await
            .expect("the log answers")
            .is_none(),
        "the refused run is in the log"
    );
}

#[then(expr = "{int} maintenance operation(s) is/are counted as running")]
async fn counted_running(agent: &mut Agent, count: usize) {
    settle().await;
    assert_eq!(agent.maintenance_queue.queue().running(), count);
}
