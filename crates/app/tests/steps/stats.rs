//! Steps for `features/repositories/stats.feature`: they read the stats of
//! the forget scenarios' repository, "demo", through a scripted engine of
//! their own, which answers an identify, a check and a stats read, and notes
//! what the operation log said of the run that asked for the stats.

use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};

use app::fakes::{FakeClock, FakeRetention, FakeSecrets, FakeSources, InMemoryStatsStore};
use app::maintenance::{MaintenanceKind, PreparedMaintenance, RunMaintenance};
use app::ports::{
    CheckSummary, Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job,
    JobControl, OperationStore, RunningJob, StatsStore, StoreError,
};
use app::secrets::ResolveRepository;
use app::stats::{ReadStats, StatsError};
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::{FailureReason, OperationStatus};
use domain::maintenance::{RepositoryStats, StatsReading};
use domain::planning::{Maintenance, RepositoryConfig};
use domain::{OperationId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp, Uuid};
use futures::channel::oneshot;
use futures::{FutureExt, StreamExt, future, stream};

use super::Agent;
use super::forget_steps::{MACHINE, assess_and_start, self_agent};

/// How the scripted engine answers a stats read.
#[derive(Clone, Copy, Default)]
enum Answer {
    /// With the scenario's figures.
    #[default]
    Figures,
    /// With a failure: the repository's index can't be read.
    Fails,
    /// Not at all: the engine can't be reached for any job.
    Unreachable,
    /// With figures, but the check fails before it has a result.
    CheckFails,
    /// Never: the read is still going when the scenario looks.
    Unanswered,
}

/// What the stats scenarios set up and what their reads answered.
#[derive(Default)]
pub struct StatsWorld {
    /// The figures the scripted engine reads.
    figures: RepositoryStats,
    answer: Answer,
    /// The history the readings are kept in.
    readings: Arc<InMemoryStatsStore>,
    /// What the person's read answered.
    read: Option<Result<StatsReading, StatsError>>,
    /// The status the operation log gave the run that asked for the stats,
    /// when the engine got the stats job.
    status_at_read: Arc<Mutex<Option<OperationStatus>>>,
    /// Told when the engine gets a stats job it never answers.
    asked: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    /// What the person's look at the last reading kept answered.
    looked: Option<Result<Option<StatsReading>, StoreError>>,
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

fn repository_id() -> RepositoryId {
    RepositoryId::parse(&"5a".repeat(32)).expect("a repository ID")
}

fn operation_id(low: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_5000 | low,
    ))
}

/// The instant the agent's clock reads, to the millisecond the history
/// keeps.
fn now(agent: &Agent) -> Timestamp {
    let now = agent.clock.now();
    Timestamp::from_millisecond(now.as_millisecond()).expect("an instant")
}

struct Nothing;

#[async_trait]
impl JobControl for Nothing {
    async fn stop(&mut self) {}
}

/// Answers an identify, a check and a stats read at once.
struct Scripted {
    figures: RepositoryStats,
    answer: Answer,
    operations: Arc<dyn OperationStore>,
    status_at_read: Arc<Mutex<Option<OperationStatus>>>,
    asked: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        if matches!(self.answer, Answer::Unreachable) {
            return Err(EngineError::Unavailable {
                detail: "the engine is down".to_owned(),
            });
        }
        let result = match job {
            Job::Identify(_) => EngineEvent::Succeeded(EngineResult::Repository {
                id: repository_id(),
            }),
            Job::Check(_) if matches!(self.answer, Answer::CheckFails) => {
                EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "the check stopped".to_owned(),
                })
            }
            Job::Check(_) => EngineEvent::Succeeded(EngineResult::Check(CheckSummary {
                repository: repository_id(),
                errors: 0,
                warnings: 0,
            })),
            Job::Stats(_) => {
                let status = self
                    .operations
                    .get(op)
                    .await
                    .expect("the operation log answers")
                    .map(|detail| detail.record.status);
                *self
                    .status_at_read
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = status;
                match self.answer {
                    Answer::Fails => EngineEvent::Failed(FailureReason::EngineFailed {
                        detail: "the index can't be read".to_owned(),
                    }),
                    Answer::Unanswered => {
                        if let Some(asked) = self
                            .asked
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .take()
                        {
                            let _ = asked.send(());
                        }
                        return Ok(RunningJob {
                            events: stream::iter([started()]).chain(stream::pending()).boxed(),
                            control: Box::new(Nothing),
                        });
                    }
                    _ => EngineEvent::Succeeded(EngineResult::Stats(self.figures)),
                }
            }
            other => panic!("the stats scenarios run no {other:?}"),
        };
        Ok(RunningJob {
            events: stream::iter([started(), result]).boxed(),
            control: Box::new(Nothing),
        })
    }
}

/// What the scripted engine says when a job starts: it names no repository.
fn started() -> EngineEvent {
    EngineEvent::Started {
        index_read_at: Timestamp::from_second(1_800_000_000).expect("an instant"),
        repository: None,
    }
}

impl StatsWorld {
    fn engine(&self, operations: Arc<dyn OperationStore>) -> Arc<dyn Engine> {
        Arc::new(Scripted {
            figures: self.figures,
            answer: self.answer,
            operations,
            status_at_read: Arc::clone(&self.status_at_read),
            asked: Arc::clone(&self.asked),
        })
    }

    fn read_stats(&self, agent: &Agent) -> ReadStats {
        // A read never answered waits on a clock that stands still, not the
        // agent's, which jumps to each deadline: only a cancel ends it.
        let clock: Arc<dyn Clock> = match self.answer {
            Answer::Unanswered => Arc::new(FakeClock::new(agent.clock.now())),
            _ => agent.clock.clone(),
        };
        ReadStats::new(
            self.engine(agent.store.clone()),
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            self.readings.clone(),
            clock,
        )
    }

    fn read(&self) -> &Result<StatsReading, StatsError> {
        self.read.as_ref().expect("the stats were read")
    }

    fn reading(&self) -> &StatsReading {
        self.read()
            .as_ref()
            .unwrap_or_else(|error| panic!("the read failed: {error:?}"))
    }

    async fn history(&self) -> Vec<StatsReading> {
        self.readings
            .history(&alias())
            .await
            .expect("the history answers")
    }
}

/// Adds "demo", the forget scenarios' repository, once.
async fn demo(agent: &mut Agent, named: &str) {
    assert_eq!(named, "demo", "the stats scenarios read \"demo\"");
    let config = agent
        .forget
        .store()
        .load()
        .await
        .expect("the configuration loads");
    if config.repository(&alias()).is_none() {
        agent.forget.repository().await;
    }
}

#[given(
    expr = "the repository {string} holds {int} snapshots of {int} bytes each, stored in {int} bytes of {int} raw"
)]
async fn holds(agent: &mut Agent, named: String, count: u64, each: u64, stored: u64, raw: u64) {
    demo(agent, &named).await;
    agent.stats.figures = RepositoryStats {
        snapshots: count,
        total_size: count * each,
        stored_size: stored,
        raw_size: raw,
    };
}

#[given(expr = "the repository {string} holds no snapshots and stores nothing")]
async fn holds_nothing(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.stats.figures = RepositoryStats::default();
}

#[given(expr = "the hub maintains the repository {string}, not this agent")]
async fn hub_maintains(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.forget.maintained_by(Maintenance::Hub).await;
}

#[given(expr = "the repository {string} can't be reached")]
async fn unreachable(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.stats.answer = Answer::Unreachable;
}

#[given(expr = "the check of {string} fails")]
async fn check_fails(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.stats.answer = Answer::CheckFails;
}

#[given(expr = "the stats of {string} can't be read")]
async fn unreadable(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.stats.answer = Answer::Fails;
}

#[given(
    expr = "the history keeps a reading of {string} taken {int} days ago and one taken {int} days ago"
)]
async fn kept_readings(agent: &mut Agent, named: String, older: i64, newer: i64) {
    demo(agent, &named).await;
    for days in [older, newer] {
        let reading = StatsReading {
            alias: alias(),
            repository: Some(repository_id()),
            taken_at: now(agent)
                .checked_sub(SignedDuration::from_hours(days * 24))
                .expect("an instant"),
            stats: RepositoryStats {
                snapshots: u64::try_from(days).expect("days ago"),
                ..RepositoryStats::default()
            },
        };
        agent
            .stats
            .readings
            .record(&reading)
            .await
            .expect("the history keeps it");
    }
}

#[given(expr = "the history keeps a reading of {string} taken of another repository")]
async fn kept_of_another(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    let reading = StatsReading {
        alias: alias(),
        repository: Some(RepositoryId::parse(&"6b".repeat(32)).expect("a repository ID")),
        taken_at: now(agent),
        stats: agent.stats.figures,
    };
    agent
        .stats
        .readings
        .record(&reading)
        .await
        .expect("the history keeps it");
}

#[given(expr = "the configuration records the ID of the repository {string}")]
async fn records_id(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.forget.records_id(repository_id()).await;
}

#[given(expr = "the stats of {string} are never answered")]
async fn unanswered(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    agent.stats.answer = Answer::Unanswered;
}

/// The configuration's entry for "demo".
async fn entry(agent: &Agent) -> RepositoryConfig {
    agent
        .forget
        .store()
        .load()
        .await
        .expect("the configuration loads")
        .repository(&alias())
        .cloned()
        .expect("the repository is configured")
}

#[when(expr = "the person reads the stats of {string}")]
async fn person_reads(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    let entry = entry(agent).await;
    let read = agent
        .stats
        .read_stats(agent)
        .read(operation_id(1), &alias(), &entry)
        .await;
    agent.stats.read = Some(read);
}

#[when(expr = "the person looks at the stats of {string}")]
async fn person_looks(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    let entry = entry(agent).await;
    let looked = agent.stats.read_stats(agent).latest(&alias(), &entry).await;
    agent.stats.looked = Some(looked);
}

/// A check of "demo", started, that reads the stats after it.
async fn started_check(agent: &Agent) -> (RunMaintenance, PreparedMaintenance) {
    let use_case = RunMaintenance::new(
        agent.stats.engine(agent.store.clone()),
        agent.clock.clone(),
        agent.store.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::default())),
        Arc::new(FakeRetention),
        agent.forget.store(),
        Arc::default(),
        self_agent(),
        MACHINE.to_owned(),
        Arc::new(FakeSources),
    )
    .with_stats(Arc::new(agent.stats.read_stats(agent)));
    let prepared = assess_and_start(&use_case, operation_id(2), MaintenanceKind::Check)
        .await
        .expect("the check starts");
    (use_case, prepared)
}

#[when(expr = "a check of {string} runs")]
async fn check_runs(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    let (use_case, prepared) = started_check(agent).await;
    // The scripted engine answers every job at once.
    agent.finished = Some(use_case.execute(prepared, future::pending()).await);
}

#[when(expr = "a check of {string} is cancelled while it reads the stats")]
async fn check_canceled_while_reading(agent: &mut Agent, named: String) {
    demo(agent, &named).await;
    let (asked, stats_asked) = oneshot::channel();
    *agent
        .stats
        .asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(asked);
    let (use_case, prepared) = started_check(agent).await;
    let (cancel, canceled) = oneshot::channel::<()>();
    let mut run = pin!(use_case.execute(prepared, async move {
        let _ = canceled.await;
    }));
    // The check runs until the engine has its stats job, which it never
    // answers.
    if let future::Either::Left((finished, _)) = future::select(run.as_mut(), stats_asked).await {
        panic!("the check ended before it read the stats: {finished:?}");
    }
    let _ = cancel.send(());
    // Nothing the run waits for once cancelled is the engine's, so it ends
    // in the poll that sees the cancel.
    agent.finished = Some(
        run.as_mut()
            .now_or_never()
            .expect("the check ends once it is cancelled"),
    );
}

#[then(
    expr = "the reading shows {int} snapshots, a total size of {int} bytes and a stored size of {int} bytes"
)]
fn shows(agent: &mut Agent, snapshots: u64, total: u64, stored: u64) {
    let stats = agent.stats.reading().stats;
    assert_eq!(
        (stats.snapshots, stats.total_size, stats.stored_size),
        (snapshots, total, stored),
        "{stats:?}"
    );
}

#[then(expr = "the reading shows that compression saved {int} percent")]
fn shows_compression(agent: &mut Agent, percent: u8) {
    assert_eq!(
        agent.stats.reading().stats.compression_saved_percent(),
        Some(percent)
    );
}

#[then("the reading shows no compression")]
fn shows_no_compression(agent: &mut Agent) {
    assert_eq!(
        agent.stats.reading().stats.compression_saved_percent(),
        None
    );
}

#[then(expr = "the history keeps the reading of {string}, taken now")]
async fn keeps_reading_now(agent: &mut Agent, named: String) {
    assert_eq!(named, "demo");
    let history = agent.stats.history().await;
    assert_eq!(history, vec![agent.stats.reading().clone()]);
    assert_eq!(history.first().map(|r| r.taken_at), Some(now(agent)));
    assert_eq!(
        history.first().map(|r| r.repository.clone()),
        Some(None),
        "the repository's entry records no ID, and the engine named none"
    );
}

#[then("the read fails because the repository can't be reached")]
fn fails_unreachable(agent: &mut Agent) {
    assert!(
        matches!(
            agent.stats.read(),
            Err(StatsError::Engine(EngineError::Unavailable { .. }))
        ),
        "{:?}",
        agent.stats.read()
    );
}

#[then(expr = "the history keeps no reading of {string}")]
async fn keeps_none(agent: &mut Agent, named: String) {
    assert_eq!(named, "demo");
    assert_eq!(agent.stats.history().await, Vec::new());
}

#[then("the check succeeded")]
fn check_succeeded(agent: &mut Agent) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Succeeded,
        "{:?}",
        agent.finished
    );
}

#[then("the check failed")]
fn check_failed(agent: &mut Agent) {
    assert!(
        matches!(agent.operation().status(), OperationStatus::Failed { .. }),
        "{:?}",
        agent.finished
    );
    assert_eq!(agent.finished().stats_failed, None, "nothing was read");
}

#[then(
    expr = "the history keeps a reading of {string} the check took while it still held the repository"
)]
async fn check_kept_reading(agent: &mut Agent, named: String) {
    assert_eq!(named, "demo");
    assert_eq!(
        *agent
            .stats
            .status_at_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
        Some(OperationStatus::Running),
        "the check's end was recorded before its stats were read"
    );
    let history = agent.stats.history().await;
    let [reading] = history.as_slice() else {
        panic!("one reading: {history:?}");
    };
    assert_eq!(reading.stats, agent.stats.figures);
    assert_eq!(
        reading.repository,
        Some(repository_id()),
        "the check's own access, which its identify named"
    );
    assert_eq!(agent.finished().stats_failed, None);
}

#[then(expr = "the history of {string} holds {int} readings, the one taken {int} days ago first")]
async fn history_holds(agent: &mut Agent, named: String, count: usize, days: u64) {
    assert_eq!(named, "demo");
    let history = agent.stats.history().await;
    assert_eq!(history.len(), count, "{history:?}");
    assert_eq!(
        history.first().map(|r| r.stats.snapshots),
        Some(days),
        "{history:?}"
    );
    assert_eq!(history.last(), Some(agent.stats.reading()));
}

#[then("no reading is shown")]
fn no_reading_shown(agent: &mut Agent) {
    assert_eq!(
        agent.stats.looked.as_ref().expect("the person looked"),
        &Ok(None)
    );
}
