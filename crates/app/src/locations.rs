//! Choosing which location of a repository a job opens (technical design
//! §6.1, decision 80). Before a job that opens a repository reached at
//! several locations, every location is probed in order, each by a
//! short-lived worker of its own with only its own secrets, and the first
//! that answers with the repository's ID is used: a location that holds
//! another repository is refused and never used. The job then carries that
//! location alone, so a location the job didn't use never had its secrets in
//! the job's process (I4).

use std::sync::Arc;

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::locations::LocationsRecord;
use domain::planning::ExposedCredentials;
use domain::{OperationId, RepositoryAlias, SignedDuration};

use crate::ports::{Clock, Engine, EngineError, Job, RepositoryAccess, RunningJob};
use crate::secrets::ResolveRepository;

/// What the choice tells the daemon's log. Nothing here is a secret or a
/// network's name: a location is named by its position, from 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocationReport {
    /// A location, as its references resolved, sends its credentials where
    /// whoever answers at its address can read them: a warning in the log
    /// for that job, never one of the operation (§10.6).
    ExposedCredentials {
        alias: RepositoryAlias,
        position: u8,
        cause: ExposedCredentials,
    },
    /// A location holds a repository whose ID isn't the entry's.
    OtherRepository {
        alias: RepositoryAlias,
        position: u8,
    },
    /// Which location a job used, and why each earlier one was skipped.
    Chosen {
        alias: RepositoryAlias,
        record: LocationsRecord,
    },
}

/// A repository access with its location chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The chosen location's access, with only its own secrets.
    pub access: RepositoryAccess,
    /// Every location as the choice saw it; `None` for a repository of one
    /// location, which is opened as it always was, with no probe.
    pub record: Option<LocationsRecord>,
}

/// An [`Engine`] that chooses, before a job that opens a repository reached
/// at several locations, the location the job opens.
pub struct ChooseLocation {
    inner: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    resolve: ResolveRepository,
    report: Arc<dyn Fn(LocationReport) + Send + Sync>,
    bound: SignedDuration,
}

impl ChooseLocation {
    /// Probes through `inner`, each probe bounded by
    /// [`LOCATION_PROBE_SECONDS`](domain::planning::LOCATION_PROBE_SECONDS)
    /// on `clock`, resolves each location's references with `resolve` when
    /// it is about to be probed, and tells `report` what the log says.
    pub fn new(
        inner: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        resolve: ResolveRepository,
        report: Arc<dyn Fn(LocationReport) + Send + Sync>,
    ) -> Self {
        let _ = (inner, clock, resolve, report);
        todo!()
    }

    /// The same, with each probe bounded by `bound` instead.
    pub fn with_probe_bound(self, bound: SignedDuration) -> Self {
        let _ = bound;
        todo!()
    }

    /// Chooses the location `access` opens, for operation `op`: the access
    /// itself for a repository of one location, else the first location that
    /// answers with the entry's repository ID. Fails with the reason the
    /// operation ends with when none can be used.
    pub async fn locate(
        &self,
        op: OperationId,
        access: RepositoryAccess,
    ) -> Result<Located, (FailureReason, Option<LocationsRecord>)> {
        let _ = (op, access);
        todo!()
    }
}

#[async_trait]
impl Engine for ChooseLocation {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let _ = (op, job);
        todo!()
    }
}
