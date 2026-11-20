//! The snapshot-browsing use cases (§16, decision 102): one page of one
//! folder of a snapshot, and a search of a repository's snapshots for
//! entries by name (decision 127), each read live by one worker. The job
//! names the ID the configuration's entry records, as a run's does, so the
//! engine opens only that repository (refusing another at the location,
//! §7.3) and gives the job the repository's cache. A read (§9): it takes no
//! I7 slot and writes nothing to the operation log.

use std::sync::Arc;

use domain::browse::{FolderPage, FolderPath, PAGE_ENTRIES};
use domain::execution::FailureReason;
use domain::find::{NamePattern, SEARCH_SECONDS, SearchAnswer, SearchCaps};
use domain::planning::RepositoryConfig;
use domain::{OperationId, PlanId, RepositoryAlias, SignedDuration, SnapshotId};

use crate::maintenance::{MaintenanceError, run_read, within, within_read_timeout};
use crate::ports::{
    BrowseJob, Clock, Engine, EngineResult, FindJob, FolderAnswer, Job, RepositoryAccess,
};
use crate::reads::ReadTurns;
use crate::secrets::{ResolveError, ResolveRepository};

/// How long a search may take in all, its wait for a turn and the
/// repository's open included: its walk's [`SEARCH_SECONDS`] and ten more,
/// within the 30 s a client of a running daemon waits for an answer (§4.6).
pub const SEARCH_TIMEOUT: SignedDuration = SignedDuration::from_secs(25);

// The walk fits in the search's time, with time left to open the repository.
const _: () = assert!(SEARCH_SECONDS + 10 <= 25);

/// Reads a page of a snapshot's folder, or searches a repository's
/// snapshots.
pub struct BrowseSnapshot {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    resolve: ResolveRepository,
    reads: Arc<ReadTurns>,
}

/// Why a folder couldn't be read, or a search couldn't be made. A typed
/// code, never a sentence (§8.4).
#[derive(Debug, PartialEq)]
pub enum BrowseError {
    /// The repository's secrets couldn't be resolved.
    Resolve(ResolveError),
    /// Identifying the repository or reading it failed, or wasn't answered
    /// within the read's timeout: as a maintenance read's would (`Engine`,
    /// `Failed`, `TimedOut`).
    Read(MaintenanceError),
    /// The repository holds no snapshot by that ID. Never a search's.
    SnapshotUnknown,
    /// The snapshot holds no folder at that path. Never a search's.
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
    /// position `from` and holds at most `most` entries (clamped from 1 to
    /// [`PAGE_ENTRIES`]), in the repository the configuration's `entry`,
    /// saved under `alias`, names.
    #[expect(
        clippy::too_many_arguments,
        reason = "each names one part of the page; a struct of them would be built only to be taken apart here"
    )]
    pub async fn folder(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
        snapshot: SnapshotId,
        folder: FolderPath,
        from: u64,
        most: u64,
    ) -> Result<FolderPage, BrowseError> {
        let repository = self.access(alias, entry).await?;
        let most = most.clamp(1, u64::try_from(PAGE_ENTRIES).unwrap_or(u64::MAX));
        let job = Job::Browse(BrowseJob {
            repository,
            snapshot,
            folder,
            from,
            most,
        });
        match within_read_timeout(self.clock.as_ref(), self.run(op, job))
            .await
            .map_err(BrowseError::Read)?
        {
            EngineResult::Folder(FolderAnswer::Page(page)) => Ok(page),
            EngineResult::Folder(FolderAnswer::SnapshotUnknown) => {
                Err(BrowseError::SnapshotUnknown)
            }
            EngineResult::Folder(FolderAnswer::FolderUnknown) => Err(BrowseError::FolderUnknown),
            // Any other answer is the engine stopping without its own.
            _ => Err(engine_exited()),
        }
    }

    /// The entries whose names match `pattern` in the newest snapshots, of
    /// `plan` when it names one, within `caps`, in the repository the
    /// configuration's `entry`, saved under `alias`, names. Given up once
    /// [`SEARCH_TIMEOUT`] has passed, its wait for a turn included.
    pub async fn find(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
        pattern: NamePattern,
        plan: Option<PlanId>,
        caps: SearchCaps,
    ) -> Result<SearchAnswer, BrowseError> {
        let repository = self.access(alias, entry).await?;
        let job = Job::Find(FindJob {
            repository,
            pattern,
            plan,
            caps,
        });
        match within(self.clock.as_ref(), SEARCH_TIMEOUT, self.run(op, job))
            .await
            .map_err(BrowseError::Read)?
        {
            EngineResult::Found(answer) => Ok(answer),
            _ => Err(engine_exited()),
        }
    }

    /// The access to the repository the configuration's `entry`, saved
    /// under `alias`, names. `resolve_saved` gives it the ID the entry
    /// records, so the engine refuses another repository at the location
    /// (§7.3).
    async fn access(
        &self,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
    ) -> Result<RepositoryAccess, BrowseError> {
        Ok(self
            .resolve
            .resolve_saved(alias, entry)
            .await
            .map_err(BrowseError::Resolve)?
            .access)
    }

    /// Runs `job` once a turn is free. The wait for a turn counts against
    /// the caller's timeout, so a read waits no longer in all than one that
    /// started at once.
    async fn run(&self, op: OperationId, job: Job) -> Result<EngineResult, MaintenanceError> {
        let _turn = self.reads.turn().await;
        run_read(self.engine.as_ref(), self.clock.as_ref(), op, job).await
    }
}

/// The engine stopping without an answer of the job's own.
fn engine_exited() -> BrowseError {
    BrowseError::Read(MaintenanceError::Failed(FailureReason::EngineExited {
        exit_code: None,
    }))
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use async_trait::async_trait;
    use domain::browse::{
        DisplayName, Entry, EntryKind, EntryName, FolderPage, FolderPath, PAGE_ENTRIES,
    };
    use domain::execution::FailureReason;
    use domain::find::{NamePattern, SearchAnswer, SearchCaps, SearchStop, Searched};
    use domain::planning::{Maintenance, RepositoryConfig};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid};
    use futures::stream;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;

    use super::{BrowseError, BrowseSnapshot, SEARCH_TIMEOUT};
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
                Job::Find(_) => EngineEvent::Succeeded(EngineResult::Found(found())),
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

    /// A search's answer: one snapshot searched, nothing found, the
    /// snapshot cap reached.
    fn found() -> SearchAnswer {
        SearchAnswer {
            searched: vec![Searched {
                snapshot: snapshot(),
                time: Timestamp::UNIX_EPOCH,
            }],
            left_out: 3,
            matches: Vec::new(),
            stopped: Some(SearchStop::Snapshots),
        }
    }

    fn pattern() -> NamePattern {
        NamePattern::parse("*.pdf").unwrap_or_else(|e| panic!("a pattern: {e:?}"))
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
                .folder(
                    op(),
                    &alias(),
                    &entry(),
                    snapshot(),
                    FolderPath::top(),
                    0,
                    50,
                )
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
            .folder(
                op(),
                &alias(),
                &entry(),
                snapshot(),
                path("source/docs"),
                0,
                50,
            )
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
        assert_eq!(browse.most, 50, "the page holds as many entries as asked");
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
                50,
            )
            .await;
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(Job::Browse(browse)) = seen.last() else {
            panic!("a browse: {seen:?}");
        };
        assert_eq!(browse.from, 1_000);
        assert_eq!(browse.folder, FolderPath::top());
    }

    /// The `most` of the browse job a read asking for `asked` entries sends.
    async fn most_sent(asked: u64) -> u64 {
        let engine = scripted(Ok(FolderAnswer::Page(page())));
        let _ = use_case(engine.clone(), at_epoch())
            .folder(
                op(),
                &alias(),
                &entry(),
                snapshot(),
                FolderPath::top(),
                0,
                asked,
            )
            .await;
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(Job::Browse(browse)) = seen.last() else {
            panic!("a browse: {seen:?}");
        };
        browse.most
    }

    #[tokio::test]
    async fn a_page_holds_at_least_one_entry_and_at_most_a_full_page() {
        let full = u64::try_from(PAGE_ENTRIES).unwrap_or(u64::MAX);
        assert_eq!(most_sent(0).await, 1, "an empty page is never asked for");
        assert_eq!(most_sent(1).await, 1);
        assert_eq!(most_sent(full).await, full);
        assert_eq!(
            most_sent(full + 1).await,
            full,
            "no page holds more than the page a folder listing shows"
        );
        assert_eq!(most_sent(u64::MAX).await, full);
    }

    #[tokio::test]
    async fn a_search_runs_one_find_job_naming_the_recorded_id_the_plan_and_the_caps() {
        let engine = scripted(Ok(FolderAnswer::Page(page())));
        let plan = PlanId::parse("documents").unwrap_or_else(|e| panic!("a plan: {e:?}"));
        let caps = SearchCaps::clamped(Some(5), Some(7));
        let found_now = use_case(engine.clone(), at_epoch())
            .find(
                op(),
                &alias(),
                &entry(),
                pattern(),
                Some(plan.clone()),
                caps,
            )
            .await;
        assert_eq!(found_now, Ok(found()));
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let [Job::Find(find)] = seen.as_slice() else {
            panic!("one search: {seen:?}");
        };
        assert_eq!(
            find.repository.id,
            Some(repository()),
            "the search names the recorded ID, so another repository is refused"
        );
        assert_eq!(find.pattern, pattern());
        assert_eq!(find.plan, Some(plan));
        assert_eq!(find.caps, caps);
    }

    #[tokio::test]
    async fn a_search_nobody_answers_is_given_up_when_the_search_timeout_ends() {
        let (_gate, open) = watch::channel(false);
        let engine = Arc::new(Gated {
            open,
            seen: Mutex::new(Vec::new()),
        });
        let clock = at_epoch();
        let browse = Arc::new(use_case(engine.clone(), Arc::clone(&clock)));
        let searching = {
            let browse = Arc::clone(&browse);
            tokio::spawn(async move {
                browse
                    .find(
                        op(),
                        &alias(),
                        &entry(),
                        pattern(),
                        None,
                        SearchCaps::clamped(None, None),
                    )
                    .await
            })
        };
        settle().await;
        assert_eq!(engine.started(), 1, "the search started its worker");
        clock.advance(SEARCH_TIMEOUT - domain::SignedDuration::from_secs(1));
        settle().await;
        assert!(
            !searching.is_finished(),
            "a second before the search's timeout"
        );
        clock.advance(domain::SignedDuration::from_secs(1));
        let found_now = tokio::time::timeout(Duration::from_secs(5), searching)
            .await
            .expect("the search ends when its timeout does")
            .expect("the search task ran");
        assert_eq!(
            found_now,
            Err(BrowseError::Read(MaintenanceError::TimedOut))
        );
    }

    #[tokio::test]
    async fn a_snapshot_that_is_gone_is_said_to_be_unknown() {
        let read = use_case(scripted(Ok(FolderAnswer::SnapshotUnknown)), at_epoch())
            .folder(
                op(),
                &alias(),
                &entry(),
                snapshot(),
                FolderPath::top(),
                0,
                50,
            )
            .await;
        assert_eq!(read, Err(BrowseError::SnapshotUnknown));
    }

    #[tokio::test]
    async fn a_folder_the_snapshot_does_not_hold_is_said_to_be_unknown() {
        let read = use_case(scripted(Ok(FolderAnswer::FolderUnknown)), at_epoch())
            .folder(
                op(),
                &alias(),
                &entry(),
                snapshot(),
                path("source/gone"),
                0,
                50,
            )
            .await;
        assert_eq!(read, Err(BrowseError::FolderUnknown));
    }

    #[tokio::test]
    async fn a_read_the_engine_failed_is_a_failed_read() {
        let reason = FailureReason::EngineFailed {
            detail: "the pack is damaged".to_owned(),
        };
        let read = use_case(scripted(Err(reason.clone())), at_epoch())
            .folder(
                op(),
                &alias(),
                &entry(),
                snapshot(),
                FolderPath::top(),
                0,
                50,
            )
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
