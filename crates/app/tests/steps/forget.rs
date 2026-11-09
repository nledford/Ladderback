//! Steps for `features/maintenance/forget.feature`: they drive the
//! maintenance use case against a scripted engine, which answers an
//! identify, a listing and a forget, and records what it was asked to
//! remove. The retention evaluation is the `FakeRetention` port.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{
    AgentId, OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid,
};
use futures::StreamExt;
use futures::stream;
use tokio::sync::Notify;

use super::Agent;
use app::fakes::{FakeRetention, InMemoryConfigStore};
use app::maintenance::{MaintenanceError, MaintenanceKind, RunMaintenance};
use app::ports::{
    ConfigStore, Engine, EngineError, EngineEvent, EngineResult, ForgetSummary, Job, JobControl,
    OperationStore, RunningJob, SnapshotSummary, Start,
};

/// The alias every forget scenario's repository goes under.
fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

/// The agent this world's plans and snapshots belong to.
fn self_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0001))
}

/// Another agent, whose history a forget must leave alone.
fn other_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_00aa))
}

fn operation_id(low: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_0000 | low,
    ))
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + second).expect("a time in range")
}

/// What the forget scenarios set up and what the runs answered.
pub struct ForgetWorld {
    /// The configuration store the use case loads: the repository and its
    /// plans.
    store: Arc<InMemoryConfigStore>,
    /// The configuration as drafted: rebuilt and saved on every step that
    /// adds to it.
    repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
    plans: Vec<Plan>,
    /// How many snapshots the script holds, for distinct IDs and times.
    made: u64,
    /// The listing the scripted engine answers with, newest first.
    snapshots: Vec<SnapshotSummary>,
    /// Every job the scripted engine was given, oldest first.
    jobs: Arc<Mutex<Vec<Job>>>,
    /// Releases a forget the engine holds, for the slot scenarios.
    release: Arc<Notify>,
    /// What a refused run answered.
    refused: Option<MaintenanceError>,
    /// The repository's ID, as the scripted identify reports it.
    repository_id: RepositoryId,
}

impl Default for ForgetWorld {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryConfigStore::new()),
            repositories: BTreeMap::new(),
            plans: Vec::new(),
            made: 0,
            snapshots: Vec::new(),
            jobs: Arc::new(Mutex::new(Vec::new())),
            release: Arc::new(Notify::new()),
            refused: None,
            repository_id: RepositoryId::parse(&"07".repeat(32)).expect("a repository ID"),
        }
    }
}

impl ForgetWorld {
    /// The engine the forget scenarios talk to: it answers an identify, a
    /// listing and a forget, holding each forget until it is released.
    pub fn engine(&self) -> Arc<dyn Engine> {
        Arc::new(Scripted {
            snapshots: self.snapshots.clone(),
            jobs: Arc::clone(&self.jobs),
            release: Arc::clone(&self.release),
            repository_id: self.repository_id.clone(),
        })
    }

    /// Saves the draft as it stands.
    async fn save(&self) {
        let repositories = self.repositories.clone();
        let plans = self.plans.clone();
        let config = AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            repositories,
            plans,
        )
        .expect("the draft is valid");
        self.store.save(&config).await.expect("the draft saves");
    }

    /// Adds the repository every forget scenario runs on.
    async fn repository(&mut self) {
        self.repositories.insert(
            alias(),
            RepositoryConfig {
                settings: RepositorySettings {
                    repository: "/srv/demo/repo".to_owned(),
                    repo_hot: None,
                    password: SecretRef::Inline(SecretValue::new("demo-password")),
                    options: Default::default(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                },
                maintenance: Maintenance::default(),
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        );
        self.save().await;
    }

    /// Adds a plan with a `keep-last` policy, or none when `keep_last` is
    /// `None`.
    async fn plan(&mut self, id: &str, keep_last: Option<i64>) {
        let mut plan = Plan::new(
            PlanId::parse(id).expect("a plan ID"),
            alias(),
            vec![domain::planning::Source::parse("/home/demo").expect("a source")],
        );
        if let Some(keep) = keep_last {
            plan.retention.keep_last =
                Some(domain::planning::KeepCount::new(keep).expect("a count"));
        }
        self.plans.push(plan);
        self.save().await;
    }

    /// Adds `count` snapshots carrying the plan tag of `plan` and the owning
    /// agent's tag, or no plan tag at all when `plan` is `None`. The first
    /// added is the newest.
    async fn snapshots(&mut self, count: u64, plan: Option<(String, AgentId)>) {
        for _ in 0..count {
            self.made += 1;
            let tags = match &plan {
                Some((plan, agent)) => vec![
                    format!("{PLAN_TAG_PREFIX}{plan}"),
                    format!("{AGENT_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
                ],
                None => Vec::new(),
            };
            self.snapshots.push(SnapshotSummary {
                id: SnapshotId::parse(&format!("{:064x}", self.made)).expect("a snapshot ID"),
                time: at(-i64::try_from(self.made * 3_600).expect("an hour")),
                hostname: "demo-host".to_owned(),
                label: String::new(),
                tags,
                paths: vec!["/home/demo".to_owned()],
            });
        }
        self.snapshots
            .sort_by_key(|snapshot| std::cmp::Reverse(snapshot.time));
    }

    /// The IDs of the snapshots the plan `plan` owns, newest first.
    fn plan_snapshots(&self, plan: &str) -> Vec<SnapshotId> {
        self.snapshots
            .iter()
            .filter(|snapshot| {
                snapshot
                    .tags
                    .iter()
                    .any(|tag| tag == &format!("{PLAN_TAG_PREFIX}{plan}"))
                    && snapshot.tags.iter().any(|tag| {
                        tag == &format!("{AGENT_TAG_PREFIX}{}", self_agent().as_uuid().hyphenated())
                    })
            })
            .map(|snapshot| snapshot.id.clone())
            .collect()
    }

    /// The IDs the engine was asked to remove, or none when it never was.
    fn removed(&self) -> Vec<SnapshotId> {
        self.jobs
            .lock()
            .expect("the job list answers")
            .iter()
            .filter_map(|job| match job {
                Job::Forget(job) => Some(job.ids.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// The kinds of job the engine was given, oldest first.
    fn jobs(&self) -> Vec<&'static str> {
        self.jobs
            .lock()
            .expect("the job list answers")
            .iter()
            .map(|job| match job {
                Job::Identify(_) => "identify",
                Job::Ls(_) => "ls",
                Job::Forget(_) => "forget",
                Job::Prune(_) => "prune",
                Job::Check(_) => "check",
                Job::Fake(_) => "fake",
                Job::Init(_) => "init",
                Job::Backup(_) => "backup",
            })
            .collect()
    }
}

/// The engine the forget scenarios talk to: each identify answers the
/// scripted repository ID, each listing the scripted snapshots, and each
/// forget records its IDs and succeeds — after `release` fires, so a run
/// can be held.
struct Scripted {
    snapshots: Vec<SnapshotSummary>,
    jobs: Arc<Mutex<Vec<Job>>>,
    release: Arc<Notify>,
    repository_id: RepositoryId,
}

struct Nothing;

#[async_trait]
impl JobControl for Nothing {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        self.jobs
            .lock()
            .expect("the job list answers")
            .push(job.clone());
        let started = EngineEvent::Started {
            index_read_at: at(0),
        };
        let events: Vec<EngineEvent> = match &job {
            Job::Identify(_) => vec![EngineEvent::Succeeded(EngineResult::Repository {
                id: self.repository_id.clone(),
            })],
            Job::Ls(_) => vec![EngineEvent::Succeeded(EngineResult::Snapshots(
                self.snapshots.clone(),
            ))],
            Job::Forget(job) => vec![EngineEvent::Succeeded(EngineResult::Forget(
                ForgetSummary {
                    repository: self.repository_id.clone(),
                    removed: job.ids.clone(),
                },
            ))],
            _ => Vec::new(),
        };
        // Everything but the result flows at once; a forget's result waits
        // for the release, so a scenario can hold a run on purpose. A read
        // (identify, listing) answers at once: `prepare` runs them before
        // anything holds the repository.
        let hold = matches!(job, Job::Forget(_));
        let release = Arc::clone(&self.release);
        let stream = stream::iter(vec![started]).chain(stream::iter(events).then(move |event| {
            let release = Arc::clone(&release);
            async move {
                if hold && matches!(event, EngineEvent::Succeeded(_)) {
                    release.notified().await;
                }
                event
            }
        }));
        Ok(RunningJob {
            events: stream.boxed(),
            control: Box::new(Nothing),
        })
    }
}

/// The maintenance use case wired to the scenario's own pieces.
fn use_case(agent: &Agent) -> Arc<RunMaintenance> {
    Arc::new(RunMaintenance::new(
        agent.forget.engine(),
        agent.clock.clone(),
        agent.store.clone(),
        app::secrets::ResolveRepository::new(Arc::new(app::fakes::FakeSecrets::default())),
        Arc::new(FakeRetention),
        agent.forget.store.clone(),
        self_agent(),
    ))
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s)")]
async fn plan_keeping_last(agent: &mut Agent, plan: String, keep: i64) {
    agent.forget.repository().await;
    agent.forget.plan(&plan, Some(keep)).await;
}

#[given(expr = "a plan {string} keeping nothing")]
async fn plan_keeping_nothing(agent: &mut Agent, plan: String) {
    agent.forget.repository().await;
    agent.forget.plan(&plan, Some(0)).await;
}

#[given(expr = "a plan {string} with no policy set")]
async fn plan_without_policy(agent: &mut Agent, plan: String) {
    agent.forget.repository().await;
    agent.forget.plan(&plan, None).await;
}

#[given(expr = "the repository holds {int} snapshots of the plan {string}")]
async fn snapshots_of_plan(agent: &mut Agent, count: u64, plan: String) {
    agent
        .forget
        .snapshots(count, Some((plan, self_agent())))
        .await;
}

#[given(expr = "the repository holds {int} snapshots no plan owns")]
async fn untagged_snapshots(agent: &mut Agent, count: u64) {
    agent.forget.snapshots(count, None).await;
}

#[given(expr = "the repository holds {int} snapshots another agent's plan {string} holds")]
async fn another_agents_snapshots(agent: &mut Agent, count: u64, plan: String) {
    agent
        .forget
        .snapshots(count, Some((plan, other_agent())))
        .await;
}

#[given("a maintenance operation is already running on the repository")]
async fn maintenance_running(agent: &mut Agent) {
    let mut operation = Operation::queue(
        operation_id(0xbb),
        OperationKind::Prune,
        Trigger::Manual,
        Target {
            plan: None,
            repository: Some(agent.forget.repository_id.clone()),
        },
        at(0),
    );
    operation.start(at(100)).expect("a queued operation starts");
    let decided = agent
        .store
        .try_start(&operation)
        .await
        .expect("the log answers");
    assert_eq!(
        decided,
        Start::Started,
        "the other operation takes the slot"
    );
}

#[when("the owner forgets the repository")]
async fn forget(agent: &mut Agent) {
    let use_case = use_case(agent);
    let prepared = use_case
        .prepare(operation_id(0xcc), &alias(), MaintenanceKind::Forget)
        .await;
    match prepared {
        Ok(prepared) => {
            let runner = Arc::clone(&use_case);
            let running =
                tokio::spawn(
                    async move { runner.execute(prepared, futures::future::pending()).await },
                );
            // The engine records the forget job before its result is held,
            // so the release reaches a waiter that is registered.
            wait_for_forget(agent).await;
            agent.forget.release.notify_waiters();
            agent.finished = Some(running.await.expect("the run ends"));
        }
        Err(error) => agent.forget.refused = Some(error),
    }
}

#[when("the owner forgets the repository and a prune is asked while it runs")]
async fn forget_then_prune(agent: &mut Agent) {
    let use_case = use_case(agent);
    let prepared = use_case
        .prepare(operation_id(0xdd), &alias(), MaintenanceKind::Forget)
        .await
        .expect("the repository is free");
    let runner = Arc::clone(&use_case);
    let running =
        tokio::spawn(async move { runner.execute(prepared, futures::future::pending()).await });
    // The engine records the forget job before its result is held.
    wait_for_forget(agent).await;
    let refused = use_case
        .prepare(operation_id(0xee), &alias(), MaintenanceKind::Prune)
        .await;
    agent.forget.refused = refused.err();
    agent.forget.release.notify_waiters();
    agent.finished = Some(running.await.expect("the run ends"));
}

/// Waits until the scripted engine was given a forget job.
async fn wait_for_forget(agent: &Agent) {
    for _ in 0..400 {
        if agent.forget.jobs().contains(&"forget") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the engine was never given the forget job");
}

#[then(expr = "the repository keeps the {int} newest snapshots of the plan {string}")]
async fn keeps_newest(agent: &mut Agent, keep: usize, plan: String) {
    let removed = agent.forget.removed();
    let of_plan = agent.forget.plan_snapshots(&plan);
    let (keeps, lets_go) = of_plan.split_at(keep);
    assert!(
        removed
            .iter()
            .filter(|id| of_plan.contains(id))
            .all(|id| lets_go.contains(id)),
        "only what the policy lets go is removed: removed {removed:?} of {lets_go:?}"
    );
    assert!(
        keeps.iter().all(|id| !removed.contains(id)),
        "the newest {keep} are kept: {keeps:?}"
    );
    let removed_of_plan: Vec<_> = removed.iter().filter(|id| of_plan.contains(id)).collect();
    assert_eq!(
        removed_of_plan.len(),
        lets_go.len(),
        "everything else of the plan goes"
    );
}

#[then(expr = "the repository keeps the newest snapshot of the plan {string}")]
async fn keeps_newest_one(agent: &mut Agent, plan: String) {
    keeps_newest(agent, 1, plan).await;
}

#[then(expr = "the repository still holds the {int} snapshots no plan owns")]
async fn untagged_survive(agent: &mut Agent, count: usize) {
    let removed = agent.forget.removed();
    let untagged: Vec<SnapshotId> = agent
        .forget
        .snapshots
        .iter()
        .filter(|snapshot| {
            !snapshot
                .tags
                .iter()
                .any(|tag| tag.starts_with(PLAN_TAG_PREFIX))
        })
        .map(|snapshot| snapshot.id.clone())
        .collect();
    assert_eq!(untagged.len(), count);
    assert!(
        removed.iter().all(|id| !untagged.contains(id)),
        "no untagged snapshot is removed: {removed:?}"
    );
}

#[then(expr = "the repository still holds the {int} snapshots the other agent's plan holds")]
async fn other_agents_survive(agent: &mut Agent, count: usize) {
    let removed = agent.forget.removed();
    let own = format!("{AGENT_TAG_PREFIX}{}", self_agent().as_uuid().hyphenated());
    let other: Vec<SnapshotId> = agent
        .forget
        .snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.tags.iter().any(|tag| {
                tag.starts_with(AGENT_TAG_PREFIX)
                    && tag != &own
                    && snapshot
                        .tags
                        .iter()
                        .any(|tag| tag.starts_with(PLAN_TAG_PREFIX))
            })
        })
        .map(|snapshot| snapshot.id.clone())
        .collect();
    assert_eq!(other.len(), count);
    assert!(
        removed.iter().all(|id| !other.contains(id)),
        "no snapshot of another agent's plan is removed: {removed:?}"
    );
}

#[then(expr = "the repository still holds the {int} snapshots of the plan {string}")]
async fn all_of_plan_survive(agent: &mut Agent, count: usize, plan: String) {
    let removed = agent.forget.removed();
    let of_plan = agent.forget.plan_snapshots(&plan);
    assert_eq!(of_plan.len(), count);
    assert!(
        removed.iter().all(|id| !of_plan.contains(id)),
        "nothing of the plan is removed: {removed:?}"
    );
}

#[then("the forget is refused because the plan's policy keeps nothing")]
fn refused_keeps_nothing(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the forget was refused");
    assert_eq!(
        error,
        MaintenanceError::KeepsNothing {
            plan: PlanId::parse("home").expect("a plan")
        }
    );
}

#[then("the forget is refused while the other operation runs")]
fn refused_busy(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the forget was refused");
    assert!(matches!(error, MaintenanceError::Busy { .. }), "{error:?}");
}

#[then("the prune is refused while the forget holds the repository")]
fn prune_refused(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the prune was refused");
    assert!(matches!(error, MaintenanceError::Busy { .. }), "{error:?}");
}

#[then("the engine was never asked to remove anything")]
fn no_forget_job(agent: &mut Agent) {
    assert!(
        !agent.forget.jobs().contains(&"forget"),
        "the engine ran: {:?}",
        agent.forget.jobs()
    );
}
