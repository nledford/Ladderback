use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use domain::execution::deadline::KeepDelete;
use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::execution::{FailureReason, Warning};
use domain::secrets::{REDACTED, Redactor, SecretValue};
use domain::{OperationId, RepositoryId, SignedDuration, SnapshotId, Timestamp};
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
///   at it stops without writing anything and ends with
///   `Failed(FailureReason::DeadlinePassed)`.
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
pub struct EngineError {
    pub detail: String,
}

/// What an operation should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// Touches no repository. Exercises the engine boundary (worker process,
    /// event protocol, crash handling) in tests and diagnostics.
    Fake(FakeJob),
    /// Opens a repository and reports its ID.
    Identify(RepositoryAccess),
    Backup(BackupJob),
    Prune(PruneJob),
    Check(CheckJob),
}

impl Job {
    /// How long the job may run after it starts reading the index; `None`
    /// when it has no deadline. See `domain::execution::deadline`.
    pub fn deadline_budget(&self) -> Option<SignedDuration> {
        match self {
            Self::Fake(job) => job.deadline_budget,
            Self::Backup(job) => Some(job.deadline_budget),
            Self::Identify(_) | Self::Prune(_) | Self::Check(_) => None,
        }
    }
}

/// How to reach and open one repository.
#[derive(Clone, PartialEq, Eq)]
pub struct RepositoryAccess {
    /// rustic's `repository` string: a path, `rest:…`, `opendal:…`, `rclone:…`.
    pub repository: String,
    pub password: SecretValue,
    /// Backend options, as rustic's `[repository.options]`.
    pub options: BTreeMap<String, String>,
    /// The repository's ID when already known; the engine uses it to give
    /// each worker its own cache dir. `None` before the first `Identify`.
    pub id: Option<RepositoryId>,
}

/// Everything that can hold a secret is masked (I4): the password, the
/// password in the repository's URL, and every option's value. The rest of
/// the URL and the options' names stay, for diagnosing.
impl std::fmt::Debug for RepositoryAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
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
    /// reserved `steading-` prefix.
    pub extra_tags: Vec<UserTag>,
    pub label: Option<String>,
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
    Fake { steps_completed: u32 },
    Repository { id: RepositoryId },
    Backup(BackupSummary),
    Prune(PruneSummary),
    Check(CheckSummary),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSummary {
    pub repository: RepositoryId,
    pub snapshot: SnapshotId,
    pub parent: Option<SnapshotId>,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
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

/// A check that ran to the end succeeds even when it found problems; they
/// are counted here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckSummary {
    pub repository: RepositoryId,
    pub errors: u64,
    pub warnings: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_never_shows_its_secrets_in_its_debug_form() {
        let access = RepositoryAccess {
            repository: "rest:https://demo:s3cr3t@nas:8000/backups".to_owned(),
            password: SecretValue::new("hunter2"),
            options: BTreeMap::from([(
                "secret_access_key".to_owned(),
                "AKIAEXAMPLEKEY".to_owned(),
            )]),
            id: None,
        };
        let debug = format!("{:?}", Job::Identify(access));
        for secret in ["hunter2", "s3cr3t", "AKIAEXAMPLEKEY"] {
            assert!(!debug.contains(secret), "{secret} leaked: {debug}");
        }
        for kept in ["https://demo:", "@nas:8000/backups", "secret_access_key"] {
            assert!(debug.contains(kept), "{kept} is gone: {debug}");
        }
    }
}
