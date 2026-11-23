use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::{OperationId, RepositoryId, Timestamp};
use futures::stream::{self, StreamExt};

use crate::ports::{
    Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RunningJob, SnapshotSummary,
};

/// An engine of repositories and the snapshots each holds: a listing opens
/// the repository its access names and finds that repository's snapshots,
/// or fails when the repository was made to fail. It counts the listings
/// of each repository. Any other job is refused.
#[derive(Default)]
pub struct ListingEngine {
    held: Mutex<BTreeMap<RepositoryId, Vec<SnapshotSummary>>>,
    failing: Mutex<BTreeSet<RepositoryId>>,
    listed: Mutex<BTreeMap<RepositoryId, usize>>,
}

impl ListingEngine {
    /// `repository` holds `snapshots` from now on, in place of what it
    /// held.
    pub fn holds(&self, repository: RepositoryId, snapshots: Vec<SnapshotSummary>) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(repository, snapshots);
    }

    /// Each listing of `repository` fails from now on, or no longer fails.
    pub fn fails(&self, repository: RepositoryId, failing: bool) {
        let mut set = self.failing.lock().unwrap_or_else(PoisonError::into_inner);
        if failing {
            set.insert(repository);
        } else {
            set.remove(&repository);
        }
    }

    /// How many listings of `repository` ran.
    pub fn listings(&self, repository: &RepositoryId) -> usize {
        self.listed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(repository)
            .copied()
            .unwrap_or(0)
    }
}

struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for ListingEngine {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let Job::Ls(access) = job else {
            return Err(EngineError::Unavailable {
                detail: "the listing engine runs listings only".to_owned(),
            });
        };
        let Some(repository) = access.id else {
            return Err(EngineError::Unavailable {
                detail: "the listing engine lists only a repository whose ID is known".to_owned(),
            });
        };
        *self
            .listed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(repository.clone())
            .or_default() += 1;
        let failing = self
            .failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&repository);
        let end = if failing {
            EngineEvent::Failed(FailureReason::EngineFailed {
                detail: "the repository couldn't be read".to_owned(),
            })
        } else {
            let snapshots = self
                .held
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&repository)
                .cloned()
                .unwrap_or_default();
            EngineEvent::Succeeded(EngineResult::Snapshots(snapshots))
        };
        let events = vec![
            EngineEvent::Started {
                index_read_at: Timestamp::UNIX_EPOCH,
                repository: Some(repository),
            },
            end,
        ];
        Ok(RunningJob {
            events: stream::iter(events).boxed(),
            control: Box::new(Stop),
        })
    }
}
