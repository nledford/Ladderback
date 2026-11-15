//! The repository-stats use case (§16, decision 104): resolves the
//! repository's secrets, reads its stats with a `Job::Stats`, and keeps the
//! reading in the history. A read is not an operation (§9): like a listing,
//! it takes no I7 slot, writes nothing to the operation log, and runs
//! alongside anything, as rustic reads alongside a write.

use std::sync::Arc;

use domain::execution::FailureReason;
use domain::maintenance::StatsReading;
use domain::planning::RepositoryConfig;
use domain::{OperationId, RepositoryAlias};

use crate::maintenance::READ_TIMEOUT;
use crate::ports::{Clock, Engine, EngineError, RepositoryAccess, StatsStore, StoreError};
use crate::secrets::{ResolveError, ResolveRepository};

/// Reads repositories' stats and keeps each reading.
pub struct ReadStats {
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
    readings: Arc<dyn StatsStore>,
    clock: Arc<dyn Clock>,
}

/// Why a read of the stats failed. A typed code, never a sentence (§8.4).
#[derive(Debug, PartialEq)]
pub enum StatsError {
    Resolve(ResolveError),
    Engine(EngineError),
    Failed(FailureReason),
    /// The read was not answered within [`READ_TIMEOUT`]: the engine was
    /// stopped and nothing was kept.
    TimedOut,
    /// The reading was taken, and the history couldn't keep it.
    Store(StoreError),
}

impl ReadStats {
    pub fn new(
        engine: Arc<dyn Engine>,
        resolve: ResolveRepository,
        readings: Arc<dyn StatsStore>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            engine,
            resolve,
            readings,
            clock,
        }
    }

    /// Reads the stats of the repository the configuration's entry `entry`,
    /// saved under `alias`, names, and keeps the reading. The engine reads
    /// the repository the entry records the ID of, or none (§7.3), as a
    /// listing does.
    pub async fn read(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
    ) -> Result<StatsReading, StatsError> {
        let _ = (op, alias, entry, &self.resolve);
        todo!()
    }

    /// Reads the stats of the repository `access` opens, under the run
    /// `op`, and keeps the reading under `alias`. A check reads them so,
    /// with its own access, before its end is recorded (§16, decision 104).
    pub async fn read_with(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        access: RepositoryAccess,
    ) -> Result<StatsReading, StatsError> {
        let _ = (op, alias, access, &self.engine, &self.clock, &self.readings);
        todo!()
    }

    /// The reading of `alias` taken last, or `None` when none is kept or it
    /// was taken of another repository than the one its entry `entry`
    /// names now, as after an alias is removed and saved again.
    pub async fn latest(
        &self,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
    ) -> Result<Option<StatsReading>, StoreError> {
        let _ = (alias, entry);
        todo!()
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::collections::BTreeMap;
    use std::pin::pin;
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::FailureReason;
    use domain::maintenance::{RepositoryStats, StatsReading};
    use domain::planning::{Maintenance, RepositoryConfig, RepositorySettings};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp, Uuid};
    use futures::{StreamExt, poll, stream};

    use super::{ReadStats, StatsError};
    use crate::fakes::{FakeClock, FakeSecrets, InMemoryStatsStore};
    use crate::maintenance::READ_TIMEOUT;
    use crate::ports::{
        Clock, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RunningJob,
        StatsStore, StoreError,
    };
    use crate::secrets::ResolveRepository;

    const FIGURES: RepositoryStats = RepositoryStats {
        snapshots: 3,
        total_size: 3000,
        stored_size: 1200,
        raw_size: 2400,
    };

    /// Answers every job with the scripted events, then ends its stream, or
    /// leaves it open when `stalls`; or refuses the job with `refuses`.
    #[derive(Default)]
    struct Scripted {
        events: Vec<EngineEvent>,
        stalls: bool,
        refuses: Option<EngineError>,
        seen: Mutex<Vec<Job>>,
    }

    impl Scripted {
        fn answering(events: Vec<EngineEvent>) -> Arc<Self> {
            Arc::new(Self {
                events,
                ..Self::default()
            })
        }
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
            if let Some(error) = &self.refuses {
                return Err(error.clone());
            }
            let events = stream::iter(self.events.clone());
            Ok(RunningJob {
                events: if self.stalls {
                    events.chain(stream::pending()).boxed()
                } else {
                    events.boxed()
                },
                control: Box::new(Stop),
            })
        }
    }

    /// A history that can't keep anything.
    struct Unwritable;

    #[async_trait]
    impl StatsStore for Unwritable {
        async fn record(&self, _reading: &StatsReading) -> Result<(), StoreError> {
            Err(StoreError::Unavailable {
                detail: "the disk is full".to_owned(),
            })
        }

        async fn latest(
            &self,
            _alias: &RepositoryAlias,
        ) -> Result<Option<StatsReading>, StoreError> {
            Ok(None)
        }

        async fn history(&self, _alias: &RepositoryAlias) -> Result<Vec<StatsReading>, StoreError> {
            Ok(Vec::new())
        }
    }

    fn op() -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(0x5ea7))
    }

    fn alias() -> RepositoryAlias {
        RepositoryAlias::parse("demo").unwrap_or_else(|e| panic!("an alias: {e:?}"))
    }

    fn id(digit: &str) -> RepositoryId {
        RepositoryId::parse(&digit.repeat(64)).unwrap_or_else(|e| panic!("an ID: {e:?}"))
    }

    fn start() -> Timestamp {
        Timestamp::from_second(1_800_000_000).unwrap_or_else(|e| panic!("an instant: {e:?}"))
    }

    /// The configuration's entry for the repository, recording `id`, with
    /// its password from `password`.
    fn entry(id: Option<RepositoryId>, password: SecretRef) -> RepositoryConfig {
        RepositoryConfig {
            locations: Default::default(),
            settings: RepositorySettings {
                repository: "/home/demo/scratch/repo".to_owned(),
                repo_hot: None,
                password,
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            id,
            maintenance: Maintenance::None,
            group_by: Default::default(),
            hooks: Default::default(),
        }
    }

    fn inline() -> SecretRef {
        SecretRef::Inline(SecretValue::new("demo-password"))
    }

    fn started(repository: Option<RepositoryId>) -> EngineEvent {
        EngineEvent::Started {
            index_read_at: start(),
            repository,
        }
    }

    fn succeeded() -> EngineEvent {
        EngineEvent::Succeeded(EngineResult::Stats(FIGURES))
    }

    struct Setup {
        use_case: ReadStats,
        readings: Arc<InMemoryStatsStore>,
        clock: Arc<FakeClock>,
    }

    fn setup(engine: Arc<Scripted>) -> Setup {
        let readings = Arc::new(InMemoryStatsStore::new());
        let clock = Arc::new(FakeClock::new(start()));
        Setup {
            use_case: ReadStats::new(
                engine,
                ResolveRepository::new(Arc::new(FakeSecrets::new())),
                readings.clone(),
                clock.clone(),
            ),
            readings,
            clock,
        }
    }

    async fn history(readings: &InMemoryStatsStore) -> Vec<StatsReading> {
        readings
            .history(&alias())
            .await
            .unwrap_or_else(|e| panic!("the history: {e:?}"))
    }

    #[tokio::test]
    async fn a_read_runs_job_stats_with_the_entrys_access_and_keeps_the_reading() {
        let engine = Scripted::answering(vec![started(None), succeeded()]);
        let Setup {
            use_case,
            readings,
            clock,
        } = setup(engine.clone());
        clock.advance(SignedDuration::from_secs(90));
        let reading = use_case
            .read(op(), &alias(), &entry(Some(id("a")), inline()))
            .await
            .unwrap_or_else(|e| panic!("the read: {e:?}"));
        assert_eq!(
            reading,
            StatsReading {
                alias: alias(),
                repository: Some(id("a")),
                taken_at: clock.now(),
                stats: FIGURES,
            },
            "the engine named no repository: the entry's ID stands"
        );
        assert_eq!(history(&readings).await, vec![reading]);
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let [Job::Stats(access)] = seen.as_slice() else {
            panic!("one Job::Stats, got {seen:?}");
        };
        assert_eq!(access.repository, "/home/demo/scratch/repo");
        assert_eq!(access.id, Some(id("a")));
    }

    /// The reading answered is the one kept: its time is the millisecond the
    /// history keeps, so a page told of a read shows what a later look shows.
    #[tokio::test]
    async fn a_reading_is_answered_as_it_is_kept() {
        let engine = Scripted::answering(vec![succeeded()]);
        let Setup {
            use_case,
            readings,
            clock,
        } = setup(engine);
        clock.advance(SignedDuration::from_micros(1_500));
        let reading = use_case
            .read(op(), &alias(), &entry(None, inline()))
            .await
            .unwrap_or_else(|e| panic!("the read: {e:?}"));
        assert_eq!(history(&readings).await, vec![reading]);
    }

    #[tokio::test]
    async fn a_reading_is_of_the_repository_the_engine_opened() {
        let engine = Scripted::answering(vec![started(Some(id("b"))), succeeded()]);
        let Setup { use_case, .. } = setup(engine);
        let reading = use_case
            .read(op(), &alias(), &entry(Some(id("a")), inline()))
            .await
            .unwrap_or_else(|e| panic!("the read: {e:?}"));
        assert_eq!(reading.repository, Some(id("b")));
    }

    #[tokio::test]
    async fn a_read_whose_secrets_cant_be_resolved_runs_nothing() {
        let engine = Scripted::answering(vec![succeeded()]);
        let Setup {
            use_case, readings, ..
        } = setup(engine.clone());
        let read = use_case
            .read(
                op(),
                &alias(),
                &entry(None, SecretRef::Env("LADDERBACK_DEMO_UNSET".to_owned())),
            )
            .await;
        assert!(matches!(read, Err(StatsError::Resolve(_))), "{read:?}");
        assert!(
            engine
                .seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        );
        assert_eq!(history(&readings).await, Vec::new());
    }

    #[tokio::test]
    async fn a_read_the_engine_refuses_keeps_nothing() {
        let refusal = EngineError::Unavailable {
            detail: "the engine is down".to_owned(),
        };
        let engine = Arc::new(Scripted {
            refuses: Some(refusal.clone()),
            ..Scripted::default()
        });
        let Setup {
            use_case, readings, ..
        } = setup(engine);
        assert_eq!(
            use_case.read(op(), &alias(), &entry(None, inline())).await,
            Err(StatsError::Engine(refusal))
        );
        assert_eq!(history(&readings).await, Vec::new());
    }

    #[tokio::test]
    async fn a_failed_read_keeps_nothing() {
        let reason = FailureReason::EngineFailed {
            detail: "the index can't be read".to_owned(),
        };
        let engine = Scripted::answering(vec![started(None), EngineEvent::Failed(reason.clone())]);
        let Setup {
            use_case, readings, ..
        } = setup(engine);
        assert_eq!(
            use_case.read(op(), &alias(), &entry(None, inline())).await,
            Err(StatsError::Failed(reason))
        );
        assert_eq!(history(&readings).await, Vec::new());
    }

    /// An engine that refuses the job once it has started it (after an
    /// identify) answers as one that refused it outright.
    #[tokio::test]
    async fn a_refusal_after_the_start_is_the_engines() {
        let engine = Scripted::answering(vec![EngineEvent::Failed(
            FailureReason::EngineUnavailable {
                detail: "no such file or directory".to_owned(),
            },
        )]);
        let Setup { use_case, .. } = setup(engine);
        assert_eq!(
            use_case.read(op(), &alias(), &entry(None, inline())).await,
            Err(StatsError::Engine(EngineError::Unavailable {
                detail: "no such file or directory".to_owned(),
            }))
        );
    }

    #[tokio::test]
    async fn an_engine_that_stops_without_a_result_fails_the_read() {
        let engine = Scripted::answering(vec![started(None)]);
        let Setup {
            use_case, readings, ..
        } = setup(engine);
        assert_eq!(
            use_case.read(op(), &alias(), &entry(None, inline())).await,
            Err(StatsError::Failed(FailureReason::EngineExited {
                exit_code: None
            }))
        );
        assert_eq!(history(&readings).await, Vec::new());
    }

    #[tokio::test]
    async fn a_read_unanswered_for_the_read_timeout_gives_up_and_keeps_nothing() {
        let engine = Arc::new(Scripted {
            events: vec![started(None)],
            stalls: true,
            ..Scripted::default()
        });
        let Setup {
            use_case,
            readings,
            clock,
        } = setup(engine);
        let entry = entry(None, inline());
        let alias = alias();
        let mut read = pin!(use_case.read(op(), &alias, &entry));
        assert!(poll!(&mut read).is_pending());
        clock.advance(READ_TIMEOUT - SignedDuration::from_secs(1));
        assert!(
            poll!(&mut read).is_pending(),
            "a second short of the timeout, it still waits"
        );
        clock.advance(SignedDuration::from_secs(1));
        assert_eq!(read.await, Err(StatsError::TimedOut));
        assert_eq!(history(&readings).await, Vec::new());
    }

    #[tokio::test]
    async fn a_reading_the_history_cant_keep_fails_the_read() {
        let use_case = ReadStats::new(
            Scripted::answering(vec![succeeded()]),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            Arc::new(Unwritable),
            Arc::new(FakeClock::new(start())),
        );
        assert_eq!(
            use_case.read(op(), &alias(), &entry(None, inline())).await,
            Err(StatsError::Store(StoreError::Unavailable {
                detail: "the disk is full".to_owned(),
            }))
        );
    }

    #[tokio::test]
    async fn the_latest_reading_is_the_one_kept_last() {
        let Setup {
            use_case, clock, ..
        } = setup(Scripted::answering(vec![succeeded()]));
        let entry = entry(None, inline());
        assert_eq!(use_case.latest(&alias(), &entry).await, Ok(None));
        use_case
            .read(op(), &alias(), &entry)
            .await
            .unwrap_or_else(|e| panic!("the first read: {e:?}"));
        clock.advance(SignedDuration::from_mins(5));
        let second = use_case
            .read(op(), &alias(), &entry)
            .await
            .unwrap_or_else(|e| panic!("the second read: {e:?}"));
        assert_eq!(use_case.latest(&alias(), &entry).await, Ok(Some(second)));
    }

    #[tokio::test]
    async fn the_latest_reading_of_another_repository_saved_under_the_alias_is_none() {
        let Setup { use_case, .. } = setup(Scripted::answering(vec![succeeded()]));
        let before = entry(Some(id("a")), inline());
        let taken = use_case
            .read(op(), &alias(), &before)
            .await
            .unwrap_or_else(|e| panic!("the read: {e:?}"));
        assert_eq!(
            use_case.latest(&alias(), &before).await,
            Ok(Some(taken)),
            "the same repository's"
        );
        let now = entry(Some(id("b")), inline());
        assert_eq!(use_case.latest(&alias(), &now).await, Ok(None));
    }
}
