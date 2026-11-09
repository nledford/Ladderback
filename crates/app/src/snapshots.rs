//! The snapshot-list use case: resolves the repository's secrets, lists its
//! snapshots with a `Job::Ls`, and returns them newest first. A read is not
//! an operation (§9): it takes no I7 slot, writes nothing to the operation
//! log, and the UI fetches the list live — nothing stores it.

use std::sync::Arc;

use domain::OperationId;
use domain::execution::FailureReason;
use domain::planning::RepositorySettings;
use futures::StreamExt;

use crate::ports::{
    Engine, EngineError, EngineEvent, EngineResult, Job, RepositoryAccess, RunningJob,
    SnapshotSummary,
};
use crate::secrets::{ResolveError, ResolveRepository};

/// Lists a repository's snapshots. Saves nothing: the list is fetched live
/// every time, for a read is safe to run alongside anything (§9).
#[allow(dead_code)] // phase 1: the body is stubbed; phase 2 wires these up
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

    /// Lists the repository's snapshots, newest first.
    pub async fn list(
        &self,
        op: OperationId,
        settings: &RepositorySettings,
    ) -> Result<Vec<SnapshotSummary>, SnapshotError> {
        let _ = (op, settings);
        todo!("phase 2: run Job::Ls and return its list, newest first")
    }
}

/// The settings with every reference resolved, ready for a job.
#[allow(clippy::result_large_err)]
async fn access(
    resolve: &ResolveRepository,
    settings: &RepositorySettings,
) -> Result<RepositoryAccess, SnapshotError> {
    Ok(resolve
        .resolve(settings)
        .await
        .map_err(SnapshotError::Resolve)?
        .access)
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
            EngineEvent::Failed(reason) => return Err(SnapshotError::Failed(reason)),
            _ => {}
        }
    }
    // The stream ended without a result: the engine stopped without reporting.
    Err(SnapshotError::Failed(FailureReason::EngineExited {
        exit_code: None,
    }))
}

#[cfg(all(test, feature = "fakes"))]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::FailureReason;
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryId, SnapshotId, Timestamp, Uuid};
    use std::collections::BTreeMap;

    use super::{ListSnapshots, SnapshotError};
    use crate::fakes::FakeSecrets;
    use crate::ports::{
        Engine, EngineError, EngineResult, Job, JobControl, RunningJob, SnapshotSummary,
    };
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
            id: SnapshotId::parse(&format!("{n:0>64}")).expect("64 hex digits"),
            time: Timestamp::from_second(at).expect("a valid instant"),
            hostname: "demo-host".to_owned(),
            label: label.to_owned(),
            tags: Vec::new(),
            paths: vec!["/home/demo".to_owned()],
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
            .list(op(), &settings())
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
        }
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
            use_case.list(op(), &settings()).await,
            Err(SnapshotError::Failed(reason))
        );
        let _ = repository();
    }
}
