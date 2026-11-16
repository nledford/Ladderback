//! The snapshot-list use case: resolves the repository's secrets, lists its
//! snapshots with a `Job::Ls`, and returns them newest first. A read is not
//! an operation (§9): it takes no I7 slot, writes nothing to the operation
//! log, and the UI fetches the list live — nothing stores it.

use std::sync::Arc;

use domain::execution::FailureReason;
use domain::planning::RepositoryConfig;
use domain::{OperationId, RepositoryAlias};
use futures::StreamExt;

use crate::ports::{
    Clock, Engine, EngineError, EngineEvent, EngineResult, Job, RunningJob, SnapshotSummary,
};
use crate::reads::ReadTurns;
use crate::secrets::{ResolveError, ResolveRepository};

/// Lists a repository's snapshots. Saves nothing: the list is fetched live
/// every time, for a read is safe to run alongside anything (§9).
pub struct ListSnapshots {
    engine: Arc<dyn Engine>,
    resolve: ResolveRepository,
}

/// Why a listing failed. A typed code, never a sentence (§8.4).
#[derive(Debug, PartialEq)]
pub enum SnapshotError {
    Resolve(ResolveError),
    Engine(EngineError),
    Failed(FailureReason),
    /// No turn came free, or the listing didn't end, within the read
    /// timeout.
    TimedOut,
}

impl ListSnapshots {
    pub fn new(engine: Arc<dyn Engine>, resolve: ResolveRepository) -> Self {
        Self { engine, resolve }
    }

    /// A listing that takes its turn from `turns`, which every listing of
    /// the daemon shares, waiting no longer than the read timeout by
    /// `clock`.
    pub fn with_turns(self, clock: Arc<dyn Clock>, turns: Arc<ReadTurns>) -> Self {
        let _ = (clock, turns);
        todo!()
    }

    /// Lists the snapshots of the repository the configuration's entry
    /// `entry`, saved under `alias`, names, newest first. The engine lists
    /// the repository the entry records the ID of, or none (§7.3), and the
    /// daemon records the ID of the one it opened in an entry that records
    /// none (§6.1, decision 57).
    pub async fn list(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
    ) -> Result<Vec<SnapshotSummary>, SnapshotError> {
        let access = self
            .resolve
            .resolve_saved(alias, entry)
            .await
            .map_err(SnapshotError::Resolve)?
            .access;
        let job = Job::Ls(access);
        let mut snapshots = run(self.engine.as_ref(), op, job).await?;
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.time));
        Ok(snapshots)
    }
}

/// Runs the listing job to its end and returns its list.
async fn run(
    engine: &dyn Engine,
    op: OperationId,
    job: Job,
) -> Result<Vec<SnapshotSummary>, SnapshotError> {
    let RunningJob { events, .. } = engine.run(op, job).await.map_err(SnapshotError::Engine)?;
    let mut events = events.fuse();
    while let Some(event) = events.next().await {
        match event {
            EngineEvent::Succeeded(EngineResult::Snapshots(snapshots)) => return Ok(snapshots),
            // A refusal after an identify is answered as a refusal before it.
            EngineEvent::Failed(reason) => {
                return Err(EngineError::try_from(reason)
                    .map_or_else(SnapshotError::Failed, SnapshotError::Engine));
            }
            _ => {}
        }
    }
    // The stream ended without a result: the engine stopped without reporting.
    Err(SnapshotError::Failed(FailureReason::EngineExited {
        exit_code: None,
    }))
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::FailureReason;
    use domain::planning::{Maintenance, RepositoryConfig};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid};
    use std::collections::BTreeMap;

    use super::{ListSnapshots, SnapshotError};
    use crate::fakes::{FakeClock, FakeSecrets, InMemoryConfigStore};
    use crate::ports::{
        Engine, EngineError, EngineResult, Job, JobControl, RunningJob, SavedEntry, SnapshotSummary,
    };
    use crate::repositories::{ConfigWrites, RecordOpened};
    use crate::secrets::ResolveRepository;

    /// An engine that answers an `Ls` job with the scripted outcome, and
    /// records the jobs it was given.
    struct Scripted {
        outcome: Mutex<Result<Vec<SnapshotSummary>, FailureReason>>,
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
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            let event = match self
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
            {
                Ok(snapshots) => EngineEvent::Succeeded(EngineResult::Snapshots(snapshots)),
                Err(reason) => EngineEvent::Failed(reason),
            };
            Ok(RunningJob {
                events: Box::pin(futures::stream::iter([event])),
                control: Box::new(Stop),
            })
        }
    }

    use crate::ports::EngineEvent;

    fn snapshot(n: u8, at: i64, label: &str) -> SnapshotSummary {
        SnapshotSummary {
            id: SnapshotId::parse(&format!("{n:0>64}"))
                .unwrap_or_else(|e| panic!("64 hex digits: {e:?}")),
            time: Timestamp::from_second(at).unwrap_or_else(|e| panic!("a valid instant: {e:?}")),
            offset: domain::Offset::UTC,
            hostname: "demo-host".to_owned(),
            label: label.to_owned(),
            tags: Vec::new(),
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: false,
            tags_truncated: false,
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
        RepositoryAlias::parse("demo").unwrap_or_else(|e| panic!("an alias: {e:?}"))
    }

    /// The configuration's entry for the repository, recording `id`.
    fn entry(id: Option<RepositoryId>) -> RepositoryConfig {
        RepositoryConfig {
            locations: Default::default(),
            settings: settings(),
            id,
            maintenance: Maintenance::None,
            group_by: Default::default(),
            hooks: Default::default(),
        }
    }

    fn repository() -> RepositoryId {
        RepositoryId::parse(&"a".repeat(64)).unwrap_or_else(|e| panic!("a repository ID: {e:?}"))
    }

    #[tokio::test]
    async fn listing_runs_job_ls_and_returns_the_snapshots_newest_first() {
        let oldest = snapshot(1, 1_700_000_000, "oldest");
        let middle = snapshot(2, 1_700_000_900, "middle");
        let newest = snapshot(3, 1_700_001_800, "newest");
        let engine = Arc::new(Scripted {
            // The engine reports them oldest first: the use case orders them.
            outcome: Mutex::new(Ok(vec![oldest, middle.clone(), newest.clone()])),
            seen: Mutex::new(Vec::new()),
        });
        let use_case = ListSnapshots::new(
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        let listed = use_case
            .list(op(), &alias(), &entry(Some(repository())))
            .await
            .unwrap_or_else(|e| panic!("the listing: {e:?}"));
        assert_eq!(
            listed,
            vec![newest, middle, snapshot(1, 1_700_000_000, "oldest")]
        );
        {
            let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
            let Job::Ls(access) = &seen[0] else {
                panic!("listing runs Job::Ls, got {:?}", seen[0]);
            };
            assert_eq!(access.repository, "/home/demo/scratch/repo");
            assert_eq!(
                access.id,
                Some(repository()),
                "the recorded ID goes with it"
            );
        }
    }

    /// The listing's job carries the entry it was made from, as it was read,
    /// so the daemon can record in it the ID of the repository the job opens
    /// (§6.1, decision 57).
    #[tokio::test]
    async fn a_listing_carries_the_entry_it_was_made_from() {
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Ok(Vec::new())),
            seen: Mutex::new(Vec::new()),
        });
        let use_case = ListSnapshots::new(
            engine.clone(),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        use_case
            .list(op(), &alias(), &entry(None))
            .await
            .unwrap_or_else(|e| panic!("the listing: {e:?}"));
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(Job::Ls(access)) = seen.first() else {
            panic!("listing runs Job::Ls, got {seen:?}");
        };
        assert_eq!(access.id, None);
        assert_eq!(
            access.entry,
            Some(SavedEntry {
                alias: alias(),
                entry: entry(None),
            })
        );
    }

    #[tokio::test]
    async fn a_repository_that_cannot_be_opened_fails_the_listing() {
        let reason = FailureReason::EngineFailed {
            detail: "no repository there".to_owned(),
        };
        let engine = Arc::new(Scripted {
            outcome: Mutex::new(Err(reason.clone())),
            seen: Mutex::new(Vec::new()),
        });
        let use_case =
            ListSnapshots::new(engine, ResolveRepository::new(Arc::new(FakeSecrets::new())));
        assert_eq!(
            use_case.list(op(), &alias(), &entry(None)).await,
            Err(SnapshotError::Failed(reason))
        );
    }

    /// Identifies the repository it is asked to, and refuses every other
    /// job, as an engine that can no longer be started does.
    struct RefusesAfterIdentify;

    #[async_trait]
    impl Engine for RefusesAfterIdentify {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            if !matches!(job, Job::Identify(_)) {
                return Err(EngineError::Unavailable {
                    detail: "no such file or directory".to_owned(),
                });
            }
            Ok(RunningJob {
                events: Box::pin(futures::stream::iter([
                    EngineEvent::Started {
                        index_read_at: Timestamp::UNIX_EPOCH,
                        repository: Some(repository()),
                    },
                    EngineEvent::Succeeded(EngineResult::Repository { id: repository() }),
                ])),
                control: Box::new(Stop),
            })
        }
    }

    /// The listing of a repository whose entry records no ID runs after an
    /// identify (decision 81); an engine that refuses it then refuses the
    /// listing as it would have refused it alone.
    #[tokio::test]
    async fn a_listing_the_engine_refuses_after_its_identify_is_refused_as_the_engine_refused_it() {
        let engine = RecordOpened::new(
            Arc::new(RefusesAfterIdentify),
            Arc::new(InMemoryConfigStore::new()),
            Arc::new(ConfigWrites::new(())),
            Arc::new(|_| {}),
        );
        let use_case = ListSnapshots::new(
            Arc::new(engine),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        );
        assert_eq!(
            use_case.list(op(), &alias(), &entry(None)).await,
            Err(SnapshotError::Engine(EngineError::Unavailable {
                detail: "no such file or directory".to_owned(),
            }))
        );
    }

    /// Answers every listing once `open` holds `true`, recording each job
    /// as it starts.
    struct Gated {
        open: tokio::sync::watch::Receiver<bool>,
        seen: Mutex<Vec<Job>>,
    }

    #[async_trait]
    impl Engine for Gated {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            let mut open = self.open.clone();
            let answer = futures::stream::once(async move {
                // A gate whose sender went opens too: the test is over.
                let _ = open.wait_for(|open| *open).await;
                EngineEvent::Succeeded(EngineResult::Snapshots(Vec::new()))
            });
            Ok(RunningJob {
                events: Box::pin(answer),
                control: Box::new(Stop),
            })
        }
    }

    impl Gated {
        fn started(&self) -> usize {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len()
        }
    }

    fn gated() -> (tokio::sync::watch::Sender<bool>, Arc<Gated>) {
        let (gate, open) = tokio::sync::watch::channel(false);
        let engine = Arc::new(Gated {
            open,
            seen: Mutex::new(Vec::new()),
        });
        (gate, engine)
    }

    fn in_turns(engine: Arc<Gated>, clock: Arc<FakeClock>) -> Arc<ListSnapshots> {
        Arc::new(
            ListSnapshots::new(engine, ResolveRepository::new(Arc::new(FakeSecrets::new())))
                .with_turns(clock, Arc::default()),
        )
    }

    fn spawn_listing(
        listing: &Arc<ListSnapshots>,
    ) -> tokio::task::JoinHandle<Result<Vec<SnapshotSummary>, SnapshotError>> {
        let listing = Arc::clone(listing);
        tokio::spawn(async move { listing.list(op(), &alias(), &entry(None)).await })
    }

    /// Lets the spawned listings run until they wait on something.
    async fn settle() {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    /// #710: each listing is a worker that reads the repository's whole
    /// index, so a burst of them is held to the limit.
    #[tokio::test]
    async fn at_most_two_listings_run_at_once_and_a_third_waits_its_turn() {
        let (gate, engine) = gated();
        let listing = in_turns(
            engine.clone(),
            Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
        );
        let listings = [
            spawn_listing(&listing),
            spawn_listing(&listing),
            spawn_listing(&listing),
        ];
        settle().await;
        assert_eq!(
            engine.started(),
            2,
            "two workers list at once; the third waits for one of them to end"
        );
        gate.send_replace(true);
        for listed in listings {
            let listed = tokio::time::timeout(std::time::Duration::from_secs(5), listed)
                .await
                .unwrap_or_else(|_| panic!("each listing ends once the lists are answered"))
                .unwrap_or_else(|e| panic!("the listing task ran: {e:?}"));
            assert_eq!(listed, Ok(Vec::new()));
        }
        assert_eq!(engine.started(), 3, "the third listing took its turn");
    }

    #[tokio::test]
    async fn a_listing_waiting_its_turn_is_given_up_when_the_read_timeout_ends() {
        let (_gate, engine) = gated();
        let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
        let listing = in_turns(engine.clone(), Arc::clone(&clock));
        let _running = [spawn_listing(&listing), spawn_listing(&listing)];
        settle().await;
        let waiting = spawn_listing(&listing);
        settle().await;
        clock.advance(crate::maintenance::READ_TIMEOUT);
        let listed = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .unwrap_or_else(|_| panic!("the waiting listing ends when the read timeout does"))
            .unwrap_or_else(|e| panic!("the listing task ran: {e:?}"));
        assert_eq!(listed, Err(SnapshotError::TimedOut));
        assert_eq!(
            engine.started(),
            2,
            "the given-up listing started no worker"
        );
    }
}
