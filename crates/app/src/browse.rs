//! The snapshot-browsing use case (§16, decision 102): one page of one folder
//! of a snapshot, read live by one worker. The job names the ID the
//! configuration's entry records, as a run's does, so the engine opens only
//! that repository (refusing another at the location, §7.3) and gives the
//! job the repository's cache. A read (§9): it takes no I7 slot and writes
//! nothing to the operation log.

use std::sync::Arc;

use domain::browse::{FolderPage, FolderPath};
use domain::planning::RepositoryConfig;
use domain::{OperationId, RepositoryAlias, SnapshotId};

use crate::maintenance::MaintenanceError;
use crate::ports::{Clock, Engine};
use crate::secrets::{ResolveError, ResolveRepository};

/// Reads a page of a snapshot's folder.
pub struct BrowseSnapshot {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    resolve: ResolveRepository,
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
    pub fn new(engine: Arc<dyn Engine>, clock: Arc<dyn Clock>, resolve: ResolveRepository) -> Self {
        Self {
            engine,
            clock,
            resolve,
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
        let _ = (
            op,
            alias,
            entry,
            snapshot,
            folder,
            from,
            &self.engine,
            &self.clock,
            &self.resolve,
        );
        todo!("BrowseSnapshot::folder")
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::browse::{DisplayName, Entry, EntryKind, EntryName, FolderPage, FolderPath};
    use domain::execution::FailureReason;
    use domain::planning::{Maintenance, RepositoryConfig};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp, Uuid};
    use futures::stream;

    use super::{BrowseError, BrowseSnapshot};
    use crate::fakes::{FakeClock, FakeSecrets};
    use crate::maintenance::MaintenanceError;
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

    #[allow(clippy::panic)]
    fn repository() -> RepositoryId {
        RepositoryId::parse(&"a".repeat(64)).unwrap_or_else(|e| panic!("a repository ID: {e:?}"))
    }

    #[allow(clippy::panic)]
    fn snapshot() -> SnapshotId {
        SnapshotId::parse(&"3f".repeat(32)).unwrap_or_else(|e| panic!("a snapshot ID: {e:?}"))
    }

    #[allow(clippy::panic)]
    fn alias() -> RepositoryAlias {
        RepositoryAlias::parse("demo").unwrap_or_else(|e| panic!("an alias: {e:?}"))
    }

    #[allow(clippy::panic)]
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

    #[allow(clippy::panic)]
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

    fn use_case(engine: Arc<Scripted>) -> BrowseSnapshot {
        BrowseSnapshot::new(
            engine,
            Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
            ResolveRepository::new(Arc::new(FakeSecrets::new())),
        )
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
        let read = use_case(Arc::clone(&engine))
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
        let _ = use_case(Arc::clone(&engine))
            .folder(op(), &alias(), &entry(), snapshot(), FolderPath::top(), 1_000)
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
        let read = use_case(scripted(Ok(FolderAnswer::SnapshotUnknown)))
            .folder(op(), &alias(), &entry(), snapshot(), FolderPath::top(), 0)
            .await;
        assert_eq!(read, Err(BrowseError::SnapshotUnknown));
    }

    #[tokio::test]
    async fn a_folder_the_snapshot_does_not_hold_is_said_to_be_unknown() {
        let read = use_case(scripted(Ok(FolderAnswer::FolderUnknown)))
            .folder(op(), &alias(), &entry(), snapshot(), path("source/gone"), 0)
            .await;
        assert_eq!(read, Err(BrowseError::FolderUnknown));
    }

    #[tokio::test]
    async fn a_read_the_engine_failed_is_a_failed_read() {
        let reason = FailureReason::EngineFailed {
            detail: "the pack is damaged".to_owned(),
        };
        let read = use_case(scripted(Err(reason.clone())))
            .folder(op(), &alias(), &entry(), snapshot(), FolderPath::top(), 0)
            .await;
        assert_eq!(read, Err(BrowseError::Read(MaintenanceError::Failed(reason))));
    }
}
