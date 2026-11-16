//! The snapshot-browsing use case (§16, decision 102): one page of one folder
//! of a snapshot, read live by one worker. The job names the ID the
//! configuration's entry records, as a run's does, so the engine opens only
//! that repository (refusing another at the location, §7.3) and gives the
//! job the repository's cache. A read (§9): it takes no I7 slot and writes
//! nothing to the operation log.

use std::sync::Arc;

use domain::browse::{FolderPage, FolderPath};
use domain::execution::FailureReason;
use domain::planning::RepositoryConfig;
use domain::{OperationId, RepositoryAlias, SnapshotId};

use crate::maintenance::{MaintenanceError, run_read, within_read_timeout};
use crate::ports::{BrowseJob, Clock, Engine, EngineResult, FolderAnswer, Job};
use crate::reads::ReadTurns;
use crate::secrets::{ResolveError, ResolveRepository};

/// Reads a page of a snapshot's folder.
pub struct BrowseSnapshot {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    resolve: ResolveRepository,
    reads: Arc<ReadTurns>,
}

/// Why a folder couldn't be read. A typed code, never a sentence (§8.4).
#[derive(Debug, PartialEq)]
pub enum BrowseError {
    /// The repository's secrets couldn't be resolved.
    Resolve(ResolveError),
    /// Identifying the repository or reading the folder failed, or wasn't
    /// answered within the read timeout: as a maintenance read's would
    /// (`Engine`, `Failed`, `TimedOut`).
    Read(MaintenanceError),
    /// The repository holds no snapshot by that ID.
    SnapshotUnknown,
    /// The snapshot holds no folder at that path.
    FolderUnknown,
}

impl BrowseSnapshot {
    /// A browse that takes its turns from `reads`, which every browse of
    /// the daemon shares: each read is a worker that may hold a folder's
    /// whole tree, hundreds of MiB for a large one (decision 102).
    pub fn new(
        engine: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        resolve: ResolveRepository,
        reads: Arc<ReadTurns>,
    ) -> Self {
        Self {
            engine,
            clock,
            resolve,
            reads,
        }
    }

    /// The page of `folder` of the snapshot `snapshot` that starts at
    /// position `from`, in the repository the configuration's `entry`,
    /// saved under `alias`, names.
    pub async fn folder(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
        snapshot: SnapshotId,
        folder: FolderPath,
        from: u64,
    ) -> Result<FolderPage, BrowseError> {
        // `resolve_saved` gives the access the ID the entry records, so the
        // engine refuses another repository at the location (§7.3).
        let repository = self
            .resolve
            .resolve_saved(alias, entry)
            .await
            .map_err(BrowseError::Resolve)?
            .access;
        let job = Job::Browse(BrowseJob {
            repository,
            snapshot,
            folder,
            from,
        });
        // The wait for a turn counts against the read timeout, so a read
        // waits no longer in all than one that started at once.
        let read = async {
            let _turn = self.reads.turn().await;
            run_read(self.engine.as_ref(), self.clock.as_ref(), op, job).await
        };
        match within_read_timeout(self.clock.as_ref(), read)
            .await
            .map_err(BrowseError::Read)?
        {
            EngineResult::Folder(FolderAnswer::Page(page)) => Ok(page),
            EngineResult::Folder(FolderAnswer::SnapshotUnknown) => {
                Err(BrowseError::SnapshotUnknown)
            }
            EngineResult::Folder(FolderAnswer::FolderUnknown) => Err(BrowseError::FolderUnknown),
            // Any other answer is the engine stopping without its own.
            _ => Err(BrowseError::Read(MaintenanceError::Failed(
                FailureReason::EngineExited { exit_code: None },
            ))),
        }
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use async_trait::async_trait;
    use domain::browse::{DisplayName, Entry, EntryKind, EntryName, FolderPage, FolderPath};
    use domain::execution::FailureReason;
    use domain::planning::{Maintenance, RepositoryConfig};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid};
    use futures::stream;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;

    use super::{BrowseError, BrowseSnapshot};
    use crate::fakes::{FakeClock, FakeSecrets};
    use crate::maintenance::{MaintenanceError, READ_TIMEOUT};
    use crate::ports::{
        Engine, EngineError, EngineEvent, EngineResult, FolderAnswer, Job, JobControl, RunningJob,
    };
    use crate::secrets::ResolveRepository;

    struct Stop;

    #[async_trait]
    impl JobControl for Stop {
        async fn stop(&mut self) {}
    }

    /// Answers a browse with `answer`, and anything else as a failure,
    /// recording every job it was given.
    struct Scripted {
        answer: Result<FolderAnswer, FailureReason>,
        seen: Mutex<Vec<Job>>,
    }

    #[async_trait]
    impl Engine for Scripted {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            let event = match &job {
                Job::Browse(_) => match self.answer.clone() {
                    Ok(answer) => EngineEvent::Succeeded(EngineResult::Folder(answer)),
                    Err(reason) => EngineEvent::Failed(reason),
                },
                other => EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: format!("unexpected job {other:?}"),
                }),
            };
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            Ok(RunningJob {
                events: Box::pin(stream::iter([event])),
                control: Box::new(Stop),
            })
        }
    }

    fn repository() -> RepositoryId {
        RepositoryId::parse(&"a".repeat(64)).unwrap_or_else(|e| panic!("a repository ID: {e:?}"))
    }

    fn snapshot() -> SnapshotId {
        SnapshotId::parse(&"3f".repeat(32)).unwrap_or_else(|e| panic!("a snapshot ID: {e:?}"))
    }

    fn alias() -> RepositoryAlias {
        RepositoryAlias::parse("demo").unwrap_or_else(|e| panic!("an alias: {e:?}"))
    }

    fn path(text: &str) -> FolderPath {
        FolderPath::parse(text).unwrap_or_else(|e| panic!("a path: {e:?}"))
    }

    fn entry() -> RepositoryConfig {
        RepositoryConfig {
            locations: Default::default(),
            settings: domain::planning::RepositorySettings {
                repository: "/home/demo/scratch/repo".to_owned(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new("demo-password")),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            // The ID the entry records: the browse opens only that
            // repository.
            id: Some(repository()),
            maintenance: Maintenance::None,
            group_by: Default::default(),
            hooks: Default::default(),
        }
    }

    fn page() -> FolderPage {
        FolderPage {
            entries: vec![Entry {
                name: EntryName::parse("readme.txt").unwrap_or_else(|e| panic!("{e:?}")),
                display: DisplayName::from("readme.txt".to_owned()),
                kind: EntryKind::File,
                size: Some(5),
                modified: Some(Timestamp::UNIX_EPOCH),
            }],
            total: 1,
            next: None,
        }
    }

    fn use_case(engine: Arc<dyn Engine>, clock: Arc<FakeClock>) -> BrowseSnapshot {
        BrowseSnapshot::new(
            engine,
            clock,
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
            Arc::default(),
        )
    }

    fn at_epoch() -> Arc<FakeClock> {
        Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH))
    }

    /// Answers every job with a page once `open` holds `true`, recording
    /// each job as it starts.
    struct Gated {
        open: watch::Receiver<bool>,
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
            let answer = stream::once(async move {
                // A gate whose sender went opens too: the test is over.
                let _ = open.wait_for(|open| *open).await;
                EngineEvent::Succeeded(EngineResult::Folder(FolderAnswer::Page(page())))
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

    /// Starts a read of the top folder on its own task.
    fn spawn_read(browse: &Arc<BrowseSnapshot>) -> JoinHandle<Result<FolderPage, BrowseError>> {
        let browse = Arc::clone(browse);
        tokio::spawn(async move {
            browse
                .folder(op(), &alias(), &entry(), snapshot(), FolderPath::top(), 0)
                .await
        })
    }

    /// Lets the spawned reads run until they wait on something.
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    fn scripted(answer: Result<FolderAnswer, FailureReason>) -> Arc<Scripted> {
        Arc::new(Scripted {
            answer,
            seen: Mutex::new(Vec::new()),
        })
    }

    fn op() -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(0x5eed))
    }

    #[tokio::test]
    async fn browsing_reads_the_folder_in_one_job_naming_the_recorded_id() {
        let engine = scripted(Ok(FolderAnswer::Page(page())));
        let read = use_case(engine.clone(), at_epoch())
            .folder(op(), &alias(), &entry(), snapshot(), path("source/docs"), 0)
            .await;
        assert_eq!(read, Ok(page()));
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let [Job::Browse(browse)] = seen.as_slice() else {
            panic!("one browse: {seen:?}");
        };
        assert_eq!(browse.repository.repository, "/home/demo/scratch/repo");
        assert_eq!(
            browse.repository.id,
            Some(repository()),
            "the browse names the recorded ID, so another repository is refused"
        );
        assert_eq!(browse.snapshot, snapshot());
        assert_eq!(browse.folder, path("source/docs"));
        assert_eq!(browse.from, 0);
    }

    #[tokio::test]
    async fn the_next_page_is_read_from_the_position_asked_for() {
        let engine = scripted(Ok(FolderAnswer::Page(page())));
        let _ = use_case(engine.clone(), at_epoch())
            .folder(
                op(),
                &alias(),
                &entry(),
                snapshot(),
                FolderPath::top(),
                1_000,
            )
            .await;
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(Job::Browse(browse)) = seen.last() else {
            panic!("a browse: {seen:?}");
        };
        assert_eq!(browse.from, 1_000);
        assert_eq!(browse.folder, FolderPath::top());
    }

    #[tokio::test]
    async fn a_snapshot_that_is_gone_is_said_to_be_unknown() {
        let read = use_case(scripted(Ok(FolderAnswer::SnapshotUnknown)), at_epoch())
            .folder(op(), &alias(), &entry(), snapshot(), FolderPath::top(), 0)
            .await;
        assert_eq!(read, Err(BrowseError::SnapshotUnknown));
    }

    #[tokio::test]
    async fn a_folder_the_snapshot_does_not_hold_is_said_to_be_unknown() {
        let read = use_case(scripted(Ok(FolderAnswer::FolderUnknown)), at_epoch())
            .folder(op(), &alias(), &entry(), snapshot(), path("source/gone"), 0)
            .await;
        assert_eq!(read, Err(BrowseError::FolderUnknown));
    }

    #[tokio::test]
    async fn a_read_the_engine_failed_is_a_failed_read() {
        let reason = FailureReason::EngineFailed {
            detail: "the pack is damaged".to_owned(),
        };
        let read = use_case(scripted(Err(reason.clone())), at_epoch())
            .folder(op(), &alias(), &entry(), snapshot(), FolderPath::top(), 0)
            .await;
        assert_eq!(
            read,
            Err(BrowseError::Read(MaintenanceError::Failed(reason)))
        );
    }

    #[tokio::test]
    async fn at_most_two_folders_are_read_at_once_and_a_third_waits_its_turn() {
        let (gate, open) = watch::channel(false);
        let engine = Arc::new(Gated {
            open,
            seen: Mutex::new(Vec::new()),
        });
        let browse = Arc::new(use_case(engine.clone(), at_epoch()));
        let reads = [
            spawn_read(&browse),
            spawn_read(&browse),
            spawn_read(&browse),
        ];
        settle().await;
        assert_eq!(
            engine.started(),
            2,
            "two workers read at once; the third waits for one of them to end"
        );
        gate.send_replace(true);
        for read in reads {
            let read = tokio::time::timeout(Duration::from_secs(5), read)
                .await
                .expect("each read ends once the pages are answered")
                .expect("the read task ran");
            assert_eq!(read, Ok(page()));
        }
        assert_eq!(engine.started(), 3, "the third read took its turn");
    }

    #[tokio::test]
    async fn a_read_dropped_while_it_waits_takes_no_turn() {
        let (gate, open) = watch::channel(false);
        let engine = Arc::new(Gated {
            open,
            seen: Mutex::new(Vec::new()),
        });
        let browse = Arc::new(use_case(engine.clone(), at_epoch()));
        let running = [spawn_read(&browse), spawn_read(&browse)];
        settle().await;
        let waiting = spawn_read(&browse);
        settle().await;
        waiting.abort();
        assert!(
            waiting.await.is_err_and(|ended| ended.is_cancelled()),
            "the waiting read was dropped before its turn"
        );
        gate.send_replace(true);
        for read in running {
            let read = tokio::time::timeout(Duration::from_secs(5), read)
                .await
                .expect("each read ends once the pages are answered")
                .expect("the read task ran");
            assert_eq!(read, Ok(page()));
        }
        settle().await;
        assert_eq!(
            engine.started(),
            2,
            "the dropped read started no worker when the turns came free"
        );
    }

    #[tokio::test]
    async fn a_read_waiting_its_turn_is_given_up_when_the_read_timeout_ends() {
        let (_gate, open) = watch::channel(false);
        let engine = Arc::new(Gated {
            open,
            seen: Mutex::new(Vec::new()),
        });
        let clock = at_epoch();
        let browse = Arc::new(use_case(engine.clone(), Arc::clone(&clock)));
        let _running = [spawn_read(&browse), spawn_read(&browse)];
        settle().await;
        let waiting = spawn_read(&browse);
        settle().await;
        clock.advance(READ_TIMEOUT);
        // The two reads it waited on are given up now too, so without its
        // own timeout it would start one and wait another READ_TIMEOUT.
        let read = tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("the waiting read ends when the read timeout does")
            .expect("the read task ran");
        assert_eq!(read, Err(BrowseError::Read(MaintenanceError::TimedOut)));
    }
}
