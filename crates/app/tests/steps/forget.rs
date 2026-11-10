//! Steps for `features/maintenance/forget.feature`: they drive the
//! maintenance use case against a scripted engine, which answers an
//! identify, a listing and a forget, and records what it was asked to
//! remove. The retention evaluation is the `FakeRetention` port.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::OperationStatus;
use domain::execution::plan_tags;
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
use app::maintenance::{
    MaintenanceAssessment, MaintenanceError, MaintenanceKind, PreparedMaintenance, RunMaintenance,
};
use app::ports::{
    CheckSummary, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, ForgetSummary, Job,
    JobControl, MAX_JOB_IDS, NormalizeSources, OperationStore, PrunePlanSummary, PruneSummary,
    RepositoryAccess, Retention, RetentionError, RewriteSummary, RunningJob, SnapshotSummary,
    SourcesError, Start,
};
use domain::maintenance::Snapshot;
use domain::planning::{RetentionPolicy, Source};

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

/// How a scenario makes the scripted engine fail a maintenance read.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ReadFailure {
    #[default]
    None,
    /// `run` itself refuses: the engine cannot be reached.
    Unreachable,
    /// The listing answers a failure event.
    Listing,
    /// The repository never answers: the identify and the listing start and
    /// say nothing more.
    Silent,
    /// The repository lists its snapshots, and a read of them for a plan's
    /// retention never answers.
    RetentionSilent,
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
    /// How the scripted engine fails a read, when a scenario asks for it.
    fail: ReadFailure,
    /// The repository's ID, as the scripted identify reports it.
    repository_id: RepositoryId,
    /// Which forget or rewrite job (counting from 0) the engine holds until
    /// released: the first, unless a scenario says otherwise.
    hold_at: Option<usize>,
    /// Which forget or rewrite job the engine fails, when a scenario says.
    fail_at: Option<usize>,
    /// The run a scenario assessed and has not started.
    assessed: Option<MaintenanceAssessment>,
    /// The folder whose normalization the sources port refuses, when a
    /// scenario says: a drive that is not mounted.
    unreadable_folder: Option<String>,
    /// How many times the sources port was asked to normalize a plan's
    /// sources.
    source_reads: Arc<AtomicUsize>,
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
            fail: ReadFailure::None,
            repository_id: RepositoryId::parse(&"07".repeat(32)).expect("a repository ID"),
            hold_at: Some(0),
            fail_at: None,
            assessed: None,
            unreadable_folder: None,
            source_reads: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ForgetWorld {
    /// The engine the forget scenarios talk to: it answers an identify, a
    /// listing and a forget, holding each forget until it is released.
    pub fn engine(&self) -> Arc<dyn Engine> {
        Arc::new(Scripted {
            snapshots: self.snapshots.clone(),
            fail: self.fail,
            jobs: Arc::clone(&self.jobs),
            release: Arc::clone(&self.release),
            changes: AtomicUsize::new(0),
            hold_at: self.hold_at,
            fail_at: self.fail_at,
            repository_id: self.repository_id.clone(),
        })
    }

    /// What a refused run answered, for the adoption steps too.
    pub fn set_refused(&mut self, error: Option<MaintenanceError>) {
        self.refused = error;
    }

    /// Takes the refusal a run answered, if it answered one.
    pub fn take_refused(&mut self) -> Option<MaintenanceError> {
        self.refused.take()
    }

    /// The release a held engine run waits for.
    pub fn release(&self) -> &Notify {
        &self.release
    }

    /// How many snapshots the scripted engine lists.
    pub fn listed(&self) -> usize {
        self.snapshots.len()
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
    pub async fn repository(&mut self) {
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

    /// Makes the scripted engine fail reads the given way.
    pub fn fail_read(&mut self, fail: ReadFailure) {
        self.fail = fail;
    }

    /// Makes the sources port refuse every plan that backs up `folder`.
    pub fn refuse_sources_of(&mut self, folder: &str) {
        self.unreadable_folder = Some(folder.to_owned());
    }

    /// Adds a plan as drafted, with whatever policy it carries.
    pub async fn plan_raw(&mut self, plan: Plan) {
        self.plans.push(plan);
        self.save().await;
    }

    /// Adds a plan with a `keep-last` policy, or none when `keep_last` is
    /// `None`.
    pub async fn plan(&mut self, id: &str, keep_last: Option<i64>) {
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
    pub async fn snapshots(&mut self, count: u64, plan: Option<(String, AgentId)>) {
        self.snapshots_of_folder(count, plan, "/home/demo").await;
    }

    /// Adds `count` snapshots of `folder`, tagged as the plan's or none at
    /// all. The first added is the newest.
    pub async fn snapshots_of_folder(
        &mut self,
        count: u64,
        plan: Option<(String, AgentId)>,
        folder: &str,
    ) {
        let tags = match &plan {
            Some((plan, agent)) => vec![
                format!("{PLAN_TAG_PREFIX}{plan}"),
                format!("{AGENT_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
            ],
            None => Vec::new(),
        };
        self.snapshots_tagged(count, &tags, folder);
    }

    /// Adds `count` snapshots of `folder` carrying exactly `tags`. The first
    /// added is the newest.
    pub fn snapshots_tagged(&mut self, count: u64, tags: &[String], folder: &str) {
        for _ in 0..count {
            self.made += 1;
            self.snapshots.push(SnapshotSummary {
                id: SnapshotId::parse(&format!("{:064x}", self.made)).expect("a snapshot ID"),
                time: at(-i64::try_from(self.made * 3_600).expect("an hour")),
                hostname: "demo-host".to_owned(),
                label: String::new(),
                tags: tags.to_vec(),
                paths: vec![folder.to_owned()],
                paths_truncated: false,
                tags_truncated: false,
            });
        }
        self.snapshots
            .sort_by_key(|snapshot| std::cmp::Reverse(snapshot.time));
    }

    /// The IDs of the snapshots carrying no `ladderback-` tag, newest first.
    pub fn untagged_snapshots(&self) -> Vec<SnapshotId> {
        self.snapshots
            .iter()
            .filter(|snapshot| {
                !snapshot
                    .tags
                    .iter()
                    .any(|tag| tag.starts_with(plan_tags::RESERVED_TAG_PREFIX))
            })
            .map(|snapshot| snapshot.id.clone())
            .collect()
    }

    /// The IDs of another agent's snapshots of `plan`, newest first.
    pub fn foreign_snapshots(&self, plan: &str, agent: AgentId) -> Vec<SnapshotId> {
        self.snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.tags.contains(&format!("{PLAN_TAG_PREFIX}{plan}"))
                    && snapshot.tags.contains(&format!(
                        "{AGENT_TAG_PREFIX}{}",
                        agent.as_uuid().hyphenated()
                    ))
            })
            .map(|snapshot| snapshot.id.clone())
            .collect()
    }

    /// What the engine was asked to rewrite, if anything: the IDs of every
    /// rewrite job it was given, in order, and the tags the first one set.
    pub fn rewrite_job(&self) -> Option<(Vec<SnapshotId>, Vec<String>)> {
        let jobs = self.jobs.lock().expect("the job list answers");
        let rewrites: Vec<_> = jobs
            .iter()
            .filter_map(|job| match job {
                Job::Rewrite(job) => Some(job),
                _ => None,
            })
            .collect();
        let first = rewrites.first()?;
        let ids = rewrites.iter().flat_map(|job| job.ids.clone()).collect();
        Some((ids, first.tags.tags().to_vec()))
    }

    /// Makes the engine fail the forget or rewrite job number `at`
    /// (counting from 0).
    pub fn fail_change_at(&mut self, at: usize) {
        self.fail_at = Some(at);
    }

    /// Makes the engine hold the forget or rewrite job number `at` until
    /// released, and none before it.
    pub fn hold_change_at(&mut self, at: usize) {
        self.hold_at = Some(at);
    }

    /// How many forget jobs the engine was given.
    pub fn forget_jobs(&self) -> usize {
        self.jobs().iter().filter(|&&name| name == "forget").count()
    }

    /// The IDs of the snapshots the plan `plan` owns, newest first.
    pub fn plan_snapshots(&self, plan: &str) -> Vec<SnapshotId> {
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
    pub fn removed(&self) -> Vec<SnapshotId> {
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
    pub fn jobs(&self) -> Vec<&'static str> {
        self.jobs
            .lock()
            .expect("the job list answers")
            .iter()
            .map(|job| match job {
                Job::Identify(_) => "identify",
                Job::Ls(_) => "ls",
                Job::Forget(_) => "forget",
                Job::Rewrite(_) => "adopt",
                Job::Prune(_) => "prune",
                Job::PrunePlan(_) => "prune-plan",
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
    fail: ReadFailure,
    jobs: Arc<Mutex<Vec<Job>>>,
    release: Arc<Notify>,
    /// How many forget or rewrite jobs the engine was given so far.
    changes: AtomicUsize,
    hold_at: Option<usize>,
    fail_at: Option<usize>,
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
        if self.fail == ReadFailure::Unreachable && matches!(job, Job::Identify(_) | Job::Ls(_)) {
            return Err(EngineError::Unavailable {
                detail: "the engine is down".to_owned(),
            });
        }
        // Like the real engine, which cannot send a spec past its message
        // cap: a job naming more snapshots than that refuses to start.
        let named = match &job {
            Job::Forget(job) => job.ids.len(),
            Job::Rewrite(job) => job.ids.len(),
            _ => 0,
        };
        if named > MAX_JOB_IDS {
            return Err(EngineError::Unavailable {
                detail: format!("a job names {named} snapshots, over the {MAX_JOB_IDS} it takes"),
            });
        }
        self.jobs
            .lock()
            .expect("the job list answers")
            .push(job.clone());
        // Which of the forgets and rewrites this is, counting from 0.
        let change = matches!(job, Job::Forget(_) | Job::Rewrite(_))
            .then(|| self.changes.fetch_add(1, Ordering::SeqCst));
        if self.fail == ReadFailure::Silent && matches!(job, Job::Identify(_) | Job::Ls(_)) {
            return Ok(RunningJob {
                events: stream::pending().boxed(),
                control: Box::new(Nothing),
            });
        }
        let started = EngineEvent::Started {
            index_read_at: at(0),
        };
        let events: Vec<EngineEvent> = match &job {
            Job::Forget(_) | Job::Rewrite(_) if change.is_some() && change == self.fail_at => {
                vec![EngineEvent::Failed(
                    domain::execution::FailureReason::EngineFailed {
                        detail: "the forget failed".to_owned(),
                    },
                )]
            }
            Job::Identify(_) => vec![EngineEvent::Succeeded(EngineResult::Repository {
                id: self.repository_id.clone(),
            })],
            Job::Ls(_) if self.fail == ReadFailure::Listing => vec![EngineEvent::Failed(
                domain::execution::FailureReason::EngineFailed {
                    detail: "the listing failed".to_owned(),
                },
            )],
            Job::Ls(_) => vec![EngineEvent::Succeeded(EngineResult::Snapshots(
                self.snapshots.clone(),
            ))],
            Job::Forget(job) => vec![EngineEvent::Succeeded(EngineResult::Forget(
                ForgetSummary {
                    repository: self.repository_id.clone(),
                    removed: job.ids.clone(),
                },
            ))],
            Job::Rewrite(job) => vec![EngineEvent::Succeeded(EngineResult::Rewrite(
                RewriteSummary {
                    repository: self.repository_id.clone(),
                    created: job.ids.clone(),
                },
            ))],
            Job::Prune(_) => vec![EngineEvent::Succeeded(EngineResult::Prune(PruneSummary {
                repository: self.repository_id.clone(),
                packs_marked: 0,
                packs_deleted: 0,
                packs_recovered: 0,
            }))],
            Job::PrunePlan(_) => vec![EngineEvent::Succeeded(EngineResult::PrunePlan(
                PrunePlanSummary {
                    repository: self.repository_id.clone(),
                    packs_marked: 2,
                    packs_deleted: 1,
                    packs_recovered: 3,
                    bytes_deleted: 4096,
                },
            ))],
            Job::Check(_) => vec![EngineEvent::Succeeded(EngineResult::Check(CheckSummary {
                repository: self.repository_id.clone(),
                errors: 0,
                warnings: 0,
            }))],
            _ => Vec::new(),
        };
        // Everything but the result flows at once; a forget's result waits
        // for the release, so a scenario can hold a run on purpose. A read
        // (identify, listing) answers at once: `assess` runs them before
        // anything holds the repository.
        let hold = change.is_some() && change == self.hold_at;
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
pub fn use_case(agent: &Agent) -> Arc<RunMaintenance> {
    Arc::new(
        agent
            .announcement
            .listening_maintenance(RunMaintenance::new(
                agent.forget.engine(),
                agent.clock.clone(),
                agent.store.clone(),
                app::secrets::ResolveRepository::new(Arc::new(app::fakes::FakeSecrets::default())),
                match agent.forget.fail {
                    ReadFailure::RetentionSilent => Arc::new(NeverAnswers),
                    _ => Arc::new(FakeRetention),
                },
                agent.forget.store.clone(),
                self_agent(),
                "demo-host".to_owned(),
                Arc::new(ScriptedSources {
                    unreadable_folder: agent.forget.unreadable_folder.clone(),
                    reads: Arc::clone(&agent.forget.source_reads),
                }),
            )),
    )
}

/// Assesses a maintenance run and starts it on this one future, which no
/// scenario drops: the route keeps the two apart for a client that hangs up.
pub async fn assess_and_start(
    use_case: &RunMaintenance,
    operation: OperationId,
    kind: MaintenanceKind,
) -> Result<PreparedMaintenance, MaintenanceError> {
    let assessment = use_case.assess(operation, &alias(), kind).await?;
    use_case.start(assessment).await
}

/// A retention port whose read never answers: the repository listed its
/// snapshots and then stalled.
struct NeverAnswers;

#[async_trait]
impl Retention for NeverAnswers {
    async fn removes(
        &self,
        _repository: &RepositoryAccess,
        _policy: &RetentionPolicy,
        _group: &[Snapshot],
    ) -> Result<Vec<SnapshotId>, RetentionError> {
        std::future::pending().await
    }
}

/// The sources as written, as the scripted listing reports them, except a
/// plan that backs up the unreadable folder: its sources can't be read.
struct ScriptedSources {
    unreadable_folder: Option<String>,
    reads: Arc<AtomicUsize>,
}

#[async_trait]
impl NormalizeSources for ScriptedSources {
    async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        match &self.unreadable_folder {
            Some(folder) if sources.iter().any(|source| source.as_str() == folder) => {
                Err(SourcesError::Unreadable {
                    detail: "no such folder".to_owned(),
                })
            }
            _ => Ok(sources.to_vec()),
        }
    }
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

#[given(expr = "a plan {string} keeping nothing that has never backed up")]
async fn plan_keeping_nothing_never_backed_up(agent: &mut Agent, plan: String) {
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

#[given("the engine cannot be reached")]
async fn engine_unreachable(agent: &mut Agent) {
    agent.forget.fail_read(ReadFailure::Unreachable);
}

#[given("the repository never answers")]
async fn repository_silent(agent: &mut Agent) {
    agent.forget.fail_read(ReadFailure::Silent);
}

#[given(expr = "the sources of the folder {string} cannot be read")]
async fn sources_unreadable(agent: &mut Agent, folder: String) {
    agent.forget.refuse_sources_of(&folder);
}

#[given("the repository lists its snapshots but never answers a read of them for retention")]
async fn retention_silent(agent: &mut Agent) {
    agent.forget.fail_read(ReadFailure::RetentionSilent);
}

#[given("the engine fails the second forget job")]
fn second_forget_fails(agent: &mut Agent) {
    agent.forget.fail_change_at(1);
}

#[given("the engine holds the second forget job")]
fn second_forget_held(agent: &mut Agent) {
    agent.forget.hold_change_at(1);
}

#[given("the repository's listing fails")]
async fn listing_fails(agent: &mut Agent) {
    agent.forget.fail_read(ReadFailure::Listing);
}

#[given(expr = "a plan {string} keeping within {int} day(s)")]
async fn plan_keeping_within(agent: &mut Agent, plan: String, days: i64) {
    agent.forget.repository().await;
    let mut draft = Plan::new(
        PlanId::parse(&plan).expect("a plan ID"),
        alias(),
        vec![domain::planning::Source::parse("/home/demo").expect("a source")],
    );
    draft.retention.keep_within =
        Some(domain::planning::KeepWithin::parse(&format!("{days} day")).expect("a span"));
    agent.forget.plan_raw(draft).await;
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
    // The scenario's clock jumps to a timer's deadline, so an assess that
    // waits on a repository that never answers ends at once; one that does
    // not time out would hang, and is cut off here.
    let prepared = tokio::time::timeout(
        Duration::from_secs(5),
        assess_and_start(&use_case, operation_id(0xcc), MaintenanceKind::Forget),
    )
    .await
    .expect("assess returns instead of waiting for ever");
    match prepared {
        Ok(prepared) => {
            // The prepared run carries the operation the route answered
            // with, and a forget carries what it decided.
            assert_eq!(prepared.operation().id(), operation_id(0xcc));
            assert_eq!(
                prepared
                    .partition()
                    .expect("a forget partitions before it deletes")
                    .len(),
                agent.forget.listed(),
                "the partition sees every snapshot once"
            );
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

#[when("the owner forgets the repository and cancels it while the second job runs")]
async fn forget_then_cancel(agent: &mut Agent) {
    let use_case = use_case(agent);
    let prepared = assess_and_start(&use_case, operation_id(0xcc), MaintenanceKind::Forget)
        .await
        .expect("the repository is free");
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let runner = Arc::clone(&use_case);
    let running = tokio::spawn(async move {
        runner
            .execute(prepared, async move {
                let _ = cancelled.await;
            })
            .await
    });
    // The first job ends at once and the second is held until the cancel.
    for _ in 0..400 {
        if agent.forget.forget_jobs() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(agent.forget.forget_jobs() >= 2, "the second job never ran");
    let _ = cancel.send(());
    agent.finished = Some(running.await.expect("the run ends"));
}

#[when("the owner forgets the repository and a prune is asked while it runs")]
async fn forget_then_prune(agent: &mut Agent) {
    let use_case = use_case(agent);
    let prepared = assess_and_start(&use_case, operation_id(0xdd), MaintenanceKind::Forget)
        .await
        .expect("the repository is free");
    let runner = Arc::clone(&use_case);
    let running =
        tokio::spawn(async move { runner.execute(prepared, futures::future::pending()).await });
    // The engine records the forget job before its result is held.
    wait_for_forget(agent).await;
    let refused = assess_and_start(&use_case, operation_id(0xee), MaintenanceKind::Prune).await;
    agent.forget.refused = refused.err();
    agent.forget.release.notify_waiters();
    agent.finished = Some(running.await.expect("the run ends"));
}

#[when("the owner assesses a forget of the repository")]
async fn assess_a_forget(agent: &mut Agent) {
    let use_case = use_case(agent);
    let assessment = use_case
        .assess(operation_id(0xcc), &alias(), MaintenanceKind::Forget)
        .await
        .expect("the forget is assessed");
    agent.forget.assessed = Some(assessment);
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

#[when("the owner prunes the repository")]
async fn prune(agent: &mut Agent) {
    run_maintenance(agent, MaintenanceKind::Prune).await;
}

#[when("the owner checks the repository")]
async fn check(agent: &mut Agent) {
    run_maintenance(agent, MaintenanceKind::Check).await;
}

#[when("the owner checks the repository after cancelling it")]
async fn check_after_cancel(agent: &mut Agent) {
    let use_case = use_case(agent);
    let prepared = assess_and_start(&use_case, operation_id(0xcc), MaintenanceKind::Check)
        .await
        .expect("the repository is free");
    // The scripted engine's result is ready the moment the job starts.
    agent.finished = Some(use_case.execute(prepared, futures::future::ready(())).await);
}

/// Assesses, starts and runs one maintenance operation, as the route does.
async fn run_maintenance(agent: &mut Agent, kind: MaintenanceKind) {
    let use_case = use_case(agent);
    let prepared = assess_and_start(&use_case, operation_id(0xcc), kind.clone()).await;
    match prepared {
        Ok(prepared) => {
            let runner = Arc::clone(&use_case);
            let running =
                tokio::spawn(
                    async move { runner.execute(prepared, futures::future::pending()).await },
                );
            wait_for_job(agent, job_name(&kind)).await;
            agent.forget.release.notify_waiters();
            agent.finished = Some(running.await.expect("the run ends"));
        }
        Err(error) => agent.forget.refused = Some(error),
    }
}

fn job_name(kind: &MaintenanceKind) -> &'static str {
    match kind {
        MaintenanceKind::Forget => "forget",
        MaintenanceKind::Adopt(_) => "adopt",
        MaintenanceKind::Prune => "prune",
        MaintenanceKind::Check => "check",
    }
}

/// Waits until the scripted engine was given the job of this kind.
async fn wait_for_job(agent: &Agent, name: &str) {
    for _ in 0..400 {
        if agent.forget.jobs().contains(&name) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the engine was never given the {name} job");
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

#[then("a prune of the repository can start")]
async fn a_prune_can_start(agent: &mut Agent) {
    let use_case = use_case(agent);
    let prepared = assess_and_start(&use_case, operation_id(0xee), MaintenanceKind::Prune).await;
    assert!(
        prepared.is_ok(),
        "a repository nothing runs on takes a prune: {:?}",
        prepared.err()
    );
}

#[then("the assessed forget is refused when it starts")]
async fn assessed_forget_refused(agent: &mut Agent) {
    let assessment = agent.forget.assessed.take().expect("a forget was assessed");
    let refused = use_case(agent).start(assessment).await;
    assert!(
        matches!(refused, Err(MaintenanceError::Busy { .. })),
        "the prune holds the repository: {:?}",
        refused.err()
    );
}

#[then("the engine was never asked to remove anything")]
fn no_forget_job(agent: &mut Agent) {
    assert!(
        !agent.forget.jobs().contains(&"forget"),
        "the engine ran: {:?}",
        agent.forget.jobs()
    );
}

#[then("the forget is refused because the engine cannot be reached")]
fn refused_engine(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the forget was refused");
    assert!(
        matches!(error, MaintenanceError::Engine { .. }),
        "an engine failure, not {error:?}"
    );
}

#[then("the forget is refused because the repository did not answer in time")]
fn refused_silent(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the forget was refused");
    assert_eq!(error, MaintenanceError::TimedOut);
}

#[then("the forget is refused because the listing failed")]
fn refused_listing(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the forget was refused");
    let MaintenanceError::Failed(domain::execution::FailureReason::EngineFailed { detail }) = error
    else {
        panic!("the engine's own failure, not {error:?}")
    };
    assert_eq!(detail, "the listing failed");
}

#[then("the forget is refused because the policy is not one the engine accepts")]
fn refused_policy(agent: &mut Agent) {
    let error = agent.forget.refused.take().expect("the forget was refused");
    assert_eq!(
        error,
        MaintenanceError::InvalidRetention {
            plan: PlanId::parse("home").expect("a plan ID")
        },
        "a typed reason, not a sentence"
    );
}

#[then("the engine was asked to prune the repository")]
fn pruned(agent: &mut Agent) {
    assert!(
        agent.forget.jobs().contains(&"prune"),
        "the jobs: {:?}",
        agent.forget.jobs()
    );
}

#[then("the engine was asked to check the repository")]
fn checked(agent: &mut Agent) {
    assert!(
        agent.forget.jobs().contains(&"check"),
        "the jobs: {:?}",
        agent.forget.jobs()
    );
}

#[then(expr = "the log shows the forget removed {int} snapshots")]
async fn log_shows_the_forget(agent: &mut Agent, removed: usize) {
    let detail = agent
        .store
        .get(operation_id(0xcc))
        .await
        .expect("the log answers")
        .expect("the forget is in the log");
    assert_eq!(detail.record.status, OperationStatus::Succeeded);
    let Some(EngineResult::Forget(summary)) = detail.record.result else {
        panic!("the log carries the forget's summary: {:?}", detail.record);
    };
    assert_eq!(summary.removed.len(), removed, "{summary:?}");
}

#[then("no forget job names more snapshots than an engine job takes")]
fn forget_jobs_are_small_enough(agent: &mut Agent) {
    let jobs = agent.forget.jobs.lock().expect("the job list answers");
    let sizes: Vec<usize> = jobs
        .iter()
        .filter_map(|job| match job {
            Job::Forget(job) => Some(job.ids.len()),
            _ => None,
        })
        .collect();
    assert!(!sizes.is_empty(), "the engine was never asked to forget");
    assert!(
        sizes.iter().all(|&size| size <= MAX_JOB_IDS),
        "forget jobs of {sizes:?} snapshots"
    );
}

#[then(expr = "the engine was given {int} forget job(s)")]
fn forget_jobs_given(agent: &mut Agent, count: usize) {
    assert_eq!(
        agent.forget.forget_jobs(),
        count,
        "{:?}",
        agent.forget.jobs()
    );
}

#[then("the log holds no operation of this forget")]
async fn no_operation_of_the_refused_forget(agent: &mut Agent) {
    let found = agent
        .store
        .get(operation_id(0xcc))
        .await
        .expect("the log answers");
    assert_eq!(found, None, "a refused forget leaves nothing in the log");
}
