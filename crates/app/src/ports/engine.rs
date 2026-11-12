use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use domain::execution::deadline::KeepDelete;
use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::execution::{FailureReason, OperationKind, Target, Warning};
use domain::planning::{Hostname, RepositoryConfig, SourceFilter, masked_options};
use domain::secrets::{REDACTED, Redactor, RepositorySecrets, SecretValue};
use domain::{
    Offset, OperationId, RepositoryAlias, RepositoryId, SignedDuration, SnapshotId, Timestamp,
};
use futures::stream::BoxStream;

/// Runs operations against a repository and reports what happens as a stream
/// of [`EngineEvent`]s.
///
/// Contract (checked by `contract::engine`):
/// - An accepted job's stream starts with exactly one [`EngineEvent::Started`]
///   and ends with exactly one [`EngineEvent::Succeeded`] or
///   [`EngineEvent::Failed`], after which it yields nothing. An engine that
///   fails before it starts yields only the `Failed` event.
/// - A job with a deadline ([`Job::deadline_budget`]) that is still running
///   at it starts no snapshot write and ends with
///   `Failed(FailureReason::DeadlinePassed)`. A backup that was archiving may
///   still be saving the index of what it wrote after that event, or
///   finishing a snapshot write it began before the deadline, until it ends
///   or [`JobControl::stop`] ends it.
/// - [`JobControl::stop`] stops the job: once it returns, the stream never
///   yields `Succeeded`, and it ends.
/// - Dropping the [`RunningJob`] stops the job at once, without a grace period.
#[async_trait]
pub trait Engine: Send + Sync {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError>;
}

/// An accepted job: what it reports, and how to stop it.
pub struct RunningJob {
    pub events: BoxStream<'static, EngineEvent>,
    pub control: Box<dyn JobControl>,
}

/// Stops a running job.
#[async_trait]
pub trait JobControl: Send {
    /// Asks the job to stop, waits up to the engine's grace period, then
    /// forces it (for a worker process: SIGTERM, then SIGKILL). Returns once
    /// the job has stopped. Calling it after the job ended does nothing.
    async fn stop(&mut self);
}

/// The engine couldn't start the operation at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The engine couldn't be started, or refused the job. `detail` is the
    /// platform's own message.
    Unavailable { detail: String },
    /// The engine started but didn't take its job in time: writing it to the
    /// worker stayed blocked, as it does for a spec larger than the pipe's
    /// buffer when the worker doesn't read. A worker that took its job and
    /// then sent nothing is not this error: `run` doesn't wait for it, and
    /// its events fail with `FailureReason::EngineStartTimedOut` instead.
    StartTimedOut,
    /// The engine refused the job before starting it: the job's secrets,
    /// with whatever else the engine masks in what it shows (the proxy
    /// credentials of its environment), come to more than `limit` bytes,
    /// more than it masks (I4, §10.2). `field` names where they passed it,
    /// such as `HTTPS_PROXY`, and is `None` when the job's own secrets did.
    SecretsTooLargeToMask { field: Option<String>, limit: usize },
}

/// What an operation the engine never started failed with.
impl From<EngineError> for FailureReason {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::Unavailable { detail } => Self::EngineUnavailable { detail },
            EngineError::StartTimedOut => Self::EngineStartTimedOut,
            EngineError::SecretsTooLargeToMask { field, limit } => {
                Self::SecretsTooLargeToMask { field, limit }
            }
        }
    }
}

/// What an operation should do.
#[allow(
    clippy::large_enum_variant,
    reason = "one job per operation, built once and moved to the engine: boxing the backup would only add an allocation"
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// Touches no repository. Exercises the engine boundary (worker process,
    /// event protocol, crash handling) in tests and diagnostics.
    Fake(FakeJob),
    /// Opens a repository and reports its ID.
    Identify(RepositoryAccess),
    /// Initialises a repository at the location, with default key options,
    /// and reports its ID. It refuses when a repository already exists
    /// there, and never overwrites one.
    Init(RepositoryAccess),
    Backup(BackupJob),
    Forget(ForgetJob),
    /// Rewrites the named snapshots with the tags given, as new snapshot
    /// files over the same trees, and removes the originals (§7.3).
    Rewrite(RewriteJob),
    Prune(PruneJob),
    /// What a [`Prune`](Self::Prune) of the same repository would do, worked
    /// out and not done: a read, so it takes no I7 slot and is never written
    /// to the operation log (§9). It is what a person reviews before a prune
    /// starts.
    PrunePlan(PruneJob),
    Check(CheckJob),
    /// Lists the repository's snapshots, newest first. A read, not an
    /// operation: it takes no I7 slot and is never written to the operation
    /// log (§9).
    Ls(RepositoryAccess),
}

impl Job {
    /// How long the job may run after it starts reading the index; `None`
    /// when it has no deadline. See `domain::execution::deadline`.
    pub fn deadline_budget(&self) -> Option<SignedDuration> {
        match self {
            Self::Fake(job) => job.deadline_budget,
            Self::Backup(job) => Some(job.deadline_budget),
            Self::Forget(_)
            | Self::Rewrite(_)
            | Self::Identify(_)
            | Self::Init(_)
            | Self::Prune(_)
            | Self::PrunePlan(_)
            | Self::Check(_)
            | Self::Ls(_) => None,
        }
    }

    /// The kind of operation this job makes.
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::Fake(_) => OperationKind::Fake,
            Self::Identify(_) => OperationKind::Identify,
            Self::Init(_) => OperationKind::Init,
            Self::Backup(_) => OperationKind::Backup,
            Self::Forget(_) => OperationKind::Forget,
            Self::Rewrite(_) => OperationKind::Adopt,
            // A plan is never recorded; it names the kind it plans.
            Self::Prune(_) | Self::PrunePlan(_) => OperationKind::Prune,
            Self::Check(_) => OperationKind::Check,
            Self::Ls(_) => OperationKind::Ls,
        }
    }

    /// The repository the job opens: every job's but a fake one's.
    pub fn access(&self) -> Option<&RepositoryAccess> {
        match self {
            Self::Fake(_) => None,
            Self::Identify(access) | Self::Init(access) | Self::Ls(access) => Some(access),
            Self::Backup(job) => Some(&job.repository),
            Self::Forget(job) => Some(&job.repository),
            Self::Rewrite(job) => Some(&job.repository),
            Self::Prune(job) | Self::PrunePlan(job) => Some(&job.repository),
            Self::Check(job) => Some(&job.repository),
        }
    }

    /// What the job works on: a backup's plan, and the repository of any job
    /// that opens one whose ID is already known.
    pub fn target(&self) -> Target {
        let plan = match self {
            Self::Backup(job) => Some(job.tags.plan().clone()),
            Self::Rewrite(job) => Some(job.tags.plan().clone()),
            _ => None,
        };
        Target {
            plan,
            repository: self.access().and_then(|access| access.id.clone()),
        }
    }
}

/// How to reach and open one repository.
#[derive(Clone, PartialEq, Eq)]
pub struct RepositoryAccess {
    /// rustic's `repository` string: a path, `rest:…`, `opendal:…`, `rclone:…`.
    /// Next to a `repo_hot`, it is the cold half.
    pub repository: String,
    /// rustic's `repo-hot`: the hot half of a hot/cold repository. `None` for
    /// a repository that is all in `repository`.
    pub repo_hot: Option<String>,
    pub password: SecretValue,
    /// Backend options, as rustic's `[repository.options]`: both halves get
    /// them.
    pub options: BTreeMap<String, String>,
    /// rustic's `options-hot`: the options of the hot half only. They apply
    /// to nothing without a `repo_hot`.
    pub options_hot: BTreeMap<String, String>,
    /// rustic's `options-cold`: the options of the cold half only.
    pub options_cold: BTreeMap<String, String>,
    /// What the `${…}` references in the repository's settings resolved to.
    /// They may sit anywhere in `repository`, `repo_hot` and the three sets
    /// of options, whatever the option's name, so the engine hides every one
    /// of them wherever it reports text (I4, technical design §10.2). The
    /// password isn't listed: it has a field of its own.
    pub secrets: Vec<SecretValue>,
    /// The repository's ID when already known; the engine uses it to give
    /// each worker its own cache dir. `None` before the first `Identify`.
    pub id: Option<RepositoryId>,
    /// The configuration entry the job was made from, when it was made from
    /// one, as the job read it. The engine doesn't use it: the daemon
    /// records in it the ID of the repository the job opened, when the entry
    /// records none (§6.1, decision 57). `None` for a repository a request
    /// gave, which no entry holds yet.
    pub entry: Option<SavedEntry>,
}

/// A repository's entry in the configuration, under its alias, as a job read
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedEntry {
    pub alias: RepositoryAlias,
    pub entry: RepositoryConfig,
}

impl RepositoryAccess {
    /// Everything of this repository that is a secret or can hold one, in the
    /// one form a [`Redactor`] is built from. A caller with more to hide than
    /// the repository holds, such as the proxy credentials of the daemon's
    /// environment, replaces `resolved` with the longer list.
    pub fn known_secrets(&self) -> RepositorySecrets<'_> {
        RepositorySecrets {
            repository: &self.repository,
            repo_hot: self.repo_hot.as_deref(),
            password: &self.password,
            options: &self.options,
            options_hot: &self.options_hot,
            options_cold: &self.options_cold,
            resolved: &self.secrets,
        }
    }

    /// What hides this repository's secrets wherever text about it is shown.
    pub fn redactor(&self) -> Redactor {
        Redactor::for_repository(self.known_secrets())
    }
}

/// Everything that can hold a secret is masked (I4): the password, the
/// password in the URL of either half, the passwords in URLs that options
/// hold, every option's value, and what the references resolved to. The rest
/// of the URLs and the options' names stay, for diagnosing.
impl std::fmt::Debug for RepositoryAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redactor = self.redactor();
        let repository = redactor.redact(&self.repository);
        let repo_hot = self.repo_hot.as_deref().map(|hot| redactor.redact(hot));
        let secrets: Vec<&str> = self.secrets.iter().map(|_| REDACTED).collect();
        f.debug_struct("RepositoryAccess")
            .field("repository", &repository)
            .field("repo_hot", &repo_hot)
            .field("password", &self.password)
            .field("options", &masked_options(&self.options))
            .field("options_hot", &masked_options(&self.options_hot))
            .field("options_cold", &masked_options(&self.options_cold))
            .field("secrets", &secrets)
            .field("id", &self.id)
            .field("entry", &self.entry.as_ref().map(|saved| &saved.alias))
            .finish()
    }
}

/// Back up `sources` as a snapshot of the plan named by `tags`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupJob {
    pub repository: RepositoryAccess,
    pub sources: Vec<String>,
    /// Added to the snapshot, and used to choose its parent
    /// ([`PlanTags::parent`]).
    pub tags: PlanTags,
    /// The user's own tags, added next to the plan tags. None uses the
    /// reserved `ladderback-` prefix.
    pub extra_tags: Vec<UserTag>,
    pub label: Option<String>,
    /// The snapshot host the snapshot records ([`Plan::snapshot_host`]).
    /// `None` leaves the name to the engine: the machine's own.
    ///
    /// [`Plan::snapshot_host`]: domain::planning::Plan::snapshot_host
    pub host: Option<Hostname>,
    /// What the backup leaves out of `sources`: the plan's source filter.
    pub filter: SourceFilter,
    /// See [`Job::deadline_budget`].
    pub deadline_budget: SignedDuration,
}

/// Prune unreferenced data. Packs are only marked, and deleted by a later
/// prune once marked for at least `keep_delete`: never instantly (I2), and
/// never sooner than rustic's default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneJob {
    pub repository: RepositoryAccess,
    pub keep_delete: KeepDelete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckJob {
    pub repository: RepositoryAccess,
    /// Also read and verify every pack's contents.
    pub read_data: bool,
}

/// The most snapshot IDs one `Forget` or `Rewrite` job may name. A job's
/// spec travels to the worker as one message under a cap of 1 MiB, and an ID
/// takes about 70 bytes of it, so a run that must name more is split into
/// several jobs of at most this many. An engine refuses a job that names
/// more.
pub const MAX_JOB_IDS: usize = 5_000;

/// Remove exactly `ids` from the repository: the snapshots a forget run's
/// selection named (I6 — the selection is the owner's own decision, made
/// before the job runs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgetJob {
    pub repository: RepositoryAccess,
    /// The snapshots to remove, each of them one the selection named.
    pub ids: Vec<SnapshotId>,
}

/// What an adoption rewrites: the group's snapshots, each becoming a new
/// snapshot file with the plan's tags over the same tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteJob {
    pub repository: RepositoryAccess,
    /// The snapshots to rewrite, each of them one the selection named.
    pub ids: Vec<SnapshotId>,
    /// `ladderback-plan:<id>` and `ladderback-agent:<id>`: what the rewritten
    /// snapshots carry instead of the `ladderback-*` tags they had.
    pub tags: PlanTags,
}

/// A scripted job: count `steps`, waiting `step_delay` before each, report
/// `warnings` right after starting, then end as `end` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeJob {
    pub steps: u32,
    pub step_delay: Duration,
    pub warnings: Vec<Warning>,
    pub end: FakeEnd,
    /// Gives the fake job a deadline, counted from its start, so deadline
    /// handling can be tested without a repository.
    pub deadline_budget: Option<SignedDuration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeEnd {
    /// Report a result after the last step.
    Succeed,
    /// Report a failure with this detail after the last step.
    Fail { detail: String },
    /// Stop without reporting anything once `after_steps` steps are done,
    /// as a crashed engine would. The worker exits with `exit_code`; a
    /// process exit status is one byte, so the type keeps fakes honest.
    Exit { after_steps: u32, exit_code: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    /// The job is running. `index_read_at` is taken just before it starts
    /// reading the repository index (a fake job: when it starts); a deadline
    /// counts from it.
    Started {
        index_read_at: Timestamp,
        /// The ID of the repository the job opened. `None` for a job that
        /// opens none: a fake job, and an initialisation, which reports the
        /// new repository's ID in its result.
        repository: Option<RepositoryId>,
    },
    Progress(ProgressEvent),
    Warning(Warning),
    Succeeded(EngineResult),
    Failed(FailureReason),
}

/// One progress bar's change; `key` tells concurrent bars apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressEvent {
    Begin {
        key: u32,
        kind: ProgressKind,
        prefix: String,
    },
    Title {
        key: u32,
        title: String,
    },
    Length {
        key: u32,
        len: u64,
    },
    Advance {
        key: u32,
        by: u64,
    },
    Finish {
        key: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    Spinner,
    Counter,
    Bytes,
}

/// What a successful operation produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineResult {
    Fake {
        steps_completed: u32,
    },
    Repository {
        id: RepositoryId,
    },
    Backup(BackupSummary),
    Prune(PruneSummary),
    /// What a [`Job::PrunePlan`] found.
    PrunePlan(PrunePlanSummary),
    Check(CheckSummary),
    Forget(ForgetSummary),
    /// What an adoption produced: the new snapshots, in the order of the
    /// originals the job was given.
    Rewrite(RewriteSummary),
    /// A repository's snapshots, newest first.
    Snapshots(Vec<SnapshotSummary>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSummary {
    pub repository: RepositoryId,
    pub snapshot: SnapshotId,
    pub parent: Option<SnapshotId>,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    /// Bytes of the source the backup went through, changed or not. `None`
    /// in a result an earlier build logged, which didn't keep it.
    pub bytes_processed: Option<u64>,
    /// Bytes of new data before and after compression and packing.
    pub data_added: u64,
    pub data_added_packed: u64,
    /// Items that couldn't be backed up; each was also reported as a warning.
    pub error_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneSummary {
    pub repository: RepositoryId,
    /// Packs newly marked for deletion by a later prune.
    pub packs_marked: u64,
    /// Packs marked by an earlier prune, now deleted.
    pub packs_deleted: u64,
    /// Marked packs that a newer snapshot uses again, now unmarked.
    pub packs_recovered: u64,
}

/// What a prune would do, from the engine's own plan: the figures of a run
/// that was not made. They are estimates, since the plan is worked out again
/// when a prune runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrunePlanSummary {
    pub repository: RepositoryId,
    /// Packs a prune would newly mark for deletion: unused ones and ones to
    /// repack.
    pub packs_marked: u64,
    /// Packs marked by an earlier prune that this one would delete.
    pub packs_deleted: u64,
    /// Marked packs a newer snapshot uses again, which this one would unmark.
    pub packs_recovered: u64,
    /// The bytes of the packs this prune would delete.
    pub bytes_deleted: u64,
}

/// A check that ran to the end succeeds even when it found problems; they
/// are counted here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckSummary {
    pub repository: RepositoryId,
    pub errors: u64,
    pub warnings: u64,
}

/// What an adoption's rewrite left in the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteSummary {
    pub repository: RepositoryId,
    /// The copy of each snapshot the job named, in the order it named them:
    /// new, or one an earlier rewrite cut short had already saved.
    pub created: Vec<SnapshotId>,
}

/// What a forget run removed. What survived and why is the selection's own
/// story, told before the run; this is what the run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgetSummary {
    pub repository: RepositoryId,
    /// The snapshots removed, in the order the job named them.
    pub removed: Vec<SnapshotId>,
}

/// One snapshot of a repository's list, as the engine reports it. The wire
/// form the UI sees is `api`'s snapshot DTO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub id: SnapshotId,
    /// When the snapshot was taken.
    pub time: Timestamp,
    /// The offset of the clock that took it, as the snapshot file records
    /// it: with `time`, the snapshot time (§3.1).
    pub offset: Offset,
    pub hostname: String,
    pub label: String,
    pub tags: Vec<String>,
    /// The snapshot's paths, capped (an input-derived size).
    pub paths: Vec<String>,
    /// The snapshot has more paths than `paths` holds, or one of them was
    /// cut short.
    pub paths_truncated: bool,
    /// The snapshot has more tags than `tags` holds, or one of them was cut
    /// short.
    pub tags_truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{AgentId, PlanId, Uuid};

    fn access(id: Option<RepositoryId>) -> RepositoryAccess {
        RepositoryAccess {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id,
            entry: None,
        }
    }

    #[test]
    fn an_engine_that_never_started_the_job_fails_it_with_the_reason_of_its_refusal() {
        for (error, reason) in [
            (
                EngineError::Unavailable {
                    detail: "no such program".to_owned(),
                },
                FailureReason::EngineUnavailable {
                    detail: "no such program".to_owned(),
                },
            ),
            (
                EngineError::StartTimedOut,
                FailureReason::EngineStartTimedOut,
            ),
            (
                EngineError::SecretsTooLargeToMask {
                    field: Some("HTTPS_PROXY".to_owned()),
                    limit: 262_144,
                },
                FailureReason::SecretsTooLargeToMask {
                    field: Some("HTTPS_PROXY".to_owned()),
                    limit: 262_144,
                },
            ),
            (
                EngineError::SecretsTooLargeToMask {
                    field: None,
                    limit: 262_144,
                },
                FailureReason::SecretsTooLargeToMask {
                    field: None,
                    limit: 262_144,
                },
            ),
        ] {
            assert_eq!(FailureReason::from(error), reason);
        }
    }

    #[test]
    fn a_job_knows_its_kind_and_what_it_targets() {
        let repository = RepositoryId::parse(&"ab".repeat(32)).unwrap();
        let plan = PlanId::parse("demo-home").unwrap();
        let on_repository = Target {
            plan: None,
            repository: Some(repository.clone()),
        };
        let access = access(Some(repository.clone()));

        let fake = Job::Fake(FakeJob {
            steps: 0,
            step_delay: Duration::ZERO,
            warnings: Vec::new(),
            end: FakeEnd::Succeed,
            deadline_budget: None,
        });
        assert_eq!(fake.kind(), OperationKind::Fake);
        assert_eq!(fake.target(), Target::default());

        let identify = Job::Identify(access.clone());
        assert_eq!(identify.kind(), OperationKind::Identify);
        assert_eq!(identify.target(), on_repository);

        let backup = Job::Backup(BackupJob {
            repository: access.clone(),
            sources: vec!["/home/demo".to_owned()],
            tags: PlanTags::new(plan.clone(), AgentId::from_uuid(Uuid::from_u128(1))),
            extra_tags: Vec::new(),
            label: None,
            host: None,
            filter: SourceFilter::default(),
            deadline_budget: SignedDuration::from_hours(6),
        });
        assert_eq!(backup.kind(), OperationKind::Backup);
        let on_plan = Target {
            plan: Some(plan.clone()),
            repository: Some(repository),
        };
        assert_eq!(backup.target(), on_plan);

        let rewrite = Job::Rewrite(RewriteJob {
            repository: access.clone(),
            ids: Vec::new(),
            tags: PlanTags::new(plan, AgentId::from_uuid(Uuid::from_u128(1))),
        });
        assert_eq!(rewrite.kind(), OperationKind::Adopt);
        assert_eq!(rewrite.target(), on_plan);

        let prune = Job::Prune(PruneJob {
            repository: access.clone(),
            keep_delete: KeepDelete::MIN,
        });
        assert_eq!(prune.kind(), OperationKind::Prune);
        assert_eq!(prune.target(), on_repository);

        let plan = Job::PrunePlan(PruneJob {
            repository: access.clone(),
            keep_delete: KeepDelete::MIN,
        });
        assert_eq!(plan.kind(), OperationKind::Prune);
        assert_eq!(plan.target(), on_repository);
        assert_eq!(plan.deadline_budget(), None);

        let check = Job::Check(CheckJob {
            repository: access,
            read_data: false,
        });
        assert_eq!(check.kind(), OperationKind::Check);
        assert_eq!(check.target(), on_repository);
    }

    #[test]
    fn a_repository_that_is_not_identified_yet_is_not_a_target() {
        let identify = Job::Identify(access(None));
        assert_eq!(identify.target(), Target::default());
    }

    fn access_at(repository: &str) -> RepositoryAccess {
        RepositoryAccess {
            repository: repository.to_owned(),
            repo_hot: None,
            password: SecretValue::new("hunter2"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: None,
            entry: None,
        }
    }

    #[test]
    fn a_job_never_shows_its_secrets_in_its_debug_form() {
        let access = RepositoryAccess {
            options: BTreeMap::from([(
                "secret_access_key".to_owned(),
                "AKIAEXAMPLEKEY".to_owned(),
            )]),
            ..access_at("rest:https://demo:s3cr3t@nas:8000/backups")
        };
        let debug = format!("{:?}", Job::Identify(access));
        for secret in ["hunter2", "s3cr3t", "AKIAEXAMPLEKEY"] {
            assert!(!debug.contains(secret), "{secret} leaked: {debug}");
        }
        for kept in ["https://demo:", "@nas:8000/backups", "secret_access_key"] {
            assert!(debug.contains(kept), "{kept} is gone: {debug}");
        }
    }

    #[test]
    fn a_job_hides_what_its_references_resolved_to_wherever_that_shows() {
        let access = RepositoryAccess {
            secrets: vec![
                SecretValue::new("canary-from-a-file"),
                SecretValue::new("canary-from-env"),
            ],
            ..access_at("rest:https://canary-from-env:8000/canary-from-a-file/backups")
        };
        let debug = format!("{:?}", Job::Identify(access));
        for secret in ["canary-from-a-file", "canary-from-env"] {
            assert!(!debug.contains(secret), "{secret} leaked: {debug}");
        }
        assert!(
            debug.contains("rest:https://***:8000/***/backups"),
            "{debug}"
        );
    }

    #[test]
    fn a_job_never_shows_the_secrets_of_its_hot_and_cold_halves_in_its_debug_form() {
        let access = RepositoryAccess {
            repo_hot: Some("rest:https://demo:hot-s3cr3t@hot.example:8000/hot".to_owned()),
            options_hot: BTreeMap::from([
                ("secret_access_key".to_owned(), "AKIA-HOT-KEY".to_owned()),
                ("region".to_owned(), "eu-hot-1".to_owned()),
            ]),
            options_cold: BTreeMap::from([
                ("sas_token".to_owned(), "sv=COLD-TOKEN".to_owned()),
                ("region".to_owned(), "eu-cold-1".to_owned()),
            ]),
            ..access_at("/srv/demo/cold")
        };
        let debug = format!("{:?}", Job::Identify(access));
        for secret in [
            "hot-s3cr3t",
            "AKIA-HOT-KEY",
            "sv=COLD-TOKEN",
            "eu-hot-1",
            "eu-cold-1",
        ] {
            assert!(!debug.contains(secret), "{secret} leaked: {debug}");
        }
        // What helps with diagnosing stays: the hot repository's URL without
        // its password, and the names of the options of both halves.
        for kept in [
            "rest:https://demo:",
            "@hot.example:8000/hot",
            "options_hot",
            "options_cold",
            "secret_access_key",
            "sas_token",
            "region",
        ] {
            assert!(debug.contains(kept), "{kept} is gone: {debug}");
        }
    }

    #[test]
    fn a_job_hides_what_its_references_resolved_to_in_the_hot_repository_too() {
        let access = RepositoryAccess {
            secrets: vec![SecretValue::new("canary-hot-reference")],
            repo_hot: Some("rest:https://hot.example:8000/canary-hot-reference/hot".to_owned()),
            ..access_at("/srv/demo/cold")
        };
        let debug = format!("{:?}", Job::Identify(access));
        assert!(!debug.contains("canary-hot-reference"), "{debug}");
        assert!(
            debug.contains("rest:https://hot.example:8000/***/hot"),
            "{debug}"
        );
    }
}
