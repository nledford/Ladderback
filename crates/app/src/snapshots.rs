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
    Engine, EngineError, EngineEvent, EngineResult, Job, RunningJob, SnapshotSummary,
};
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
}

#[allow(clippy::result_large_err)]
impl ListSnapshots {
    pub fn new(engine: Arc<dyn Engine>, resolve: ResolveRepository) -> Self {
        Self { engine, resolve }
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
#[allow(clippy::result_large_err)]
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
    use crate::fakes::{FakeSecrets, InMemoryConfigStore};
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

    #[allow(clippy::panic)]
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

    #[allow(clippy::panic)]
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

    // Fixed demo values; the `panic` they guard never happens, and a helper
    // outside a test body is production code to clippy.
    #[allow(clippy::panic)]
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
}
