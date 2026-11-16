//! Steps for `features/restore/restore.feature`: they run the restore use
//! case against a scripted engine of their own, which lists the scenario's
//! snapshot, answers a restore's dry run with the scenario's counts and runs
//! a restore as the scenario says, and note every job it was given.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use app::access::Access;
use app::execution::{Finished, RunOperation};
use app::fakes::{
    FakeClock, FakeSecrets, InMemoryAdminPassword, InMemoryApiTokenStore, InMemoryConfigStore,
    InMemoryOperationStore, InMemorySessionStore, SeededRandom, SimulatedEngine,
};
use app::ports::{
    Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob,
    HistoryQuery, Job, JobControl, OperationStore, PageSize, RestoreJob, RestorePlanSummary,
    RestoreSummary, RestoreTarget, RunningJob, SnapshotSummary,
};
use app::restore::{PreparedRestore, RestoreAsk, RestoreError, RestoreSetup, Restores};
use app::secrets::ResolveRepository;
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::browse::FolderPath;
use domain::execution::{FailureReason, OperationKind, OperationStatus, Trigger};
use domain::notifications::{EventKind, Outcome};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::restore::{Destination, RestoreOrigin, RestoreRefused};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, SignedDuration, SnapshotId, TimeZone, Timestamp, Uuid};
use futures::{StreamExt, stream};

use super::Agent;

/// The agent's restore folder in every untagged scenario.
const RESTORE_FOLDER: &str = "/home/demo/ladderback-restore";

/// How the scripted engine runs a restore.
#[derive(Clone, Copy, Default)]
enum RestoreRun {
    /// It succeeds at once.
    #[default]
    AtOnce,
    /// It counts `steps`, each `step` long, on the scenario's clock.
    Steps { steps: u32, step: SignedDuration },
    /// It can't write its target.
    Unwritable,
}

/// What the restore scenarios set up and what the restores answered.
#[derive(Default)]
pub struct RestoreWorld {
    /// The scenario's snapshot and the host it records.
    snapshot: Option<(SnapshotId, String)>,
    /// The admin password, with sign-in on.
    password: Option<String>,
    allow_hub_restore: bool,
    run: RestoreRun,
    /// What the scripted dry run counts.
    plan: RestorePlanSummary,
    /// Every job the engine was given, oldest first.
    jobs: Arc<Mutex<Vec<Job>>>,
    /// The operation log the restores write to.
    log: Arc<InMemoryOperationStore>,
    /// The use case, made at the first restore so the slot and the sign-in
    /// waits last the scenario.
    restores: Option<Arc<Restores>>,
    /// How each start answered, oldest first.
    started: Vec<Result<OperationId, RestoreError>>,
    /// A restore started and not yet run.
    pending: Option<PreparedRestore>,
    /// A restore started and held unrun for the rest of the scenario, so it
    /// keeps the agent's restore slot.
    held: Option<PreparedRestore>,
    finished: Option<Finished>,
    preview: Option<Result<RestorePlanSummary, RestoreError>>,
    runs: u128,
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

/// The whole ID of the snapshot whose short ID is `short`.
fn snapshot_id(short: &str) -> SnapshotId {
    SnapshotId::parse(&format!("{short}{}", "d".repeat(64 - short.len()))).expect("a snapshot ID")
}

fn path(text: &str) -> FolderPath {
    FolderPath::parse(text).expect("an entry path")
}

/// A machine that starts no worker: the scripted engine needs none.
struct Nothing;

#[async_trait]
impl JobControl for Nothing {
    async fn stop(&mut self) {}
}

/// Lists the scenario's snapshot, answers a dry run with its counts and
/// runs a restore as the scenario says.
struct Scripted {
    snapshot: Option<(SnapshotId, String)>,
    run: RestoreRun,
    plan: RestorePlanSummary,
    jobs: Arc<Mutex<Vec<Job>>>,
    clock: Arc<dyn Clock>,
}

fn answer(event: EngineEvent) -> RunningJob {
    RunningJob {
        events: stream::iter([event]).boxed(),
        control: Box::new(Nothing),
    }
}

impl Scripted {
    /// A restore that starts, as a worker reports reading the index, then
    /// ends as `end`.
    fn started_then(&self, end: EngineEvent) -> RunningJob {
        let started = EngineEvent::Started {
            index_read_at: self.clock.now(),
            repository: None,
        };
        RunningJob {
            events: stream::iter([started, end]).boxed(),
            control: Box::new(Nothing),
        }
    }
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(job.clone());
        Ok(match job {
            Job::Ls(_) => answer(EngineEvent::Succeeded(EngineResult::Snapshots(
                self.snapshot
                    .iter()
                    .map(|(id, host)| SnapshotSummary {
                        id: id.clone(),
                        time: Timestamp::from_second(1_799_000_000).expect("an instant"),
                        offset: domain::Offset::UTC,
                        hostname: host.clone(),
                        label: String::new(),
                        tags: Vec::new(),
                        paths: vec!["/home".to_owned()],
                        paths_truncated: false,
                        tags_truncated: false,
                    })
                    .collect(),
            ))),
            Job::RestorePlan(_) => {
                answer(EngineEvent::Succeeded(EngineResult::RestorePlan(self.plan)))
            }
            Job::Restore(RestoreJob {
                deadline_budget, ..
            }) => match self.run {
                RestoreRun::AtOnce => self.started_then(EngineEvent::Succeeded(
                    EngineResult::Restore(RestoreSummary {
                        files_restored: 6,
                        ..RestoreSummary::default()
                    }),
                )),
                RestoreRun::Unwritable => {
                    self.started_then(EngineEvent::Failed(FailureReason::RestoreTargetUnwritable))
                }
                RestoreRun::Steps { steps, step } => {
                    SimulatedEngine::new(self.clock.clone())
                        .run(
                            op,
                            Job::Fake(FakeJob {
                                steps,
                                step_delay: Duration::try_from(step).expect("a step"),
                                warnings: Vec::new(),
                                end: FakeEnd::Succeed,
                                deadline_budget: Some(deadline_budget),
                            }),
                        )
                        .await?
                }
            },
            other => panic!("a restore asks for no {:?} job", other.kind()),
        })
    }
}

impl RestoreWorld {
    async fn config(&self) -> Arc<InMemoryConfigStore> {
        let store = Arc::new(InMemoryConfigStore::new());
        let repositories = [(
            alias(),
            RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    repository: "/srv/demo/repo".to_owned(),
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
        )]
        .into_iter()
        .collect();
        let config = AgentConfig::new(
            AgentSettings {
                allow_hub_restore: self.allow_hub_restore,
                ..AgentSettings::default()
            },
            Hooks::default(),
            repositories,
            Vec::new(),
        )
        .expect("the configuration is valid");
        store.save(&config).await.expect("the configuration saves");
        store
    }

    /// The use case, made on `clock` the first time it is asked for.
    async fn restores(&mut self, clock: Arc<FakeClock>) -> Arc<Restores> {
        if let Some(restores) = &self.restores {
            return restores.clone();
        }
        let engine: Arc<dyn Engine> = Arc::new(Scripted {
            snapshot: self.snapshot.clone(),
            run: self.run,
            plan: self.plan,
            jobs: self.jobs.clone(),
            clock: clock.clone(),
        });
        let mut restores = Restores::new(
            engine.clone(),
            clock.clone(),
            self.config().await,
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            RunOperation::new(engine, clock.clone(), self.log.clone()),
            RestoreSetup {
                default_folder: RESTORE_FOLDER.to_owned(),
                zone: TimeZone::UTC,
            },
        );
        if let Some(password) = &self.password {
            restores = restores.with_access(Arc::new(Access::new(
                Arc::new(InMemoryAdminPassword::with(password)),
                Arc::new(SeededRandom::new(1)),
                clock,
                Arc::new(InMemoryApiTokenStore::new()),
                Arc::new(InMemorySessionStore::new()),
            )));
        }
        let restores = Arc::new(restores);
        self.restores = Some(restores.clone());
        restores
    }

    fn next_op(&mut self) -> OperationId {
        self.runs += 1;
        OperationId::from_uuid(Uuid::from_u128(
            0x018f_0000_0000_7000_8000_0000_0000_6000 | self.runs,
        ))
    }

    fn ask(
        &self,
        entry: &str,
        destination: Destination,
        origin: RestoreOrigin,
        confirmation: &str,
    ) -> RestoreAsk {
        RestoreAsk {
            alias: alias(),
            snapshot: self.snapshot.clone().expect("the scenario's snapshot").0,
            entry: path(entry),
            destination,
            origin,
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            confirmation: confirmation.to_owned(),
        }
    }

    /// Starts `ask`, keeping how it answered and, when it started, the
    /// restore to run.
    async fn start(&mut self, clock: Arc<FakeClock>, ask: RestoreAsk) {
        let restores = self.restores(clock).await;
        let op = self.next_op();
        match restores.start(op, ask).await {
            Ok(prepared) => {
                self.started.push(Ok(prepared.operation()));
                self.pending = Some(prepared);
            }
            Err(error) => self.started.push(Err(error)),
        }
    }

    /// Runs the restore started last, if it started, until it ends.
    async fn run_pending(&mut self, cancel: impl Future<Output = ()> + Send) {
        if let Some(prepared) = self.pending.take() {
            let restores = self.restores.clone().expect("the use case was made");
            self.finished = Some(restores.run(prepared, cancel).await);
        }
    }

    fn jobs(&self) -> Vec<Job> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn restore_job(&self) -> RestoreJob {
        self.jobs()
            .into_iter()
            .find_map(|job| match job {
                Job::Restore(job) => Some(job),
                _ => None,
            })
            .expect("the engine was given a restore")
    }

    fn refusal(&self) -> &RestoreError {
        match self.started.last().expect("a restore was asked for") {
            Err(error) => error,
            Ok(op) => panic!("the restore started as {op:?}"),
        }
    }

    fn status(&self) -> &OperationStatus {
        self.finished
            .as_ref()
            .expect("a restore ran")
            .operation
            .status()
    }

    async fn history(&self) -> Vec<app::ports::OperationRecord> {
        self.log
            .list(&HistoryQuery {
                plan: None,
                after: None,
                limit: PageSize::DEFAULT,
            })
            .await
            .expect("the log answers")
            .operations
    }
}

#[given(expr = "a snapshot {string} of the host {string} in the repository {string}")]
fn a_snapshot(agent: &mut Agent, short: String, host: String, repository: String) {
    assert_eq!(repository, "demo", "the scenarios restore from \"demo\"");
    agent.restore.snapshot = Some((snapshot_id(&short), host));
}

#[given(expr = "sign-in is on with the admin password {string}")]
fn sign_in_on(agent: &mut Agent, password: String) {
    agent.restore.password = Some(password);
}

#[given("sign-in is off")]
fn sign_in_off(agent: &mut Agent) {
    agent.restore.password = None;
}

/// The snapshot holds 6 files under the entry: what the original holds as
/// it changed, as it holds it, and the rest, which the original lacks.
#[given(
    expr = "the original {string} holds {int} files the snapshot changed, {int} it holds as they are and {int} it doesn't hold"
)]
fn the_original_holds(agent: &mut Agent, _entry: String, changed: u64, same: u64, extra: u64) {
    agent.restore.plan = RestorePlanSummary {
        files_added: 6 - changed - same,
        files_overwritten: changed,
        files_unchanged: same,
        files_kept: extra,
        bytes_to_write: 0,
    };
}

#[given("the agent allows restores from the hub")]
fn allows_hub_restores(agent: &mut Agent) {
    agent.restore.allow_hub_restore = true;
}

#[given(expr = "the agent's configuration sets its restore-folder to {string}")]
fn sets_restore_folder(_agent: &mut Agent, folder: String) {
    todo!("set the agent's restore-folder to {folder}")
}

#[given("the agent has no home folder and its configuration sets no restore-folder")]
fn no_restore_folder(_agent: &mut Agent) {
    todo!("an agent with no folder to restore into")
}

#[given(expr = "the engine restores in {int} steps of 1 minute each")]
fn restores_in_minutes(agent: &mut Agent, steps: u32) {
    agent.restore.run = RestoreRun::Steps {
        steps,
        step: SignedDuration::from_mins(1),
    };
}

#[given(expr = "the engine restores in {int} steps of 1 hour each")]
fn restores_in_hours(agent: &mut Agent, steps: u32) {
    agent.restore.run = RestoreRun::Steps {
        steps,
        step: SignedDuration::from_hours(1),
    };
}

#[given("the engine can't write the restore's target")]
fn target_unwritable(agent: &mut Agent) {
    agent.restore.run = RestoreRun::Unwritable;
}

#[when(expr = "the person restores {string} of the snapshot, confirming with {string}")]
async fn restores(agent: &mut Agent, entry: String, confirmation: String) {
    let ask = agent.restore.ask(
        &entry,
        Destination::NewFolder,
        RestoreOrigin::LocalUi,
        &confirmation,
    );
    agent.restore.start(agent.clock.clone(), ask).await;
    agent.restore.run_pending(futures::future::pending()).await;
}

#[when(expr = "the person starts restoring {string} of the snapshot, confirming with {string}")]
async fn starts_restoring(agent: &mut Agent, entry: String, confirmation: String) {
    let ask = agent.restore.ask(
        &entry,
        Destination::NewFolder,
        RestoreOrigin::LocalUi,
        &confirmation,
    );
    agent.restore.start(agent.clock.clone(), ask).await;
    assert!(
        agent.restore.started.last().is_some_and(Result::is_ok),
        "the first restore started: {:?}",
        agent.restore.started.last()
    );
    // Held, not run: it keeps the slot for the rest of the scenario.
    agent.restore.held = agent.restore.pending.take();
}

#[when(expr = "the person previews restoring {string} of the snapshot in place")]
async fn previews(agent: &mut Agent, entry: String) {
    let restores = agent.restore.restores(agent.clock.clone()).await;
    let op = agent.restore.next_op();
    let snapshot = agent.restore.snapshot.clone().expect("a snapshot").0;
    agent.restore.preview = Some(
        restores
            .preview_in_place(op, &alias(), snapshot, path(&entry), RestoreOrigin::LocalUi)
            .await,
    );
}

#[when(expr = "the person restores {string} of the snapshot in place, confirming with {string}")]
async fn restores_in_place(agent: &mut Agent, entry: String, confirmation: String) {
    let ask = agent.restore.ask(
        &entry,
        Destination::InPlace,
        RestoreOrigin::LocalUi,
        &confirmation,
    );
    agent.restore.start(agent.clock.clone(), ask).await;
    agent.restore.run_pending(futures::future::pending()).await;
}

#[when(expr = "the hub asks for {string} of the snapshot to be restored in place")]
async fn hub_restores_in_place(agent: &mut Agent, entry: String) {
    let short =
        domain::access::step_up::short_id(&agent.restore.snapshot.clone().expect("a snapshot").0)
            .to_owned();
    let ask = agent
        .restore
        .ask(&entry, Destination::InPlace, RestoreOrigin::Hub, &short);
    agent.restore.start(agent.clock.clone(), ask).await;
    agent.restore.run_pending(futures::future::pending()).await;
}

/// The shared clock jumps straight to each sleeper's deadline, so the
/// engine would never let a cancel in; this one moves time by hand.
#[when(
    expr = "the person restores {string} of the snapshot, confirming with {string}, and cancels it after {int} minutes"
)]
async fn restores_and_cancels(
    agent: &mut Agent,
    entry: String,
    confirmation: String,
    minutes: u32,
) {
    let clock = Arc::new(FakeClock::new(agent.clock.now()));
    let ask = agent.restore.ask(
        &entry,
        Destination::NewFolder,
        RestoreOrigin::LocalUi,
        &confirmation,
    );
    agent.restore.start(clock.clone(), ask).await;
    let prepared = agent.restore.pending.take().expect("the restore started");
    let restores = agent
        .restore
        .restores
        .clone()
        .expect("the use case was made");
    let (cancel, cancelled) = futures::channel::oneshot::channel::<()>();
    let running = tokio::spawn(async move {
        restores
            .run(prepared, async move {
                let _ = cancelled.await;
            })
            .await
    });
    for _ in 0..minutes {
        until_sleeping(&clock).await;
        clock.advance(SignedDuration::from_mins(1));
    }
    until_sleeping(&clock).await;
    cancel.send(()).expect("the restore is still running");
    agent.restore.finished = Some(running.await.expect("the restore ran"));
}

/// Waits until the engine sleeps on `clock`, so the next `advance` wakes it.
async fn until_sleeping(clock: &FakeClock) {
    for _ in 0..100_000 {
        if clock.sleepers() > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the engine never slept on the clock");
}

#[then(expr = "the engine restores {string} into the new folder {string}")]
fn restores_into(agent: &mut Agent, entry: String, folder: String) {
    let job = agent.restore.restore_job();
    assert_eq!(job.entry, path(&entry));
    assert_eq!(job.target, RestoreTarget::NewFolder(folder));
    assert_eq!(
        Some(&job.snapshot),
        agent.restore.snapshot.as_ref().map(|(id, _)| id)
    );
}

#[then(expr = "the engine restores {string} over its original paths, deleting nothing")]
fn restores_over(agent: &mut Agent, entry: String) {
    let job = agent.restore.restore_job();
    assert_eq!(job.entry, path(&entry));
    // In place adds and overwrites; the target has no way to delete.
    assert_eq!(job.target, RestoreTarget::InPlace);
}

#[then("the restore ends as succeeded")]
fn ends_succeeded(agent: &mut Agent) {
    assert_eq!(agent.restore.status(), &OperationStatus::Succeeded);
}

#[then("the restore ends as cancelled")]
fn ends_cancelled(agent: &mut Agent) {
    assert_eq!(agent.restore.status(), &OperationStatus::Cancelled);
}

#[then("the restore fails because its deadline passed")]
fn fails_deadline(agent: &mut Agent) {
    assert_eq!(
        agent.restore.status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
}

#[then("the restore fails because its target can't be written")]
fn fails_unwritable(agent: &mut Agent) {
    assert_eq!(
        agent.restore.status(),
        &OperationStatus::Failed {
            reason: FailureReason::RestoreTargetUnwritable
        }
    );
}

#[then("the history shows the restore, started by hand")]
async fn history_shows_restore(agent: &mut Agent) {
    let history = agent.restore.history().await;
    let [record] = history.as_slice() else {
        panic!("one operation in {history:?}");
    };
    assert_eq!(record.kind, OperationKind::Restore);
    assert_eq!(record.trigger, Trigger::Manual);
    assert_eq!(record.status, OperationStatus::Succeeded);
}

#[then("the history shows the restore as it ended")]
async fn history_shows_end(agent: &mut Agent) {
    let history = agent.restore.history().await;
    let [record] = history.as_slice() else {
        panic!("one operation in {history:?}");
    };
    assert_eq!(record.kind, OperationKind::Restore);
    assert_eq!(&record.status, agent.restore.status());
}

#[then("the history shows no operation")]
async fn history_shows_none(agent: &mut Agent) {
    let history = agent.restore.history().await;
    assert!(history.is_empty(), "{history:?}");
}

#[then(
    regex = r"^the preview says (\d+) files? would be added, (\d+) overwritten and (\d+) left as they are$"
)]
fn preview_counts(agent: &mut Agent, added: u64, overwritten: u64, unchanged: u64) {
    let preview = match agent
        .restore
        .preview
        .as_ref()
        .expect("a preview was asked for")
    {
        Ok(preview) => preview,
        Err(error) => panic!("the preview failed: {error:?}"),
    };
    assert_eq!(
        (
            preview.files_added,
            preview.files_overwritten,
            preview.files_unchanged
        ),
        (added, overwritten, unchanged)
    );
}

#[then(expr = "the preview says the {int} files the snapshot doesn't hold would stay")]
fn preview_kept(agent: &mut Agent, kept: u64) {
    let preview = match agent
        .restore
        .preview
        .as_ref()
        .expect("a preview was asked for")
    {
        Ok(preview) => preview,
        Err(error) => panic!("the preview failed: {error:?}"),
    };
    assert_eq!(preview.files_kept, kept);
    assert!(
        agent
            .restore
            .jobs()
            .iter()
            .all(|job| !matches!(job, Job::Restore(_))),
        "a preview restores nothing"
    );
}

#[then("the restore is refused because the step-up confirmation failed")]
fn refused_step_up(agent: &mut Agent) {
    assert!(
        matches!(agent.restore.refusal(), RestoreError::StepUpFailed),
        "{:?}",
        agent.restore.refusal()
    );
}

#[then("the restore is refused while the sign-in wait lasts")]
fn refused_wait(agent: &mut Agent) {
    assert!(
        matches!(agent.restore.refusal(), RestoreError::StepUpWait { seconds } if *seconds > 0),
        "{:?}",
        agent.restore.refusal()
    );
}

#[then("the restore is refused because in place is offered only in the agent's own web UI")]
fn refused_in_place_from_hub(agent: &mut Agent) {
    assert!(
        matches!(
            agent.restore.refusal(),
            RestoreError::Refused(RestoreRefused::InPlaceFromHub)
        ),
        "{:?}",
        agent.restore.refusal()
    );
}

#[then("the restore is refused because the agent has no folder to restore into")]
fn refused_no_restore_folder(_agent: &mut Agent) {
    todo!("refuse a restore with no folder to restore into")
}

#[then("the engine was asked for nothing")]
fn asked_nothing(agent: &mut Agent) {
    let jobs = agent.restore.jobs();
    assert!(jobs.is_empty(), "{jobs:?}");
}

#[then("the second restore is refused, naming the restore that runs")]
fn second_refused(agent: &mut Agent) {
    let first = match agent.restore.started.first() {
        Some(Ok(op)) => *op,
        other => panic!("the first restore started: {other:?}"),
    };
    assert!(
        matches!(agent.restore.refusal(), RestoreError::AlreadyRunning(op) if *op == first),
        "{:?}",
        agent.restore.refusal()
    );
}

#[then(expr = "the failure is told as the event {string}")]
fn told_as(agent: &mut Agent, event: String) {
    let kind = EventKind::parse(&event).expect("an event's name");
    assert_eq!(
        EventKind::ended(
            agent
                .restore
                .finished
                .as_ref()
                .expect("a restore ran")
                .operation
                .kind(),
            Outcome::Failed
        ),
        Some(kind)
    );
}
