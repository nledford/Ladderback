//! The maintenance use case: forget, prune and check, run manually by the
//! repository's local owner (technical design §7.3). Only a repository whose
//! entry asks for `local` is maintained here (I1): any other is refused with
//! [`MaintenanceError::NotOwner`] before the engine or the log is reached,
//! previews included, though its history can still be listed. A maintenance
//! operation takes its repository's I7 slot — `try_start` decides and
//! stores atomically — *before* the engine is started, and fails closed on
//! a refusal; nothing runs and nothing is stored then.
//!
//! Forget is decided, then done: the snapshots are listed (a read, no slot),
//! partitioned by who owns them (I6), each managed group's policy evaluated
//! through the `Retention` port, a policy that would keep nothing refused
//! before anything is removed, and the engine asked to remove exactly the
//! selected snapshots. Untagged and orphaned history is never named.
//!
//! A run is assessed, then started. `assess` is the reads and the decisions,
//! and stores nothing: a caller dropped there has left nothing behind. `start`
//! is the write that takes the slot, and a `start` dropped while it waits for
//! the log may have taken it already, so a caller that can be dropped runs
//! `start` and `execute` on a task of its own (`RunMaintenance::start`).

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::Arc;

use domain::maintenance::{
    AdoptSelection, AdoptionRefusal, ForgetPreview, ForgetSelection, ManagedPlan, OrphanedGroup,
    OtherHosts, Ownership, Partition, SelectionToken, Snapshot, UnmanagedGroup,
};
use domain::planning::{AgentConfig, Plan};
use domain::{
    AgentId, OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration, SnapshotId,
};
use futures::StreamExt;
use futures::future::{FusedFuture, FutureExt};
use futures::select_biased;

use crate::announce::{announce_end, announce_start};
use crate::execution::Finished;
use crate::log_write::save_within_bound;
use crate::ports::{
    CheckJob, Clock, ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult,
    EventBus, ForgetJob, Job, MAX_JOB_IDS, NormalizeSources, OperationStore, PruneJob,
    RepositoryAccess, Retention, RetentionError, RewriteJob, RunningJob, SnapshotSummary,
    SourcesError, Start, StoreError,
};
use crate::secrets::{ResolveError, ResolveRepository};
use domain::execution::deadline::KeepDelete;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{FailureReason, Operation, OperationKind, Trigger, Warning, WarningKind};

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

/// How long `assess` waits for each of its reads (the repository's ID, its
/// snapshot list, each plan's retention) before it gives up. A repository
/// that never answers would otherwise hold the caller's request open for ever.
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
    /// This agent doesn't maintain the repository: its entry doesn't ask for
    /// `local` (I1, §7.3). Refused before the engine is asked anything.
    NotOwner,
    /// The configuration couldn't be read or used: the store's own report,
    /// which says where a mistake is and never holds a value of the file,
    /// so a route refuses it as a save of it is refused (#347).
    Unavailable(Box<ConfigError>),
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
    /// A run was asked to act on the snapshots a preview named, and what it
    /// would act on now is not that: the repository changed in between.
    /// Nothing was removed or rewritten; preview again.
    SelectionChanged,
    /// A plan's retention policy is not one the engine accepts, so what it
    /// would remove can't be told: the run is refused before anything is
    /// removed, and the plan is named.
    InvalidRetention {
        plan: PlanId,
    },
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

/// A maintenance run with every decision made and nothing held: no row in
/// the log and no engine job left running. [`RunMaintenance::start`] takes
/// its repository's I7 slot.
pub struct MaintenanceAssessment {
    operation: OperationId,
    /// The alias the run was asked under, which the log keeps with it.
    alias: RepositoryAlias,
    job: Job,
    partition: Option<Partition>,
    other_hosts: OtherHosts,
}

impl MaintenanceAssessment {
    /// The other hosts the repository holds snapshots from, when it is
    /// maintained here (§7.3): the run carries them as a warning. A check
    /// only reads, so it has none.
    pub fn other_hosts(&self) -> &OtherHosts {
        &self.other_hosts
    }
}

/// What a run would do, decided and not yet done (§7.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaintenancePreview {
    /// What each plan's policy lets go, and the history left alone.
    Forget(ForgetPreview),
    /// The snapshots an adoption would rewrite.
    Adopt(AdoptPreview),
    /// What a prune would do, from a dry run of the engine's own plan.
    Prune(PrunePreview),
    /// A check changes nothing.
    Check(CheckPreview),
}

impl MaintenancePreview {
    /// The selection a start must carry for the run to act on what this
    /// preview showed: a forget's and an adoption's; none for a prune or a
    /// check, which select nothing.
    pub fn selection(&self) -> Option<SelectionToken> {
        match self {
            Self::Forget(forget) => Some(forget.selection()),
            Self::Adopt(adopt) => Some(adopt.selection()),
            Self::Prune(_) | Self::Check(_) => None,
        }
    }
}

/// A preview, and the other hosts the repository holds snapshots from when
/// it is maintained here (§7.3), which the person reviewing the run is told
/// of. A check only reads, so its preview names none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Previewed {
    pub preview: MaintenancePreview,
    pub other_hosts: OtherHosts,
}

/// The snapshots an adoption would rewrite with the tags of `into`, newest
/// first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdoptPreview {
    pub into: PlanId,
    pub snapshots: Vec<Snapshot>,
}

impl AdoptPreview {
    /// The fingerprint of what the adoption rewrites and where it goes.
    pub fn selection(&self) -> SelectionToken {
        SelectionToken::of_adoption(
            &self.into,
            self.snapshots.iter().map(|snapshot| &snapshot.id),
        )
    }
}

/// What a prune would do, as the engine's own dry-run plan counts it. The
/// plan is computed again when the prune runs, so these are estimates: the
/// repository can change in between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrunePreview {
    /// Packs the prune would mark for deletion: unused ones and ones to
    /// repack.
    pub packs_marked: u64,
    /// Packs it would delete now: marked by an earlier prune and past
    /// `keep_delete`.
    pub packs_deleted: u64,
    /// Packs an earlier prune marked and this one would bring back, because
    /// something uses them again.
    pub packs_recovered: u64,
    /// The bytes the deletions would free.
    pub bytes_deleted: u64,
    /// How long a pack stays marked before a later prune deletes it (I2).
    pub keep_delete: KeepDelete,
}

/// What a check would do: it reads, and changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckPreview {
    /// Whether it reads every pack's data. A manual check reads the
    /// repository's structure only.
    pub read_data: bool,
}

/// One plan of this agent, and the snapshots it owns, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanHistory {
    pub plan: PlanId,
    pub snapshots: Vec<Snapshot>,
}

/// History carrying `ladderback-` tags no plan of this agent owns, and the
/// plans of the repository that could take it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanedHistory {
    pub group: OrphanedGroup,
    pub adopt_into: Vec<PlanId>,
}

/// Untagged history, and the plans whose host, label and sources equal its.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmanagedHistory {
    pub group: UnmanagedGroup,
    pub adopt_into: Vec<PlanId>,
}

/// A repository's snapshots by who owns them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepositoryGroups {
    /// Every plan of this agent on the repository, in the configuration's
    /// order, the ones with no snapshots too.
    pub plans: Vec<PlanHistory>,
    pub orphaned: Vec<OrphanedHistory>,
    pub unmanaged: Vec<UnmanagedHistory>,
    /// The hosts other than this one that the repository holds snapshots
    /// from, when it is maintained here (§7.3).
    pub other_hosts: OtherHosts,
}

/// Runs maintenance manually on the local owner.
pub struct RunMaintenance {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn OperationStore>,
    resolve: ResolveRepository,
    retention: Arc<dyn Retention>,
    config: Arc<dyn ConfigStore>,
    agent: AgentId,
    /// This machine's own name, which the engine records on a snapshot when
    /// neither its plan nor the agent names a host (`Plan::snapshot_host`).
    hostname: String,
    /// A plan's sources as the engine records them, so adoption's matching
    /// compares like with like.
    sources: Arc<dyn NormalizeSources>,
    /// Where the end of each run is announced, when anything listens.
    events: Option<Arc<dyn EventBus>>,
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
            events: None,
        }
    }

    /// Announces how each run ends on `events` (§4.4): once, after the last
    /// write to the log, whether the log took that write or not. A run of
    /// several engine jobs ends once, and so does a run that reads the
    /// repository first; a cancelled run is not announced.
    pub fn with_events(mut self, events: Arc<dyn EventBus>) -> Self {
        self.events = Some(events);
        self
    }

    /// Whether a manual check reads every pack's data. It doesn't: a check a
    /// person starts reads the repository's structure only.
    const CHECK_READS_DATA: bool = false;

    /// Decides what the run does, from reads alone: the configuration, the
    /// repository's ID and, for a forget, an adoption or a prune, its
    /// snapshots, which also tell the other hosts a run warns of (§7.3).
    /// Nothing is stored, no I7 slot is taken and no engine job is left
    /// running, so a caller that is dropped here, because the client that
    /// asked went away, has left nothing behind. Every refusal that doesn't
    /// need the slot (an unknown repository, one this agent doesn't maintain,
    /// a policy that keeps nothing, an adoption that matches no group, a
    /// listing that failed) is made here, before the log is touched, and each
    /// read gives up after [`READ_TIMEOUT`].
    pub async fn assess(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        kind: MaintenanceKind,
    ) -> Result<MaintenanceAssessment, MaintenanceError> {
        self.assess_with(op, alias, kind, None).await
    }

    /// [`assess`](Self::assess) for a run a person reviewed: refused with
    /// [`MaintenanceError::SelectionChanged`], before the slot is taken, when
    /// what the run would act on is not what `selection` names. Nothing is
    /// removed or rewritten then. A prune and a check select nothing, so a
    /// selection holds them to nothing.
    pub async fn assess_previewed(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        kind: MaintenanceKind,
        selection: &SelectionToken,
    ) -> Result<MaintenanceAssessment, MaintenanceError> {
        self.assess_with(op, alias, kind, Some(selection)).await
    }

    async fn assess_with(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        kind: MaintenanceKind,
        held: Option<&SelectionToken>,
    ) -> Result<MaintenanceAssessment, MaintenanceError> {
        // Only reads run before the slot is taken, and none before this
        // agent is found to maintain the repository.
        let config = self.maintained(alias).await?;
        let access = self.identified(op, &config, alias).await?;

        let (other_hosts, partition, job) = match kind {
            MaintenanceKind::Forget => {
                let (partition, forget) = self.forget_of(op, &config, alias, &access).await?;
                held_to(held, || forget.selection())?;
                (
                    // The forget's own listing tells the hosts.
                    self.other_hosts_in(&config, alias, &partition),
                    Some(partition),
                    Job::Forget(ForgetJob {
                        repository: access,
                        ids: forget.removal(),
                    }),
                )
            }
            MaintenanceKind::Adopt(adoption) => {
                let (partition, snapshots) = self
                    .adoption_of(op, &config, alias, &access, &adoption)
                    .await?;
                let adopt = AdoptPreview {
                    into: adoption.into.clone(),
                    snapshots,
                };
                held_to(held, || adopt.selection())?;
                let ids = adopt
                    .snapshots
                    .iter()
                    .map(|snapshot| snapshot.id.clone())
                    .collect();
                let tags = PlanTags::new(adoption.into, self.agent);
                (
                    self.other_hosts_in(&config, alias, &partition),
                    Some(partition),
                    Job::Rewrite(RewriteJob {
                        repository: access,
                        ids,
                        tags,
                    }),
                )
            }
            MaintenanceKind::Prune => (
                self.listed_other_hosts(op, &config, alias, &access).await?,
                None,
                Job::Prune(PruneJob {
                    repository: access,
                    keep_delete: default_keep_delete(),
                }),
            ),
            MaintenanceKind::Check => (
                // A check only reads, so it lists nothing and warns of nothing.
                OtherHosts::default(),
                None,
                Job::Check(CheckJob {
                    repository: access,
                    read_data: Self::CHECK_READS_DATA,
                }),
            ),
        };

        Ok(MaintenanceAssessment {
            operation: op,
            alias: alias.clone(),
            job,
            partition,
            other_hosts,
        })
    }

    /// Takes the assessed run's repository slot (I7): the operation is stored
    /// as running in the one step that decides it, or refused, naming the
    /// operation that holds the repository, with nothing stored. On success
    /// the run's own job has not been started: only `execute` starts it.
    ///
    /// A call dropped while it waits for the log may already have stored the
    /// operation as running, and the row then holds the repository until
    /// something ends it, which only the run does: a start dropped there
    /// leaves the repository held and nobody to run it. So a caller that can
    /// be dropped, as a route's handler is when its client hangs up, never
    /// awaits this on its own future. It assesses first, which is safe to drop,
    /// then starts and runs on a task of its own.
    pub async fn start(
        &self,
        assessment: MaintenanceAssessment,
    ) -> Result<PreparedMaintenance, MaintenanceError> {
        let MaintenanceAssessment {
            operation: id,
            alias,
            job,
            partition,
            other_hosts,
        } = assessment;
        let mut operation = Operation::queue(
            id,
            job.kind(),
            Trigger::Manual,
            job.target(),
            self.clock.now(),
        )
        .with_repository_alias(alias);
        #[allow(
            clippy::expect_used,
            reason = "the operation was queued a moment above and nothing else runs"
        )]
        operation
            .start(self.clock.now())
            .expect("a queued operation starts");
        if let Some(warning) =
            other_hosts_during(job.kind()).and_then(|during| other_hosts.warning(during))
        {
            // Stored with the running row, so the run says it from the start.
            let _ = operation.record_warning(warning);
        }
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

    /// What `kind` would do, decided from a read of the repository and told
    /// before anything runs: a read like a snapshot listing, so it takes no
    /// I7 slot and writes nothing to the operation log. A forget and an
    /// adoption return the snapshots they would act on and a selection a
    /// later [`assess_previewed`](Self::assess_previewed) can be held to.
    /// The decision is the one [`assess`](Self::assess) makes, so what a
    /// person reviews is what a run acts on.
    pub async fn preview(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
        kind: &MaintenanceKind,
    ) -> Result<Previewed, MaintenanceError> {
        let (preview, other_hosts) = match kind {
            MaintenanceKind::Check => {
                // A check needs nothing of the repository to say what it
                // does, only that this agent maintains it. It only reads, so
                // it lists nothing and warns of nothing.
                self.maintained(alias).await?;
                Ok((
                    MaintenancePreview::Check(CheckPreview {
                        read_data: Self::CHECK_READS_DATA,
                    }),
                    OtherHosts::default(),
                ))
            }
            MaintenanceKind::Forget => {
                let config = self.maintained(alias).await?;
                let access = self.open(&config, alias).await?;
                let (partition, forget) = self.forget_of(op, &config, alias, &access).await?;
                Ok((
                    MaintenancePreview::Forget(forget),
                    self.other_hosts_in(&config, alias, &partition),
                ))
            }
            MaintenanceKind::Adopt(adoption) => {
                let config = self.maintained(alias).await?;
                let access = self.open(&config, alias).await?;
                let (partition, snapshots) = self
                    .adoption_of(op, &config, alias, &access, adoption)
                    .await?;
                Ok((
                    MaintenancePreview::Adopt(AdoptPreview {
                        into: adoption.into.clone(),
                        snapshots,
                    }),
                    self.other_hosts_in(&config, alias, &partition),
                ))
            }
            MaintenanceKind::Prune => {
                // Opened as the prune opens it, so through the cache.
                let config = self.maintained(alias).await?;
                let access = self.identified(op, &config, alias).await?;
                let other_hosts = self.listed_other_hosts(op, &config, alias, &access).await?;
                let keep_delete = default_keep_delete();
                let job = Job::PrunePlan(PruneJob {
                    repository: access,
                    keep_delete,
                });
                let EngineResult::PrunePlan(plan) =
                    run_read(self.engine.as_ref(), self.clock.as_ref(), op, job).await?
                else {
                    return Err(MaintenanceError::Failed(FailureReason::EngineExited {
                        exit_code: None,
                    }));
                };
                Ok((
                    MaintenancePreview::Prune(PrunePreview {
                        packs_marked: plan.packs_marked,
                        packs_deleted: plan.packs_deleted,
                        packs_recovered: plan.packs_recovered,
                        bytes_deleted: plan.bytes_deleted,
                        keep_delete,
                    }),
                    other_hosts,
                ))
            }
        }?;
        Ok(Previewed {
            preview,
            other_hosts,
        })
    }

    /// The repository's snapshots grouped by who owns them (§7.3), as a
    /// read: every plan of this agent on the repository, the history no plan
    /// of this agent owns, and for each group of it the plans that could
    /// take it. A read, so it is answered whoever maintains the repository.
    pub async fn groups(
        &self,
        op: OperationId,
        alias: &RepositoryAlias,
    ) -> Result<RepositoryGroups, MaintenanceError> {
        let config = self.load().await?;
        let access = self.open(&config, alias).await?;
        let managed = plans_of(&config, alias);
        let partition = self.partitioned(op, access, &managed).await?;
        let plans = repository_plans(&config, alias);
        // Only an unmanaged group is matched by a plan's sources, so only
        // then does the engine's own spelling of them need asking. A plan
        // whose sources can't be read claims nothing.
        let normalized: Vec<Plan> = if partition.unmanaged.is_empty() {
            Vec::new()
        } else {
            self.normalized(&plans)
                .await
                .into_iter()
                .filter_map(|(_, result)| result.ok())
                .collect()
        };
        let other_hosts = self.other_hosts_in(&config, alias, &partition);
        // Which plan takes orphaned history is the person's choice, so every
        // plan of the repository is offered, to a group an adoption can name:
        // it names both tags, so a group missing one can't be adopted.
        let every_plan: Vec<PlanId> = plans.iter().map(|plan| plan.id.clone()).collect();
        let mut owned = partition.managed;
        let claimants = AdoptSelection::claimants(
            &partition.unmanaged,
            config.agent(),
            &self.hostname,
            &normalized,
        );
        Ok(RepositoryGroups {
            plans: managed
                .iter()
                .map(|plan| PlanHistory {
                    plan: plan.plan.clone(),
                    snapshots: owned
                        .iter()
                        .position(|group| group.plan == plan.plan)
                        .map(|at| owned.swap_remove(at).snapshots)
                        .unwrap_or_default(),
                })
                .collect(),
            orphaned: partition
                .orphaned
                .into_iter()
                .map(|group| OrphanedHistory {
                    adopt_into: if group.agent.is_some() && group.plan.is_some() {
                        every_plan.clone()
                    } else {
                        Vec::new()
                    },
                    group,
                })
                .collect(),
            unmanaged: partition
                .unmanaged
                .into_iter()
                .zip(claimants)
                .map(|(group, adopt_into)| UnmanagedHistory { group, adopt_into })
                .collect(),
            other_hosts,
        })
    }

    /// The configuration, as it is now.
    async fn load(&self) -> Result<AgentConfig, MaintenanceError> {
        self.config
            .load()
            .await
            .map_err(|error| MaintenanceError::Unavailable(Box::new(error)))
    }

    /// The configuration, once it shows that this agent maintains the
    /// repository `alias` (I1, §7.3): refused with
    /// [`MaintenanceError::NotOwner`] otherwise, before anything else is
    /// asked of the repository.
    async fn maintained(&self, alias: &RepositoryAlias) -> Result<AgentConfig, MaintenanceError> {
        let config = self.load().await?;
        let repository = config
            .repository(alias)
            .ok_or(MaintenanceError::UnknownRepository)?;
        if !Ownership::owned_here(repository.maintenance) {
            return Err(MaintenanceError::NotOwner);
        }
        Ok(config)
    }

    /// The access to the repository `alias` names in `config`. The access
    /// names the repository ID the configuration recorded for it, when it
    /// recorded one, so the engine opens that repository or none (§7.3),
    /// and the entry it was made from, so the ID it opens is recorded in an
    /// entry that records none (§6.1, decision 57).
    async fn open(
        &self,
        config: &AgentConfig,
        alias: &RepositoryAlias,
    ) -> Result<RepositoryAccess, MaintenanceError> {
        let repository = config
            .repository(alias)
            .ok_or(MaintenanceError::UnknownRepository)?;
        Ok(self
            .resolve
            .resolve_saved(alias, repository)
            .await
            .map_err(MaintenanceError::Resolve)?
            .access)
    }

    /// As [`open`](Self::open), with the access naming the repository's ID,
    /// as a run opens it: the ID names the repository in the log, which is
    /// what I7 holds it by, and the engine gives a job the repository's
    /// cache by it. A read reports it.
    async fn identified(
        &self,
        op: OperationId,
        config: &AgentConfig,
        alias: &RepositoryAlias,
    ) -> Result<RepositoryAccess, MaintenanceError> {
        let mut access = self.open(config, alias).await?;
        let id = identify(
            self.engine.as_ref(),
            self.clock.as_ref(),
            op,
            access.clone(),
        )
        .await?;
        access.id = Some(id);
        Ok(access)
    }

    /// The repository's snapshots partitioned by who owns them, from one
    /// listing. The listing and the copies the partition is made from are
    /// dropped before this returns, so the long reads that follow (a plan's
    /// retention) don't hold them.
    async fn partitioned(
        &self,
        op: OperationId,
        access: RepositoryAccess,
        plans: &[ManagedPlan],
    ) -> Result<Partition, MaintenanceError> {
        let snapshots: Vec<Snapshot> = list(self.engine.as_ref(), self.clock.as_ref(), op, access)
            .await?
            .iter()
            .map(selected)
            .collect();
        Ok(ForgetSelection::partition(&snapshots, &self.agent, plans))
    }

    /// What a forget would do to the repository, from one listing of it. A
    /// preview and a run are both made from this value, so what a person
    /// reviews is what a run removes (I6).
    async fn forget_of(
        &self,
        op: OperationId,
        config: &AgentConfig,
        alias: &RepositoryAlias,
        access: &RepositoryAccess,
    ) -> Result<(Partition, ForgetPreview), MaintenanceError> {
        let plans = plans_of(config, alias);
        let partition = self.partitioned(op, access.clone(), &plans).await?;
        let mut removals = Vec::new();
        for plan in &plans {
            let Some(group) = partition
                .managed
                .iter()
                .find(|group| group.plan == plan.plan)
            else {
                // A plan with no snapshots has nothing a policy could
                // remove, so its policy refuses nothing.
                continue;
            };
            // Checked per group, before anything is selected: a policy that
            // would keep nothing removes the plan's whole history, which is
            // the user's decision to make explicitly, not a rule's.
            if plan.policy.keeps_nothing() {
                return Err(MaintenanceError::KeepsNothing {
                    plan: plan.plan.clone(),
                });
            }
            let ids = within_read_timeout(
                self.clock.as_ref(),
                self.retention
                    .removes(access, &plan.policy, &group.snapshots)
                    .map(|answer| answer.map_err(|error| retention_failed(error, &plan.plan))),
            )
            .await?;
            removals.push((plan.plan.clone(), ids));
        }
        let forget = ForgetPreview::new(&partition, removals);
        Ok((partition, forget))
    }

    /// The snapshots an adoption would rewrite, newest first, from one
    /// listing of the repository.
    async fn adoption_of(
        &self,
        op: OperationId,
        config: &AgentConfig,
        alias: &RepositoryAlias,
        access: &RepositoryAccess,
        adoption: &Adoption,
    ) -> Result<(Partition, Vec<Snapshot>), MaintenanceError> {
        // Only a plan of this repository can take history: a plan of another
        // repository, or none at all, would leave the rewritten snapshots
        // orphaned again.
        let plans = repository_plans(config, alias);
        if !plans.iter().any(|plan| plan.id == adoption.into) {
            return Err(MaintenanceError::UnknownPlan {
                plan: adoption.into.clone(),
            });
        }
        let managed = plans_of(config, alias);
        let partition = self.partitioned(op, access.clone(), &managed).await?;
        let group = match &adoption.from {
            AdoptionSource::Orphaned { agent, plan } => {
                AdoptSelection::orphaned(&partition, agent, plan)
                    .ok_or(MaintenanceError::AdoptionNoMatch)?
                    .snapshots
                    .clone()
            }
            AdoptionSource::Unmanaged => {
                let mut normalized = Vec::new();
                for (id, result) in self.normalized(&plans).await {
                    match result {
                        Ok(plan) => normalized.push(plan),
                        // Only the destination has to be readable: a plan
                        // whose sources can't be read matches nothing.
                        Err(error) if id == adoption.into => return Err(sources_failed(error)),
                        Err(_) => {}
                    }
                }
                let into = normalized
                    .iter()
                    .find(|plan| plan.id == adoption.into)
                    .ok_or(MaintenanceError::UnknownPlan {
                        plan: adoption.into.clone(),
                    })?;
                AdoptSelection::unmanaged(
                    &partition,
                    config.agent(),
                    &self.hostname,
                    into,
                    &normalized,
                )
                .map_err(|refusal| match refusal {
                    AdoptionRefusal::NoMatch => MaintenanceError::AdoptionNoMatch,
                    AdoptionRefusal::Ambiguous { plans } => {
                        MaintenanceError::AdoptionAmbiguous { plans }
                    }
                })?
                .snapshots
                .clone()
            }
        };
        Ok((partition, group))
    }

    /// Each of `plans` with its sources as the engine records them, so
    /// matching an unmanaged group compares like with like: rustic resolves
    /// symlinks when it records a snapshot's paths. A plan whose sources
    /// can't be read (a drive that is not mounted, a folder renamed) fails
    /// alone, and the others are unaffected.
    async fn normalized(&self, plans: &[Plan]) -> Vec<(PlanId, Result<Plan, SourcesError>)> {
        futures::future::join_all(plans.iter().map(|plan| async {
            let normalized = self
                .sources
                .normalized(&plan.sources)
                .await
                .map(|sources| Plan {
                    sources,
                    ..plan.clone()
                });
            (plan.id.clone(), normalized)
        }))
        .await
    }

    /// The other hosts of the repository `alias` among the snapshots one
    /// listing of it was partitioned from (§7.3).
    fn other_hosts_in(
        &self,
        config: &AgentConfig,
        alias: &RepositoryAlias,
        partition: &Partition,
    ) -> OtherHosts {
        let snapshots = partition
            .managed
            .iter()
            .flat_map(|group| &group.snapshots)
            .chain(partition.orphaned.iter().flat_map(|group| &group.snapshots))
            .chain(
                partition
                    .unmanaged
                    .iter()
                    .flat_map(|group| &group.snapshots),
            );
        OtherHosts::of(
            config,
            alias,
            &self.agent,
            &self.hostname,
            self.clock.now(),
            snapshots,
        )
    }

    /// The other hosts of the repository `alias`, for a run that lists
    /// nothing of its own (a prune): a listing of it. Only a repository this
    /// agent maintains gets this far.
    async fn listed_other_hosts(
        &self,
        op: OperationId,
        config: &AgentConfig,
        alias: &RepositoryAlias,
        access: &RepositoryAccess,
    ) -> Result<OtherHosts, MaintenanceError> {
        let snapshots: Vec<Snapshot> = list(
            self.engine.as_ref(),
            self.clock.as_ref(),
            op,
            access.clone(),
        )
        .await?
        .iter()
        .map(selected)
        .collect();
        Ok(OtherHosts::of(
            config,
            alias,
            &self.agent,
            &self.hostname,
            self.clock.now(),
            &snapshots,
        ))
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
        // The log has had the operation running since `start` stored it, so
        // the news can go out now, once however many jobs the run takes (#305).
        announce_start(self.events.as_deref(), &operation, None).await;
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
                    note_partial(&mut operation, done.as_ref(), total);
                    let _ = operation.fail(FailureReason::from(error), self.clock.now());
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
        announce_end(self.events.as_deref(), &operation).await;
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
        Job::Forget(ForgetJob { repository, ids }) => parts(ids)
            .into_iter()
            .map(|ids| {
                Job::Forget(ForgetJob {
                    repository: repository.clone(),
                    ids,
                })
            })
            .collect(),
        Job::Rewrite(RewriteJob {
            repository,
            ids,
            tags,
        }) => parts(ids)
            .into_iter()
            .map(|ids| {
                Job::Rewrite(RewriteJob {
                    repository: repository.clone(),
                    ids,
                    tags: tags.clone(),
                })
            })
            .collect(),
        job => vec![job],
    }
}

/// `ids` in consecutive parts of at most [`MAX_JOB_IDS`], each ID moved once;
/// no ids make one empty part, so a job is never lost.
fn parts(ids: Vec<SnapshotId>) -> Vec<Vec<SnapshotId>> {
    if ids.is_empty() {
        return vec![ids];
    }
    let mut rest = ids.into_iter();
    std::iter::from_fn(|| {
        let part: Vec<SnapshotId> = rest.by_ref().take(MAX_JOB_IDS).collect();
        (!part.is_empty()).then_some(part)
    })
    .collect()
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

/// The run an other-hosts warning names, as `WarningKind::OtherHosts` says:
/// a forget, an adoption or a prune. A check only reads, so it warns of none.
fn other_hosts_during(kind: OperationKind) -> Option<&'static str> {
    match kind {
        OperationKind::Forget => Some("forget"),
        OperationKind::Adopt => Some("adopt"),
        OperationKind::Prune => Some("prune"),
        _ => None,
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

/// The plans of `config` that back up the repository `alias`.
fn repository_plans(config: &AgentConfig, alias: &RepositoryAlias) -> Vec<Plan> {
    config
        .plans()
        .iter()
        .filter(|plan| plan.repository == *alias)
        .cloned()
        .collect()
}

/// Refuses a run that is held to a selection other than the one it would
/// now act on: the repository changed after the person reviewed it. `now`
/// is only worked out when the run is held to one: it reads every snapshot
/// of the selection.
fn held_to(
    held: Option<&SelectionToken>,
    now: impl FnOnce() -> SelectionToken,
) -> Result<(), MaintenanceError> {
    match held {
        Some(held) if *held != now() => Err(MaintenanceError::SelectionChanged),
        _ => Ok(()),
    }
}

/// One snapshot of a listing, as the selection reads it.
fn selected(summary: &SnapshotSummary) -> Snapshot {
    Snapshot {
        id: summary.id.clone(),
        time: summary.time,
        offset: summary.offset,
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
    within_read_timeout(clock, async {
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
    })
    .await
}

/// Awaits a read of the repository, giving it up when [`READ_TIMEOUT`] has
/// passed on `clock` (dropping the read cancels what can be cancelled).
async fn within_read_timeout<T>(
    clock: &dyn Clock,
    read: impl Future<Output = Result<T, MaintenanceError>>,
) -> Result<T, MaintenanceError> {
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
        error @ (EngineError::StartTimedOut | EngineError::SecretsTooLargeToMask { .. }) => {
            MaintenanceError::Failed(FailureReason::from(error))
        }
    }
}

fn sources_failed(error: SourcesError) -> MaintenanceError {
    let SourcesError::Unreadable { detail } = error;
    MaintenanceError::Engine { detail }
}

fn retention_failed(error: RetentionError, plan: &PlanId) -> MaintenanceError {
    match error {
        RetentionError::Engine { detail } => MaintenanceError::Engine { detail },
        RetentionError::InvalidPolicy => MaintenanceError::InvalidRetention { plan: plan.clone() },
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
            entry: None,
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

#[cfg(test)]
#[cfg(feature = "fakes")]
mod read_timeout_tests {
    #![allow(clippy::expect_used, reason = "test code")]

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use domain::planning::{Maintenance, RepositoryConfig, RepositorySettings};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{OperationId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp, Uuid};
    use futures::StreamExt;
    use futures::stream;

    use super::{MaintenanceError, READ_TIMEOUT, list, run_read};
    use crate::fakes::{FakeClock, InMemoryConfigStore};
    use crate::ports::{
        Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, RepositoryAccess,
        RunningJob, SavedEntry,
    };
    use crate::repositories::{ConfigWrites, RecordOpened};

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
            entry: None,
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
            let id = RepositoryId::parse(&"a".repeat(64)).expect("a repository ID");
            Ok(RunningJob {
                events: stream::iter([
                    EngineEvent::Started {
                        index_read_at: Timestamp::UNIX_EPOCH,
                        repository: Some(id.clone()),
                    },
                    EngineEvent::Succeeded(EngineResult::Repository { id }),
                ])
                .boxed(),
                control: Box::new(Nothing),
            })
        }
    }

    /// A read of a repository whose entry records no ID runs after an
    /// identify (decision 81); an engine that refuses it then refuses the
    /// read as it would have refused it alone.
    #[tokio::test]
    async fn a_read_the_engine_refuses_after_its_identify_is_refused_as_the_engine_refused_it() {
        let engine = RecordOpened::new(
            Arc::new(RefusesAfterIdentify),
            Arc::new(InMemoryConfigStore::new()),
            Arc::new(ConfigWrites::new(())),
            Arc::new(|_| {}),
        );
        let mut unrecorded = access();
        unrecorded.entry = Some(SavedEntry {
            alias: RepositoryAlias::parse("demo").expect("an alias"),
            entry: RepositoryConfig {
                settings: RepositorySettings {
                    repository: "/srv/demo/repo".to_owned(),
                    repo_hot: None,
                    password: SecretRef::Inline(SecretValue::new("demo-password")),
                    options: BTreeMap::new(),
                    options_hot: BTreeMap::new(),
                    options_cold: BTreeMap::new(),
                },
                id: None,
                maintenance: Maintenance::None,
                group_by: Default::default(),
                hooks: Default::default(),
            },
        });
        let clock = FakeClock::new(Timestamp::UNIX_EPOCH);
        let refused = list(
            &engine,
            &clock,
            OperationId::from_uuid(Uuid::from_u128(1)),
            unrecorded,
        )
        .await;
        assert_eq!(
            refused,
            Err(MaintenanceError::Engine {
                detail: "no such file or directory".to_owned(),
            })
        );
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
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
        async fn recover(
            &self,
            _reason: &domain::execution::FailureReason,
        ) -> Result<crate::ports::Recovered, StoreError> {
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

        async fn ever_succeeded(&self, _plan: &domain::PlanId) -> Result<bool, StoreError> {
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
            Stalled.recover(&domain::execution::FailureReason::DaemonStopped),
        )
        .await;
        assert!(answered.is_err(), "a stalled log answers nothing");
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod config_tests {
    #![allow(clippy::expect_used, reason = "test code")]

    use std::sync::Arc;

    use async_trait::async_trait;
    use domain::planning::RepositoryConfig;
    use domain::{AgentId, OperationId, RepositoryId, Timestamp, Uuid};

    use super::*;
    use crate::fakes::{
        FakeClock, FakeRetention, FakeSecrets, FakeSources, InMemoryOperationStore,
    };
    use crate::ports::RecordError;

    /// A configuration file with a mistake in it, as the store reports it.
    fn mistake() -> ConfigError {
        ConfigError::SchemaMissing {
            file: "/c/config.toml".to_owned(),
        }
    }

    /// A configuration store whose file has a mistake in it.
    struct Mistaken;

    #[async_trait]
    impl ConfigStore for Mistaken {
        async fn load(&self) -> Result<AgentConfig, ConfigError> {
            Err(mistake())
        }

        async fn save(&self, _config: &AgentConfig) -> Result<(), ConfigError> {
            Err(mistake())
        }

        async fn record_id(
            &self,
            _alias: &RepositoryAlias,
            _read: &RepositoryConfig,
            _id: &RepositoryId,
        ) -> Result<(), RecordError> {
            Err(RecordError::Store(mistake()))
        }
    }

    fn use_case() -> RunMaintenance {
        RunMaintenance::new(
            Arc::new(crate::fakes::SimulatedEngine::new(Arc::new(
                FakeClock::auto_advancing(Timestamp::UNIX_EPOCH),
            ))),
            Arc::new(FakeClock::auto_advancing(Timestamp::UNIX_EPOCH)),
            Arc::new(InMemoryOperationStore::new()),
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            Arc::new(FakeRetention),
            Arc::new(Mistaken),
            AgentId::from_uuid(Uuid::from_u128(1)),
            "demo-host".to_owned(),
            Arc::new(FakeSources),
        )
    }

    /// A read, a preview and a run all refuse a configuration they can't
    /// use with the store's own report of it, so the route can say where
    /// the mistake is (#347).
    #[tokio::test]
    async fn a_configuration_that_cannot_be_used_is_refused_with_the_stores_report() {
        let (use_case, op) = (use_case(), OperationId::from_uuid(Uuid::from_u128(1)));
        let alias = RepositoryAlias::parse("demo").expect("an alias");
        let expected = Some(MaintenanceError::Unavailable(Box::new(mistake())));
        assert_eq!(use_case.groups(op, &alias).await.err(), expected);
        assert_eq!(
            use_case
                .preview(op, &alias, &MaintenanceKind::Check)
                .await
                .err(),
            expected
        );
        assert_eq!(
            use_case
                .assess(op, &alias, MaintenanceKind::Check)
                .await
                .err(),
            expected
        );
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
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
                    entry: None,
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
