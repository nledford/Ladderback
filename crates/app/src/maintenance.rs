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
use std::pin::{Pin, pin};
use std::sync::Arc;

use domain::maintenance::{
    AdoptSelection, AdoptionRefusal, ForgetSelection, ManagedPlan, Partition, Snapshot,
};
use domain::planning::{AgentConfig, Plan};
use domain::{
    AgentId, OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration, SnapshotId,
};
use futures::StreamExt;
use futures::future::{FusedFuture, FutureExt};
use futures::select_biased;

use crate::execution::Finished;
use crate::log_write::save_within_bound;
use crate::ports::{
    CheckJob, Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, ForgetJob, Job,
    MAX_JOB_IDS, NormalizeSources, OperationStore, PruneJob, RepositoryAccess, Retention,
    RetentionError, RewriteJob, RunningJob, SnapshotSummary, SourcesError, Start, StoreError,
};
use crate::secrets::{ResolveError, ResolveRepository};
use domain::execution::deadline::KeepDelete;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{FailureReason, Operation, Trigger, Warning, WarningKind};

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

/// How long `prepare` waits for each of its reads (the repository's ID, its
/// snapshot list) before it gives up. A repository that never answers would
/// otherwise hold the caller's request open for ever.
pub const READ_TIMEOUT: SignedDuration = SignedDuration::from_mins(10);

/// Which maintenance operation to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaintenanceKind {
    Forget,
    /// Rewrites one foreign group with the tags of `into`, the plan it
    /// becomes history of (§7.3, step 3).
    Adopt(Adoption),
    Prune,
    Check,
}

/// One adoption: the group to rewrite, and the plan it becomes history of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adoption {
    pub from: AdoptionSource,
    pub into: PlanId,
}

/// The group an adoption rewrites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdoptionSource {
    /// The orphaned group these two tags name: another agent's plan, or a
    /// plan this configuration no longer has.
    Orphaned {
        /// The `ladderback-agent:` tag's text.
        agent: String,
        /// The `ladderback-plan:` tag's text.
        plan: String,
    },
    /// The unmanaged group whose host, label and paths equal the destination
    /// plan's host, label and sources.
    Unmanaged,
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
    /// A read of the repository (its ID, its snapshot list) was not answered
    /// within [`READ_TIMEOUT`]. Nothing was stored and the engine was
    /// stopped.
    TimedOut,
    /// A plan's policy is set and would keep nothing: the run is refused
    /// before anything is removed, and the plan is named.
    KeepsNothing {
        plan: PlanId,
    },
    /// The configuration holds no plan to adopt into, under the repository
    /// asked about. The argument `plan` names it.
    UnknownPlan {
        plan: PlanId,
    },
    /// The group an adoption named matches no plan of this agent: nothing
    /// was written.
    AdoptionNoMatch,
    /// More than one plan matches the group an adoption named. The plans
    /// are named; which one the history belongs to is the owner's decision.
    AdoptionAmbiguous {
        plans: Vec<PlanId>,
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
    /// The host name this agent's own backups carry: the fallback a plan
    /// without a host of its own matches an unmanaged group by.
    hostname: String,
    /// A plan's sources as the engine records them, so adoption's matching
    /// compares like with like.
    sources: Arc<dyn NormalizeSources>,
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
        hostname: String,
        sources: Arc<dyn NormalizeSources>,
    ) -> Self {
        Self {
            engine,
            clock,
            store,
            resolve,
            retention,
            config,
            agent,
            hostname,
            sources,
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
        let id = identify(
            self.engine.as_ref(),
            self.clock.as_ref(),
            op,
            access.clone(),
        )
        .await?;
        access.id = Some(id);

        let (partition, job) = match kind {
            MaintenanceKind::Forget => {
                let listed = list(
                    self.engine.as_ref(),
                    self.clock.as_ref(),
                    op,
                    access.clone(),
                )
                .await?;
                let snapshots: Vec<Snapshot> = listed.iter().map(selected).collect();
                let plans = plans_of(&config, alias);
                let partition = ForgetSelection::partition(&snapshots, &self.agent, &plans);
                let mut ids = Vec::new();
                for plan in &plans {
                    let Some(group) = partition
                        .managed
                        .iter()
                        .find(|group| group.plan == plan.plan)
                    else {
                        // A plan with no snapshots has nothing a policy
                        // could remove, so its policy refuses nothing.
                        continue;
                    };
                    // Checked per group, before anything is selected: a
                    // policy that would keep nothing removes the plan's
                    // whole history, which is the user's decision to make
                    // explicitly, not a rule's.
                    if plan.policy.keeps_nothing() {
                        return Err(MaintenanceError::KeepsNothing {
                            plan: plan.plan.clone(),
                        });
                    }
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
            MaintenanceKind::Adopt(adoption) => {
                // Only a plan of this repository can take history: a plan of
                // another repository, or none at all, would leave the
                // rewritten snapshots orphaned again.
                let plans: Vec<Plan> = config
                    .plans()
                    .iter()
                    .filter(|plan| plan.repository == *alias)
                    .cloned()
                    .collect();
                if !plans.iter().any(|plan| plan.id == adoption.into) {
                    return Err(MaintenanceError::UnknownPlan {
                        plan: adoption.into.clone(),
                    });
                }
                let listed = list(
                    self.engine.as_ref(),
                    self.clock.as_ref(),
                    op,
                    access.clone(),
                )
                .await?;
                let snapshots: Vec<Snapshot> = listed.iter().map(selected).collect();
                let managed = plans_of(&config, alias);
                let partition = ForgetSelection::partition(&snapshots, &self.agent, &managed);
                let ids: Vec<SnapshotId> = match &adoption.from {
                    AdoptionSource::Orphaned { agent, plan } => {
                        let Some(group) = AdoptSelection::orphaned(&partition, agent, plan) else {
                            return Err(MaintenanceError::AdoptionNoMatch);
                        };
                        group.snapshots.iter().map(|s| s.id.clone()).collect()
                    }
                    AdoptionSource::Unmanaged => {
                        // The destination plan is among `plans`, checked
                        // above.
                        let hostname = config
                            .agent()
                            .hostname
                            .as_ref()
                            .map(|host| host.as_str().to_owned())
                            .unwrap_or_else(|| self.hostname.clone());
                        // Every candidate plan's sources as the engine
                        // records them, so the match compares like with
                        // like: rustic resolves symlinks when it records a
                        // snapshot's paths.
                        let normalized: Vec<Plan> =
                            futures::future::try_join_all(plans.iter().map(|plan| async {
                                self.sources.normalized(&plan.sources).await.map(|sources| {
                                    let mut normalized = plan.clone();
                                    normalized.sources = sources;
                                    normalized
                                })
                            }))
                            .await
                            .map_err(sources_failed)?;
                        // Every plan of `plans` is among the normalized
                        // ones, so the destination is too.
                        let into = normalized
                            .iter()
                            .find(|plan| plan.id == adoption.into)
                            .ok_or(MaintenanceError::UnknownPlan {
                                plan: adoption.into.clone(),
                            })?;
                        let group =
                            AdoptSelection::unmanaged(&partition, &hostname, into, &normalized)
                                .map_err(|refusal| match refusal {
                                    AdoptionRefusal::NoMatch => MaintenanceError::AdoptionNoMatch,
                                    AdoptionRefusal::Ambiguous { plans } => {
                                        MaintenanceError::AdoptionAmbiguous { plans }
                                    }
                                })?;
                        group.snapshots.iter().map(|s| s.id.clone()).collect()
                    }
                };
                let tags = PlanTags::new(adoption.into.clone(), self.agent);
                (
                    Some(partition),
                    Job::Rewrite(RewriteJob {
                        repository: access,
                        ids,
                        tags,
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
    ///
    /// A forget or rewrite that names more snapshots than one engine job
    /// takes ([`MAX_JOB_IDS`]) runs as several jobs one after the other, as
    /// one operation with one summary. Each job removes or rewrites only
    /// snapshots the selection named, so a run that ends partway (a failure,
    /// a cancel) leaves the earlier jobs' work done and the rest untouched;
    /// the operation says so in a warning, and a later run selects what is
    /// left.
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
        let mut cancel = pin!(cancel.fuse());
        let jobs = split(job);
        let total: usize = jobs.iter().map(names).sum();
        let last = jobs.len().saturating_sub(1);
        // What the jobs finished so far reported, merged into one.
        let mut done: Option<EngineResult> = None;
        for (index, job) in jobs.into_iter().enumerate() {
            // A cancel that came while the last job was finishing is seen
            // before another worker is started.
            if index > 0 && futures::poll!(cancel.as_mut()).is_ready() {
                note_partial(&mut operation, done.as_ref(), total);
                let _ = operation.cancel(self.clock.now());
                break;
            }
            let running = match self.engine.run(operation.id(), job).await {
                Ok(running) => running,
                Err(error) => {
                    let reason = match error {
                        EngineError::Unavailable { detail } => {
                            FailureReason::EngineUnavailable { detail }
                        }
                        EngineError::StartTimedOut => FailureReason::EngineStartTimedOut,
                    };
                    note_partial(&mut operation, done.as_ref(), total);
                    let _ = operation.fail(reason, self.clock.now());
                    break;
                }
            };
            match self
                .play(
                    &mut operation,
                    running,
                    cancel.as_mut(),
                    done.as_ref(),
                    total,
                )
                .await
            {
                Played::Reported(reported) => {
                    let merged = merge(done.take(), reported);
                    if index == last {
                        if operation.succeed(self.clock.now()).is_ok() {
                            result = Some(merged);
                        }
                        break;
                    }
                    done = Some(merged);
                }
                Played::Ended => break,
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

    /// Plays one engine job's events into the operation until it reports a
    /// result (the operation is left running for the caller to settle) or
    /// the operation ends: failed, or cancelled. `done` is what the jobs
    /// before this one finished, and `total` how many snapshots the whole
    /// run names.
    async fn play(
        &self,
        operation: &mut Operation,
        running: RunningJob,
        mut cancel: Pin<&mut (impl FusedFuture<Output = ()> + Send)>,
        done: Option<&EngineResult>,
        total: usize,
    ) -> Played {
        let RunningJob {
            events,
            mut control,
        } = running;
        // A cancel made before the engine was up is honoured at once, not
        // raced against the events the engine already has ready: the client
        // was told it was accepted.
        if futures::poll!(cancel.as_mut()).is_ready() {
            control.stop().await;
            note_partial(operation, done, total);
            let _ = operation.cancel(self.clock.now());
            return Played::Ended;
        }
        let mut events = events.fuse();
        loop {
            select_biased! {
                event = events.next() => {
                    let Some(event) = event else {
                        note_partial(operation, done, total);
                        let _ = operation.fail(
                            FailureReason::EngineExited { exit_code: None },
                            self.clock.now(),
                        );
                        return Played::Ended;
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
                        EngineEvent::Succeeded(reported) => return Played::Reported(reported),
                        EngineEvent::Failed(reason) => {
                            note_partial(operation, done, total);
                            let _ = operation.fail(reason, self.clock.now());
                            return Played::Ended;
                        }
                    }
                }
                () = cancel => {
                    control.stop().await;
                    note_partial(operation, done, total);
                    let _ = operation.cancel(self.clock.now());
                    return Played::Ended;
                }
            }
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
        let saved = save_within_bound(&*self.clock, &*self.store, operation, result.as_ref()).await;
        if let Err(error) = saved {
            recording_errors.push(error);
        }
    }
}

/// How one engine job ended, as `execute` plays it.
enum Played {
    /// The job reported this result; the operation is still running.
    Reported(EngineResult),
    /// The operation ended: failed, or cancelled.
    Ended,
}

/// The job as the engine jobs that do it: a forget or rewrite is cut into
/// jobs of at most [`MAX_JOB_IDS`] snapshots each, in order (one job when they
/// all fit, and one naming none stays that one); any other job stays whole.
fn split(job: Job) -> Vec<Job> {
    match job {
        Job::Forget(job) => chunks(&job.ids)
            .map(|ids| {
                Job::Forget(ForgetJob {
                    repository: job.repository.clone(),
                    ids: ids.to_vec(),
                })
            })
            .collect(),
        Job::Rewrite(job) => chunks(&job.ids)
            .map(|ids| {
                Job::Rewrite(RewriteJob {
                    repository: job.repository.clone(),
                    ids: ids.to_vec(),
                    tags: job.tags.clone(),
                })
            })
            .collect(),
        job => vec![job],
    }
}

/// `ids` in consecutive parts of at most [`MAX_JOB_IDS`]; no ids make one
/// empty part, so a job is never lost.
fn chunks(ids: &[SnapshotId]) -> impl Iterator<Item = &[SnapshotId]> {
    (0..ids.len().max(1)).step_by(MAX_JOB_IDS).map(|start| {
        ids.get(start..ids.len().min(start + MAX_JOB_IDS))
            .unwrap_or_default()
    })
}

/// The result of the jobs so far with the next one's added: the snapshots
/// removed or created, in order. Any other result stands alone.
fn merge(done: Option<EngineResult>, next: EngineResult) -> EngineResult {
    match (done, next) {
        (Some(EngineResult::Forget(mut all)), EngineResult::Forget(next)) => {
            all.removed.extend(next.removed);
            EngineResult::Forget(all)
        }
        (Some(EngineResult::Rewrite(mut all)), EngineResult::Rewrite(next)) => {
            all.created.extend(next.created);
            EngineResult::Rewrite(all)
        }
        (_, next) => next,
    }
}

/// How many snapshots a job names; none for a job that names none.
fn names(job: &Job) -> usize {
    match job {
        Job::Forget(job) => job.ids.len(),
        Job::Rewrite(job) => job.ids.len(),
        _ => 0,
    }
}

/// Warns that the run ends with some of its jobs already done, which stay
/// done. Called before the operation ends, while it can still take a
/// warning. The warning carries no English (§8.4): `during` is `forget` or
/// `adopt` and `detail` is `<done>/<total>`, how many snapshots the finished
/// jobs handled of how many the run names, which are the arguments of
/// `WarningCode::MaintenancePartial`.
fn note_partial(operation: &mut Operation, done: Option<&EngineResult>, total: usize) {
    let (during, handled) = match done {
        Some(EngineResult::Forget(done)) => ("forget", done.removed.len()),
        Some(EngineResult::Rewrite(done)) => ("adopt", done.created.len()),
        _ => return,
    };
    let _ = operation.record_warning(Warning {
        kind: WarningKind::MaintenancePartial,
        item: None,
        during: during.to_owned(),
        detail: format!("{handled}/{total}"),
    });
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
    clock: &dyn Clock,
    op: OperationId,
    access: RepositoryAccess,
) -> Result<RepositoryId, MaintenanceError> {
    match run_read(engine, clock, op, Job::Identify(access)).await? {
        EngineResult::Repository { id } => Ok(id),
        _ => Err(MaintenanceError::Failed(FailureReason::EngineExited {
            exit_code: None,
        })),
    }
}

/// Lists the repository's snapshots, as a read.
async fn list(
    engine: &dyn Engine,
    clock: &dyn Clock,
    op: OperationId,
    access: RepositoryAccess,
) -> Result<Vec<SnapshotSummary>, MaintenanceError> {
    match run_read(engine, clock, op, Job::Ls(access)).await? {
        EngineResult::Snapshots(snapshots) => Ok(snapshots),
        _ => Err(MaintenanceError::Failed(FailureReason::EngineExited {
            exit_code: None,
        })),
    }
}

/// Runs a read-only job to its end and reports its result. A read not
/// answered within [`READ_TIMEOUT`] is given up: dropping the job stops the
/// engine.
async fn run_read(
    engine: &dyn Engine,
    clock: &dyn Clock,
    op: OperationId,
    job: Job,
) -> Result<EngineResult, MaintenanceError> {
    let read = async {
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
    };
    let give_up = clock.sleep_until(clock.now() + READ_TIMEOUT);
    // The read first: one that is ready is never lost to a timer.
    select_biased! {
        result = pin!(read.fuse()) => result,
        () = pin!(give_up.fuse()) => Err(MaintenanceError::TimedOut),
    }
}

fn engine_error(error: EngineError) -> MaintenanceError {
    match error {
        EngineError::Unavailable { detail } => MaintenanceError::Engine { detail },
        EngineError::StartTimedOut => MaintenanceError::Failed(FailureReason::EngineStartTimedOut),
    }
}

fn sources_failed(error: SourcesError) -> MaintenanceError {
    let SourcesError::Unreadable { detail } = error;
    MaintenanceError::Engine { detail }
}

fn retention_failed(error: RetentionError) -> MaintenanceError {
    match error {
        RetentionError::Engine { detail } => MaintenanceError::Engine { detail },
        RetentionError::InvalidPolicy => MaintenanceError::Failed(FailureReason::EngineFailed {
            detail: "the retention policy is not one the engine accepts".to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use domain::secrets::SecretValue;
    use domain::{AgentId, Timestamp, Uuid};

    use crate::ports::{ForgetSummary, RewriteSummary};

    fn access() -> RepositoryAccess {
        RepositoryAccess {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: None,
        }
    }

    fn ids(count: usize) -> Vec<SnapshotId> {
        (0..count)
            .map(|n| SnapshotId::parse(&format!("{n:064x}")).expect("a snapshot ID"))
            .collect()
    }

    fn forget(count: usize) -> Job {
        Job::Forget(ForgetJob {
            repository: access(),
            ids: ids(count),
        })
    }

    fn sizes(jobs: &[Job]) -> Vec<usize> {
        jobs.iter()
            .map(|job| match job {
                Job::Forget(job) => job.ids.len(),
                Job::Rewrite(job) => job.ids.len(),
                _ => 0,
            })
            .collect()
    }

    #[test]
    fn a_forget_at_the_limit_of_one_job_stays_one_job() {
        assert_eq!(sizes(&split(forget(MAX_JOB_IDS))), [MAX_JOB_IDS]);
        assert_eq!(sizes(&split(forget(1))), [1]);
    }

    #[test]
    fn a_forget_naming_no_snapshot_still_runs_as_one_job() {
        assert_eq!(sizes(&split(forget(0))), [0]);
    }

    #[test]
    fn a_forget_one_past_the_limit_runs_as_two_jobs_in_order() {
        let jobs = split(forget(MAX_JOB_IDS + 1));
        assert_eq!(sizes(&jobs), [MAX_JOB_IDS, 1]);
        let named: Vec<SnapshotId> = jobs
            .iter()
            .flat_map(|job| match job {
                Job::Forget(job) => job.ids.clone(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(named, ids(MAX_JOB_IDS + 1), "every one once, in order");
    }

    #[test]
    fn a_rewrite_past_the_limit_splits_with_the_same_tags_in_each_job() {
        let tags = PlanTags::new(
            PlanId::parse("home").expect("a plan ID"),
            AgentId::from_uuid(Uuid::from_u128(1)),
        );
        let jobs = split(Job::Rewrite(RewriteJob {
            repository: access(),
            ids: ids(2 * MAX_JOB_IDS + 3),
            tags: tags.clone(),
        }));
        assert_eq!(sizes(&jobs), [MAX_JOB_IDS, MAX_JOB_IDS, 3]);
        assert!(
            jobs.iter()
                .all(|job| matches!(job, Job::Rewrite(job) if job.tags == tags))
        );
    }

    #[test]
    fn a_rewrite_splits_at_the_same_limit_as_a_forget() {
        let tags = PlanTags::new(
            PlanId::parse("home").expect("a plan ID"),
            AgentId::from_uuid(Uuid::from_u128(1)),
        );
        let rewrite = |count: usize| {
            Job::Rewrite(RewriteJob {
                repository: access(),
                ids: ids(count),
                tags: tags.clone(),
            })
        };
        assert_eq!(sizes(&split(rewrite(MAX_JOB_IDS))), [MAX_JOB_IDS]);
        assert_eq!(sizes(&split(rewrite(MAX_JOB_IDS + 1))), [MAX_JOB_IDS, 1]);
        assert_eq!(sizes(&split(rewrite(0))), [0]);
    }

    fn running() -> Operation {
        let mut operation = Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(1)),
            domain::execution::OperationKind::Forget,
            Trigger::Manual,
            domain::execution::Target {
                plan: None,
                repository: None,
            },
            Timestamp::UNIX_EPOCH,
        );
        operation
            .start(Timestamp::UNIX_EPOCH)
            .expect("a queued operation starts");
        operation
    }

    #[test]
    fn a_run_that_ends_after_some_jobs_says_how_many_snapshots_they_did() {
        let repository = RepositoryId::parse(&"07".repeat(32)).expect("a repository ID");
        let mut forgot = running();
        note_partial(&mut forgot, Some(&removed(&repository, ids(3))), 12);
        let [warning] = forgot.warnings() else {
            panic!("one warning: {:?}", forgot.warnings());
        };
        assert_eq!(warning.kind, WarningKind::MaintenancePartial);
        assert_eq!(warning.during, "forget");
        assert_eq!(warning.detail, "3/12");
        let mut adopted = running();
        let created = EngineResult::Rewrite(RewriteSummary {
            repository,
            created: ids(2),
        });
        note_partial(&mut adopted, Some(&created), 7);
        let [warning] = adopted.warnings() else {
            panic!("one warning: {:?}", adopted.warnings());
        };
        assert_eq!(warning.kind, WarningKind::MaintenancePartial);
        assert_eq!(warning.during, "adopt");
        assert_eq!(warning.detail, "2/7");
    }

    #[test]
    fn a_run_that_ends_before_any_job_finished_has_nothing_to_say() {
        let mut operation = running();
        note_partial(&mut operation, None, 12);
        assert!(operation.warnings().is_empty());
    }

    #[test]
    fn a_job_names_the_snapshots_it_acts_on_and_a_prune_names_none() {
        let tags = PlanTags::new(
            PlanId::parse("home").expect("a plan ID"),
            AgentId::from_uuid(Uuid::from_u128(1)),
        );
        assert_eq!(names(&forget(3)), 3);
        assert_eq!(
            names(&Job::Rewrite(RewriteJob {
                repository: access(),
                ids: ids(4),
                tags,
            })),
            4
        );
        assert_eq!(
            names(&Job::Prune(PruneJob {
                repository: access(),
                keep_delete: default_keep_delete(),
            })),
            0
        );
    }

    #[test]
    fn a_job_that_names_no_snapshots_is_never_split() {
        let prune = Job::Prune(PruneJob {
            repository: access(),
            keep_delete: default_keep_delete(),
        });
        assert_eq!(split(prune.clone()), [prune]);
    }

    fn removed(repository: &RepositoryId, ids: Vec<SnapshotId>) -> EngineResult {
        EngineResult::Forget(ForgetSummary {
            repository: repository.clone(),
            removed: ids,
        })
    }

    #[test]
    fn the_results_of_several_forget_jobs_merge_in_order() {
        let repository = RepositoryId::parse(&"07".repeat(32)).expect("a repository ID");
        let all = ids(4);
        let first = removed(&repository, all[..3].to_vec());
        let second = removed(&repository, all[3..].to_vec());
        assert_eq!(merge(Some(first), second), removed(&repository, all));
    }

    #[test]
    fn the_results_of_several_rewrite_jobs_merge_in_order() {
        let repository = RepositoryId::parse(&"07".repeat(32)).expect("a repository ID");
        let all = ids(4);
        let created = |ids: Vec<SnapshotId>| {
            EngineResult::Rewrite(RewriteSummary {
                repository: repository.clone(),
                created: ids,
            })
        };
        assert_eq!(
            merge(Some(created(all[..1].to_vec())), created(all[1..].to_vec())),
            created(all)
        );
    }

    #[test]
    fn the_first_result_stands_alone() {
        let repository = RepositoryId::parse(&"07".repeat(32)).expect("a repository ID");
        let only = removed(&repository, ids(2));
        assert_eq!(merge(None, only.clone()), only);
    }

    #[test]
    fn a_manual_prune_keeps_packs_marked_for_six_days() {
        assert_eq!(
            default_keep_delete().get(),
            SignedDuration::from_hours(24 * 6),
            "just under the weekly prune interval (decision 9)"
        );
    }
}

#[cfg(all(test, feature = "fakes"))]
mod read_timeout_tests {
    #![allow(clippy::expect_used, reason = "test code")]

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use domain::secrets::SecretValue;
    use domain::{OperationId, SignedDuration, Timestamp, Uuid};
    use futures::StreamExt;
    use futures::stream;

    use super::{MaintenanceError, READ_TIMEOUT, run_read};
    use crate::fakes::FakeClock;
    use crate::ports::{Engine, EngineError, Job, JobControl, RepositoryAccess, RunningJob};

    struct Nothing;

    #[async_trait]
    impl JobControl for Nothing {
        async fn stop(&mut self) {}
    }

    /// An engine whose every job starts and then says nothing, as a
    /// repository that never answers does.
    struct Silent;

    #[async_trait]
    impl Engine for Silent {
        async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
            Ok(RunningJob {
                events: stream::pending().boxed(),
                control: Box::new(Nothing),
            })
        }
    }

    fn access() -> RepositoryAccess {
        RepositoryAccess {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: None,
        }
    }

    #[tokio::test]
    async fn a_read_nobody_answers_is_given_up_exactly_when_the_timeout_ends() {
        let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
        let waiting = Arc::clone(&clock);
        let read = tokio::spawn(async move {
            run_read(
                &Silent,
                waiting.as_ref(),
                OperationId::from_uuid(Uuid::from_u128(1)),
                Job::Identify(access()),
            )
            .await
        });
        for _ in 0..200 {
            if clock.sleepers() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(clock.sleepers(), 1, "the read set a timer");
        clock.advance(READ_TIMEOUT - SignedDuration::from_secs(1));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!read.is_finished(), "a second before the timeout ends");
        clock.advance(SignedDuration::from_secs(1));
        let outcome = tokio::time::timeout(Duration::from_secs(5), read)
            .await
            .expect("the read ends when the timeout does")
            .expect("the read task ran");
        assert_eq!(outcome.unwrap_err(), MaintenanceError::TimedOut);
    }
}

#[cfg(all(test, feature = "fakes"))]
mod record_tests {
    #![allow(clippy::expect_used, reason = "test code")]

    use std::sync::Arc;

    use async_trait::async_trait;
    use domain::execution::{OperationKind, Target, Trigger};
    use domain::{AgentId, OperationId, Timestamp, Uuid};

    use super::*;
    use crate::fakes::{
        FakeClock, FakeRetention, FakeSecrets, FakeSources, InMemoryConfigStore,
        InMemoryOperationStore,
    };
    use crate::ports::{HistoryPage, HistoryQuery, OperationDetail, Start};

    /// An operation log whose writes never answer: a stalled disk.
    struct Stalled;

    #[async_trait]
    impl OperationStore for Stalled {
        async fn fail_unfinished(
            &self,
            _reason: &domain::execution::FailureReason,
        ) -> Result<u64, StoreError> {
            futures::future::pending().await
        }

        async fn save(
            &self,
            _operation: &Operation,
            _result: Option<&EngineResult>,
        ) -> Result<(), StoreError> {
            futures::future::pending().await
        }

        async fn try_start(&self, _operation: &Operation) -> Result<Start, StoreError> {
            futures::future::pending().await
        }

        async fn get(&self, _id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
            Ok(None)
        }

        async fn list(&self, _query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
            futures::future::pending().await
        }
    }

    fn use_case(store: Arc<dyn OperationStore>) -> RunMaintenance {
        RunMaintenance::new(
            Arc::new(crate::fakes::SimulatedEngine::new(Arc::new(
                FakeClock::auto_advancing(Timestamp::UNIX_EPOCH),
            ))),
            Arc::new(FakeClock::auto_advancing(Timestamp::UNIX_EPOCH)),
            store,
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            Arc::new(FakeRetention),
            Arc::new(InMemoryConfigStore::new()),
            AgentId::from_uuid(Uuid::from_u128(1)),
            "demo-host".to_owned(),
            Arc::new(FakeSources),
        )
    }

    fn operation() -> Operation {
        Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(1)),
            OperationKind::Check,
            Trigger::Manual,
            Target {
                plan: None,
                repository: None,
            },
            Timestamp::UNIX_EPOCH,
        )
    }

    #[tokio::test]
    async fn a_write_the_log_never_answers_is_given_up_on_and_noted() {
        let mut errors = Vec::new();
        let use_case = use_case(Arc::new(Stalled));
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            use_case.record(&operation(), &None, &mut errors),
        )
        .await
        .expect("the write is given up on");
        assert!(
            matches!(errors.as_slice(), [StoreError::Unavailable { .. }]),
            "{errors:?}"
        );
    }

    #[tokio::test]
    async fn a_write_the_log_answers_is_not_noted() {
        let mut errors = Vec::new();
        use_case(Arc::new(InMemoryOperationStore::new()))
            .record(&operation(), &None, &mut errors)
            .await;
        assert!(errors.is_empty(), "{errors:?}");
    }

    /// The double stays stalled for the one write of the log that no use case
    /// in this module issues: ending what a stopped daemon left unfinished.
    #[tokio::test]
    async fn a_stalled_log_never_answers_a_recovery_either() {
        let answered = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            Stalled.fail_unfinished(&domain::execution::FailureReason::DaemonStopped),
        )
        .await;
        assert!(answered.is_err(), "a stalled log answers nothing");
    }
}

#[cfg(all(test, feature = "fakes"))]
mod cancel_tests {
    #![allow(clippy::expect_used, reason = "test code")]

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use domain::execution::{OperationKind, OperationStatus, Target};
    use domain::secrets::SecretValue;
    use domain::{AgentId, OperationId, RepositoryId, SnapshotId, Timestamp, Uuid};
    use futures::StreamExt;
    use futures::stream;

    use super::*;
    use crate::fakes::{
        FakeClock, FakeRetention, FakeSecrets, FakeSources, InMemoryConfigStore,
        InMemoryOperationStore,
    };
    use crate::ports::{ForgetSummary, JobControl};

    /// A worker's control that counts the stops it is given.
    struct Stops(Arc<AtomicUsize>);

    #[async_trait]
    impl JobControl for Stops {
        async fn stop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// An engine that finishes every forget job at once and counts them. The
    /// first job to finish fires `cancel_with_the_first_result`, if there is
    /// one: the cancel is there when the job ends, not before it started.
    struct Counting {
        runs: Arc<AtomicUsize>,
        stops: Arc<AtomicUsize>,
        cancel_with_the_first_result:
            std::sync::Mutex<Option<futures::channel::oneshot::Sender<()>>>,
    }

    #[async_trait]
    impl Engine for Counting {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let Job::Forget(job) = job else {
                return Err(EngineError::StartTimedOut);
            };
            let result = EngineResult::Forget(ForgetSummary {
                repository: RepositoryId::parse(&"07".repeat(32)).expect("a repository ID"),
                removed: job.ids,
            });
            let cancel = self
                .cancel_with_the_first_result
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            Ok(RunningJob {
                events: stream::once(async move {
                    if let Some(cancel) = cancel {
                        let _ = cancel.send(());
                    }
                    EngineEvent::Succeeded(result)
                })
                .boxed(),
                control: Box::new(Stops(Arc::clone(&self.stops))),
            })
        }
    }

    fn prepared(count: usize) -> PreparedMaintenance {
        let mut operation = Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(1)),
            OperationKind::Forget,
            Trigger::Manual,
            Target {
                plan: None,
                repository: None,
            },
            Timestamp::UNIX_EPOCH,
        );
        operation
            .start(Timestamp::UNIX_EPOCH)
            .expect("a queued operation starts");
        PreparedMaintenance {
            operation,
            job: Job::Forget(ForgetJob {
                repository: RepositoryAccess {
                    repository: "/srv/demo/repo".to_owned(),
                    repo_hot: None,
                    password: SecretValue::new("demo-password"),
                    options: BTreeMap::new(),
                    options_hot: BTreeMap::new(),
                    options_cold: BTreeMap::new(),
                    secrets: Vec::new(),
                    id: None,
                },
                ids: (0..count)
                    .map(|n| SnapshotId::parse(&format!("{n:064x}")).expect("a snapshot ID"))
                    .collect(),
            }),
            partition: None,
        }
    }

    fn use_case(runs: &Arc<AtomicUsize>, stops: &Arc<AtomicUsize>) -> RunMaintenance {
        use_case_cancelling(runs, stops, None)
    }

    fn use_case_cancelling(
        runs: &Arc<AtomicUsize>,
        stops: &Arc<AtomicUsize>,
        cancel: Option<futures::channel::oneshot::Sender<()>>,
    ) -> RunMaintenance {
        RunMaintenance::new(
            Arc::new(Counting {
                runs: Arc::clone(runs),
                stops: Arc::clone(stops),
                cancel_with_the_first_result: std::sync::Mutex::new(cancel),
            }),
            Arc::new(FakeClock::auto_advancing(Timestamp::UNIX_EPOCH)),
            Arc::new(InMemoryOperationStore::new()),
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            Arc::new(FakeRetention),
            Arc::new(InMemoryConfigStore::new()),
            AgentId::from_uuid(Uuid::from_u128(1)),
            "demo-host".to_owned(),
            Arc::new(FakeSources),
        )
    }

    #[tokio::test]
    async fn a_cancel_that_is_already_there_when_a_job_ends_starts_no_further_job() {
        let runs = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        // Fires as the first job reports, before the second is asked for.
        let (fire, cancel) = futures::channel::oneshot::channel::<()>();
        let finished = use_case_cancelling(&runs, &stops, Some(fire))
            .execute(prepared(2 * MAX_JOB_IDS + 1), async move {
                let _ = cancel.await;
            })
            .await;
        assert_eq!(runs.load(Ordering::SeqCst), 1, "only the first job started");
        assert_eq!(finished.operation.status(), &OperationStatus::Cancelled);
        let [warning] = finished.operation.warnings() else {
            panic!("one warning: {:?}", finished.operation.warnings());
        };
        assert_eq!(warning.kind, WarningKind::MaintenancePartial);
        assert_eq!(warning.during, "forget");
        assert_eq!(
            warning.detail,
            format!("{MAX_JOB_IDS}/{}", 2 * MAX_JOB_IDS + 1),
            "the first job's snapshots of all the run names"
        );
        assert_eq!(finished.result, None);
    }

    #[tokio::test]
    async fn a_cancel_that_is_there_before_the_first_result_stops_the_worker_and_keeps_nothing() {
        let runs = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let finished = use_case(&runs, &stops)
            .execute(prepared(2 * MAX_JOB_IDS + 1), futures::future::ready(()))
            .await;
        assert_eq!(runs.load(Ordering::SeqCst), 1, "only the first job started");
        assert_eq!(stops.load(Ordering::SeqCst), 1, "its worker was stopped");
        assert_eq!(finished.operation.status(), &OperationStatus::Cancelled);
        assert!(
            finished.operation.warnings().is_empty(),
            "nothing had been done to report: {:?}",
            finished.operation.warnings()
        );
        assert_eq!(finished.result, None);
    }

    #[tokio::test]
    async fn without_a_cancel_every_job_runs_and_the_result_is_one() {
        let runs = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let finished = use_case(&runs, &stops)
            .execute(prepared(2 * MAX_JOB_IDS + 1), futures::future::pending())
            .await;
        assert_eq!(runs.load(Ordering::SeqCst), 3);
        assert_eq!(finished.operation.status(), &OperationStatus::Succeeded);
        let Some(EngineResult::Forget(summary)) = finished.result else {
            panic!("one merged result");
        };
        assert_eq!(summary.removed.len(), 2 * MAX_JOB_IDS + 1);
    }
}
