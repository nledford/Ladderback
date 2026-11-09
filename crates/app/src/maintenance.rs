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
use std::pin::pin;
use std::sync::Arc;

use domain::maintenance::{ForgetSelection, ManagedPlan, Partition, Snapshot};
use domain::planning::AgentConfig;
use domain::{AgentId, OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration};
use futures::StreamExt;
use futures::future::FutureExt;
use futures::select_biased;

use crate::execution::Finished;
use crate::ports::{
    CheckJob, Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, ForgetJob, Job,
    OperationStore, PruneJob, RepositoryAccess, Retention, RetentionError, RunningJob,
    SnapshotSummary, Start, StoreError,
};
use crate::secrets::{ResolveError, ResolveRepository};
use domain::execution::deadline::KeepDelete;
use domain::execution::{FailureReason, Operation, Trigger};

/// The `keep-delete` a manual prune uses when nothing else says: six days,
/// just under the weekly interval a prune is expected to run on (§7.3).
#[allow(
    clippy::expect_used,
    reason = "six days is always at least rustic's own 23-hour minimum"
)]
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
    /// The configuration couldn't be read.
    Unavailable,
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
    pub async fn prepare(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        kind: MaintenanceKind,
    ) -> Result<PreparedMaintenance, MaintenanceError> {
        let config = self
            .config
            .load()
            .await
            .map_err(|_| MaintenanceError::Unavailable)?;
        let repository = config
            .repository(alias)
            .ok_or(MaintenanceError::UnknownRepository)?;
        let mut access = self
            .resolve
            .resolve(&repository.settings)
            .await
            .map_err(MaintenanceError::Resolve)?
            .access;
        // The repository's ID names it in the log, which is what I7 holds
        // it by. A read reports it; nothing else runs before the slot is
        // taken.
        let id = identify(self.engine.as_ref(), op, access.clone()).await?;
        access.id = Some(id);

        let (partition, job) = match kind {
            MaintenanceKind::Forget => {
                let listed = list(self.engine.as_ref(), op, access.clone()).await?;
                let snapshots: Vec<Snapshot> = listed.iter().map(selected).collect();
                let plans = plans_of(&config, alias);
                // Checked before anything is selected: a policy that would
                // keep nothing removes the plan's whole history, which is
                // the user's decision to make explicitly, not a rule's.
                for plan in &plans {
                    if plan.policy.keeps_nothing() {
                        return Err(MaintenanceError::KeepsNothing {
                            plan: plan.plan.clone(),
                        });
                    }
                }
                let partition = ForgetSelection::partition(&snapshots, &self.agent, &plans);
                let mut ids = Vec::new();
                for plan in &plans {
                    let Some(group) = partition
                        .managed
                        .iter()
                        .find(|group| group.plan == plan.plan)
                    else {
                        continue;
                    };
                    ids.extend(
                        self.retention
                            .removes(&access, &plan.policy, &group.snapshots)
                            .await
                            .map_err(retention_failed)?,
                    );
                }
                (
                    Some(partition),
                    Job::Forget(ForgetJob {
                        repository: access,
                        ids,
                    }),
                )
            }
            MaintenanceKind::Prune => (
                None,
                Job::Prune(PruneJob {
                    repository: access,
                    keep_delete: default_keep_delete(),
                }),
            ),
            MaintenanceKind::Check => (
                None,
                Job::Check(CheckJob {
                    repository: access,
                    read_data: false,
                }),
            ),
        };

        let mut operation = Operation::queue(
            op,
            job.kind(),
            Trigger::Manual,
            job.target(),
            self.clock.now(),
        );
        #[allow(
            clippy::expect_used,
            reason = "the operation was queued a moment above and nothing else runs"
        )]
        operation
            .start(self.clock.now())
            .expect("a queued operation starts");
        match self.store.try_start(&operation).await {
            Ok(Start::Started) => Ok(PreparedMaintenance {
                operation,
                job,
                partition,
            }),
            Ok(Start::Refused { running }) => Err(MaintenanceError::Busy { running }),
            Err(error) => Err(MaintenanceError::Store(error)),
        }
    }

    /// Runs the prepared job to its end, playing the engine's events into
    /// the operation, and records the outcome. The operation is already
    /// running, so the engine's own `Started` changes nothing.
    pub async fn execute(
        &self,
        prepared: PreparedMaintenance,
        cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        let PreparedMaintenance {
            mut operation,
            job,
            partition: _,
        } = prepared;
        let mut result = None;
        let mut recording_errors = Vec::new();
        let RunningJob {
            events,
            mut control,
        } = match self.engine.run(operation.id(), job).await {
            Ok(running) => running,
            Err(error) => {
                let reason = match error {
                    EngineError::Unavailable { detail } => {
                        FailureReason::EngineUnavailable { detail }
                    }
                    EngineError::StartTimedOut => FailureReason::EngineStartTimedOut,
                };
                let _ = operation.fail(reason, self.clock.now());
                self.record(&operation, &result, &mut recording_errors)
                    .await;
                return Finished {
                    operation,
                    result,
                    recording_errors,
                };
            }
        };
        let mut cancel = pin!(cancel.fuse());
        let mut events = events.fuse();
        loop {
            select_biased! {
                event = events.next() => {
                    let Some(event) = event else {
                        let _ = operation.fail(
                            FailureReason::EngineExited { exit_code: None },
                            self.clock.now(),
                        );
                        break;
                    };
                    match event {
                        // The operation started before the engine did; its
                        // own `Started` is news from someone else's run.
                        EngineEvent::Started { .. } => {}
                        EngineEvent::Progress(_) => {
                            let _ = operation.report_progress();
                        }
                        EngineEvent::Warning(warning) => {
                            let _ = operation.record_warning(warning);
                        }
                        EngineEvent::Succeeded(reported) => {
                            if operation.succeed(self.clock.now()).is_ok() {
                                result = Some(reported);
                            }
                            break;
                        }
                        EngineEvent::Failed(reason) => {
                            let _ = operation.fail(reason, self.clock.now());
                            break;
                        }
                    }
                }
                () = cancel => {
                    control.stop().await;
                    let _ = operation.cancel(self.clock.now());
                    break;
                }
            }
        }
        self.record(&operation, &result, &mut recording_errors)
            .await;
        Finished {
            operation,
            result,
            recording_errors,
        }
    }

    /// Writes the operation as it stands. A refused write is noted for the
    /// caller and nothing more.
    async fn record(
        &self,
        operation: &Operation,
        result: &Option<EngineResult>,
        recording_errors: &mut Vec<StoreError>,
    ) {
        if let Err(error) = self.store.save(operation, result.as_ref()).await {
            recording_errors.push(error);
        }
    }
}

/// The plans of `config` that back up the repository `alias`, with their
/// retention policies: what a forget run on that repository manages.
fn plans_of(config: &AgentConfig, alias: &RepositoryAlias) -> Vec<ManagedPlan> {
    config
        .plans()
        .iter()
        .filter(|plan| plan.repository == *alias)
        .map(|plan| ManagedPlan {
            plan: plan.id.clone(),
            policy: plan.retention.clone(),
        })
        .collect()
}

/// One snapshot of a listing, as the selection reads it.
fn selected(summary: &SnapshotSummary) -> Snapshot {
    Snapshot {
        id: summary.id.clone(),
        time: summary.time,
        hostname: summary.hostname.clone(),
        label: summary.label.clone(),
        tags: summary.tags.clone(),
        paths: summary.paths.clone(),
    }
}

/// Opens the repository and reports its ID, as a read: no operation, no
/// log row, no slot.
async fn identify(
    engine: &dyn Engine,
    op: OperationId,
    access: RepositoryAccess,
) -> Result<RepositoryId, MaintenanceError> {
    match run_read(engine, op, Job::Identify(access)).await? {
        EngineResult::Repository { id } => Ok(id),
        _ => Err(MaintenanceError::Failed(FailureReason::EngineExited {
            exit_code: None,
        })),
    }
}

/// Lists the repository's snapshots, as a read.
async fn list(
    engine: &dyn Engine,
    op: OperationId,
    access: RepositoryAccess,
) -> Result<Vec<SnapshotSummary>, MaintenanceError> {
    match run_read(engine, op, Job::Ls(access)).await? {
        EngineResult::Snapshots(snapshots) => Ok(snapshots),
        _ => Err(MaintenanceError::Failed(FailureReason::EngineExited {
            exit_code: None,
        })),
    }
}

/// Runs a read-only job to its end and reports its result.
async fn run_read(
    engine: &dyn Engine,
    op: OperationId,
    job: Job,
) -> Result<EngineResult, MaintenanceError> {
    let RunningJob { events, .. } = engine.run(op, job).await.map_err(engine_error)?;
    let mut events = events.fuse();
    while let Some(event) = events.next().await {
        match event {
            EngineEvent::Succeeded(result) => return Ok(result),
            EngineEvent::Failed(reason) => return Err(MaintenanceError::Failed(reason)),
            _ => {}
        }
    }
    Err(MaintenanceError::Failed(FailureReason::EngineExited {
        exit_code: None,
    }))
}

fn engine_error(error: EngineError) -> MaintenanceError {
    match error {
        EngineError::Unavailable { detail } => MaintenanceError::Engine { detail },
        EngineError::StartTimedOut => MaintenanceError::Failed(FailureReason::EngineStartTimedOut),
    }
}

fn retention_failed(error: RetentionError) -> MaintenanceError {
    match error {
        RetentionError::Engine { detail } => MaintenanceError::Engine { detail },
        RetentionError::InvalidPolicy => MaintenanceError::Failed(FailureReason::EngineFailed {
            detail: "the retention policy is not one the engine accepts".to_owned(),
        }),
    }
}
