//! The maintenance use case: forget, prune and check, run manually by the
//! repository's local owner (technical design §7.3). A maintenance
//! operation takes its repository's I7 slot — `try_start` decides and
//! stores atomically — *before* the engine is started, and fails closed on
//! a refusal; nothing runs and nothing is stored then.
//!
//! Forget is decided, then done: the snapshots are listed (a read, no slot),
//! partitioned by who owns them (I6), each managed group's policy evaluated
//! through the `Retention` port, a policy that would keep nothing refused
//! before anything is removed, and the engine asked to remove exactly the
//! selected snapshots. Untagged and orphaned history is never named.

use std::future::Future;
use std::sync::Arc;

use domain::maintenance::Partition;
use domain::{AgentId, OperationId, PlanId, RepositoryAlias, SignedDuration};

use crate::execution::Finished;
use crate::ports::{Clock, ConfigStore, Engine, Job, OperationStore, Retention, StoreError};
use crate::secrets::{ResolveError, ResolveRepository};
use domain::execution::deadline::KeepDelete;
use domain::execution::{FailureReason, Operation};

/// The `keep-delete` a manual prune uses when nothing else says: six days,
/// just under the weekly interval a prune is expected to run on (§7.3).
pub fn default_keep_delete() -> KeepDelete {
    KeepDelete::new(SignedDuration::from_hours(24 * 6))
        .expect("six days is never shorter than rustic's own minimum")
}

/// Which maintenance operation to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceKind {
    Forget,
    Prune,
    Check,
}

/// Why a maintenance run was refused before it started. A typed code, never
/// a sentence (§8.4).
#[derive(Debug, PartialEq)]
pub enum MaintenanceError {
    /// The configuration holds no repository under the alias asked for.
    UnknownRepository,
    Resolve(ResolveError),
    /// The repository couldn't be opened to learn its ID or list its
    /// snapshots. `detail` is the engine's own message.
    Engine {
        detail: String,
    },
    /// The listing or the identify failed.
    Failed(FailureReason),
    /// A plan's policy is set and would keep nothing: the run is refused
    /// before anything is removed, and the plan is named.
    KeepsNothing {
        plan: PlanId,
    },
    /// Another maintenance operation, `running`, already holds the
    /// repository (I7): nothing ran and nothing was stored.
    Busy {
        running: OperationId,
    },
    /// The operation log couldn't be written.
    Store(StoreError),
}

/// A maintenance run with every decision made and its I7 slot held: the
/// engine has not been started yet. `execute` runs it to its end.
pub struct PreparedMaintenance {
    operation: Operation,
    job: Job,
    /// What the forget selection found, for the run's own record; `None`
    /// for prune and check.
    partition: Option<Partition>,
}

impl PreparedMaintenance {
    /// The operation as it stands: running, and holding its repository.
    pub fn operation(&self) -> &Operation {
        &self.operation
    }

    /// What the selection found in the repository, when the run is a forget.
    pub fn partition(&self) -> Option<&Partition> {
        self.partition.as_ref()
    }
}

/// Runs maintenance manually on the local owner.
#[allow(dead_code, reason = "red test: the run fills them next")]
pub struct RunMaintenance {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn OperationStore>,
    resolve: ResolveRepository,
    retention: Arc<dyn Retention>,
    config: Arc<dyn ConfigStore>,
    agent: AgentId,
}

impl RunMaintenance {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        store: Arc<dyn OperationStore>,
        resolve: ResolveRepository,
        retention: Arc<dyn Retention>,
        config: Arc<dyn ConfigStore>,
        agent: AgentId,
    ) -> Self {
        Self {
            engine,
            clock,
            store,
            resolve,
            retention,
            config,
            agent,
        }
    }

    /// Resolves the repository, decides what the run does, and takes the
    /// repository's I7 slot. On success the operation is running and
    /// recorded, and only `execute` talks to the engine; on any refusal or
    /// error the engine was never started and nothing of this run is in the
    /// log.
    #[allow(
        clippy::todo,
        reason = "red test: commit the scenarios before the run exists"
    )]
    pub async fn prepare(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        kind: MaintenanceKind,
    ) -> Result<PreparedMaintenance, MaintenanceError> {
        let _ = (op, alias, kind);
        todo!("resolve, select, take the I7 slot")
    }

    /// Runs the prepared job to its end, playing the engine's events into
    /// the operation, and records the outcome. The operation is already
    /// running, so the engine's own `Started` changes nothing.
    #[allow(
        clippy::todo,
        reason = "red test: commit the scenarios before the run exists"
    )]
    pub async fn execute(
        &self,
        prepared: PreparedMaintenance,
        cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        let _ = (&prepared.job, &cancel);
        todo!("drive the engine's events into the operation")
    }
}
