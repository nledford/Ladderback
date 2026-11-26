//! Execution context: one operation's lifecycle (technical design §3.1).

pub mod conditions;
pub mod deadline;
pub mod full_disk_access;
pub mod locations;
pub mod logged_entry;
pub mod next_run;
pub mod plan_tags;
#[cfg(test)]
mod properties;
pub mod queue;
pub mod refusal;
pub mod retry;
pub mod wake;

use serde::{Deserialize, Serialize};

use crate::{OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp};

/// The most warnings one operation keeps (technical design §9). Warnings
/// past it are counted but not kept ([`Operation::warning_count`]), so a
/// source with millions of unreadable files can't grow the log without bound.
pub const MAX_STORED_WARNINGS: usize = 10_000;

/// The most bytes one text field of a kept warning holds: its item, what the
/// engine was doing and the engine's own message (technical design §9). Longer
/// text is cut when the warning is recorded and ends in [`WARNING_TEXT_CUT`],
/// so a worker's megabyte message, kept ten thousand times over, can't grow
/// the log without bound either.
pub const MAX_WARNING_TEXT_BYTES: usize = 4 * 1024;

/// What ends a warning's text that was cut to [`MAX_WARNING_TEXT_BYTES`].
pub const WARNING_TEXT_CUT: &str = "…";

/// Where an operation is in its lifecycle:
/// `Queued` → `Running` → `Succeeded` / `SucceededWithWarnings` / `Failed` / `Canceled`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    Queued,
    Running,
    Succeeded,
    SucceededWithWarnings,
    Failed {
        reason: FailureReason,
    },
    // Serialized as before the rename to American spelling, so nothing
    // written with the old name reads differently.
    #[serde(rename = "Cancelled")]
    Canceled,
}

impl OperationStatus {
    /// Whether the operation has ended and can no longer change.
    pub fn is_finished(&self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }

    /// Whether the operation failed because no repository is at its
    /// location ([`FailureReason::RepositoryNotFound`]): a saved repository
    /// whose latest operation did is one to create (decision 53).
    pub fn found_no_repository(&self) -> bool {
        matches!(
            self,
            Self::Failed {
                reason: FailureReason::RepositoryNotFound
            }
        )
    }

    /// What the operation tells of reading its repository: [`Read`] when it
    /// succeeded, [`Unreadable`] when it failed because the repository
    /// couldn't be opened or read where it was looked for (no repository or
    /// another one there, its credentials or password refused, no location
    /// usable, a secret of its settings unresolved), and `None` when it
    /// tells neither: it is queued, running or canceled, or failed for a
    /// reason of its own. The status summary names a repository whose last
    /// operation that tells was [`Unreadable`] (§4.6).
    ///
    /// [`Read`]: RepositoryReading::Read
    /// [`Unreadable`]: RepositoryReading::Unreadable
    pub fn repository_reading(&self) -> Option<RepositoryReading> {
        use FailureReason as Why;
        match self {
            Self::Succeeded | Self::SucceededWithWarnings => Some(RepositoryReading::Read),
            Self::Failed { reason } => match reason {
                Why::RepositoryReplaced
                | Why::RepositoryNotFound
                | Why::RepositoryConfigNotFound { .. }
                | Why::CredentialsRefused { .. }
                | Why::NoLocationUsable
                | Why::NoLocationRuleHolds
                | Why::PasswordNotAccepted
                | Why::SecretRefused { .. } => Some(RepositoryReading::Unreadable),
                Why::EngineUnavailable { .. }
                | Why::JobRejected { .. }
                | Why::EngineExited { .. }
                | Why::EngineFailed { .. }
                | Why::ProtocolViolated { .. }
                | Why::DeadlinePassed
                | Why::RepositoryExists
                | Why::EngineStartTimedOut
                | Why::EngineExitTimedOut
                | Why::HookFailed { .. }
                | Why::SecretsTooLargeToMask { .. }
                | Why::DaemonStopped
                | Why::DaemonShutDown
                | Why::Superseded
                | Why::PlanRemoved
                | Why::PlanMoved
                | Why::ConfigUnusable
                | Why::KeyFilesNotLeftOut
                | Why::RestoreTargetUnwritable
                | Why::LeaseLapsed
                | Why::DeletionUnanswered => None,
            },
            Self::Queued | Self::Running | Self::Canceled => None,
        }
    }
}

/// Whether an operation read its repository, as
/// [`OperationStatus::repository_reading`] tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryReading {
    Read,
    Unreadable,
}

/// Why an operation failed. Typed, never a sentence: adapters map it onto
/// published codes and the presentation layer renders it (§8.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    /// The engine couldn't be started at all, so the operation never ran.
    /// `detail` is the platform's own message.
    EngineUnavailable { detail: String },
    /// The engine refused the job it was given.
    JobRejected { detail: String },
    /// The engine stopped without reporting a result, for example because its
    /// process crashed or was killed. `exit_code` is `None` when no exit status
    /// is known: a signal ended it, its status couldn't be read, or the daemon
    /// never learned it.
    EngineExited { exit_code: Option<i32> },
    /// The engine reported that the operation failed. `detail` is the engine's own message.
    EngineFailed { detail: String },
    /// The engine reported events out of order: `transition` is the step the
    /// operation refused.
    ProtocolViolated { transition: Transition },
    /// A backup was still running at its deadline and stopped without
    /// writing a snapshot (I3).
    DeadlinePassed,
    /// A repository already exists where one was to be initialized, so
    /// nothing was written. The agent refuses to overwrite one.
    RepositoryExists,
    /// The repository at the entry's location has another repository ID than
    /// the one the configuration recorded for it (§7.3): the location now
    /// holds a different repository, so nothing was read or written.
    RepositoryReplaced,
    /// No repository is at the location: it is an empty folder on this
    /// machine, so nothing was opened and nothing was written. A saved
    /// repository whose latest operation failed this way is one to create
    /// (decision 53).
    RepositoryNotFound,
    /// No repository's config file was found at the location, and nothing
    /// tells whether the place is empty or out of reach: a folder that isn't
    /// there (an unplugged drive), a path that is no folder, a folder that
    /// can't be read, or a server or a store that answered with no config
    /// file. Never one to create from its row (decision 53). `detail` is the
    /// engine's own message, masked of the repository's secrets.
    RepositoryConfigNotFound { detail: String },
    /// The server the repository is on answered and refused the credentials
    /// it was sent (HTTP 401 or 403), so nothing was opened or written. Not
    /// a missing repository: the server never said what is there. `half`
    /// names the half whose server refused them, for a hot/cold repository
    /// only.
    CredentialsRefused { half: Option<RepositoryHalf> },
    /// The engine's process didn't start the operation in time, so it never
    /// ran: it didn't read its job, or read it and sent nothing (not even a
    /// log line) before the start timeout, and it was killed.
    EngineStartTimedOut,
    /// The engine stopped reporting, but its process stayed alive and didn't
    /// exit in time; it was killed.
    EngineExitTimedOut,
    /// A `run-before` hook with `on-failure = "error"` stopped the operation
    /// before the engine started (I4, §6.1): which hook it was and how it
    /// ended, never what it wrote. Either is `None` only for a failure an
    /// earlier version kept, which recorded no more than the exit status.
    HookFailed {
        hook: Option<HookPlace>,
        ended: Option<HookEnd>,
    },
    /// The engine refused to start the job: its secrets, with the proxy
    /// credentials of the engine's environment, come to more than `limit`
    /// bytes ([`MAX_MASKED_BYTES`](crate::secrets::MAX_MASKED_BYTES)), more
    /// than it masks in what the job shows (I4, §10.2). `field` names the
    /// setting at which they passed it, the proxy variable such as
    /// `HTTPS_PROXY`, and is `None` when the job's own secrets did. Neither is
    /// a secret or tells one.
    SecretsTooLargeToMask { field: Option<String>, limit: usize },
    /// The daemon stopped while the operation was queued or running, so the
    /// operation never reached its end: a crash, a kill, a power cut, or a
    /// stop that didn't get as far as recording it, left it unfinished in
    /// the log, and the next start ended it as failed (§9). What it did
    /// before it stopped is unknown, so it is never recorded as succeeded.
    DaemonStopped,
    /// The daemon was told to stop while the operation was running (a
    /// service stop, a restart, the computer shutting down), and its orderly
    /// stop ended it once the workers had stopped (§4.2, decision 92). It is
    /// recorded, not announced: whoever stopped the daemon knows why.
    DaemonShutDown,
    /// A queued backup's wait ended while a newer run of its plan was going,
    /// running or waiting for its turn, so it gave way to that run rather
    /// than back up the plan twice (decision 135, #1245): it never ran.
    Superseded,
    /// A queued backup's plan was deleted while the backup waited for its
    /// keyring, so it didn't run (decision 135, #1293).
    PlanRemoved,
    /// A queued backup's plan now backs up to another repository than the
    /// one the backup was queued for, found once its wait for its keyring
    /// was over, so it didn't run there (decision 135, #1293).
    PlanMoved,
    /// The configuration couldn't be read or used once a queued backup's
    /// wait for its keyring was over, so the backup didn't run (decision
    /// 135, #1293).
    ConfigUnusable,
    /// Once a queued backup's wait for its keyring was over, its plan's
    /// sources held the data folder, whose secret key files no glob can
    /// leave out as its path isn't UTF-8, so the backup didn't run rather
    /// than back the keys up (decisions 120 and 135, #1293).
    KeyFilesNotLeftOut,
    /// The repository is reached at several locations and none could be
    /// used (decision 80): each was silent, refused, held no repository or
    /// another one, or a secret of it was unavailable. The operation's
    /// locations say why, by position.
    NoLocationUsable,
    /// A restore couldn't write where it was to write: the restore folder,
    /// or a path it restores in place, refused it (§10.4). What it wrote
    /// before stays.
    RestoreTargetUnwritable,
    /// No location's rule held when the job started (decision 80): each
    /// rule wasn't met on the machine's network link, or couldn't be
    /// evaluated. The operation's locations say which, by position.
    NoLocationRuleHolds,
    /// The repository's password opened nothing at any location tried
    /// (decision 80): a wrong password is the likelier cause.
    PasswordNotAccepted,
    /// A run the schedule started was refused before anything ran, because
    /// the setting `field` of its repository (`password`, `repository`,
    /// `repo-hot`, `options.<name>`, `options-hot.<name>`,
    /// `options-cold.<name>`, as the configuration writes it) couldn't be resolved (§5, §10.2): the
    /// refusal a request to start it is answered with. Nothing ran, so no
    /// hook ran either.
    SecretRefused {
        field: String,
        refusal: refusal::SettingRefusal,
    },
    /// A prune came to its first delete and wasn't let go on: this paired
    /// machine no longer held a live lease on the repository from its hub
    /// (§7.3; §16, decision 161). Nothing was deleted.
    LeaseLapsed,
    /// A prune came to its first delete, asked whether this machine may
    /// still maintain the repository, and had no answer in time (§16,
    /// decision 161). Nothing was deleted.
    DeletionUnanswered,
}

impl FailureReason {
    /// The failure's stored code (§9): the kebab-case name the operation
    /// log's `error_code` column and a hook's `LADDERBACK_ERROR` carry.
    pub fn code(&self) -> &'static str {
        match self {
            Self::EngineUnavailable { .. } => "engine-unavailable",
            Self::JobRejected { .. } => "invalid-job-spec",
            Self::EngineExited { .. } => "engine-exited",
            Self::EngineFailed { .. } => "operation-failed",
            Self::ProtocolViolated { .. } => "protocol-violated",
            Self::DeadlinePassed => "deadline-passed",
            Self::EngineStartTimedOut => "engine-start-timed-out",
            Self::EngineExitTimedOut => "engine-exit-timed-out",
            Self::HookFailed { .. } => "hook-failed",
            Self::SecretsTooLargeToMask { .. } => "secrets-too-large-to-mask",
            Self::RepositoryExists => "repository-exists",
            Self::RepositoryReplaced => "repository-replaced",
            Self::RepositoryNotFound => "repository-not-found",
            Self::RepositoryConfigNotFound { .. } => "repository-config-not-found",
            Self::CredentialsRefused { .. } => "credentials-refused",
            Self::DaemonStopped => "daemon-stopped",
            Self::DaemonShutDown => "daemon-shut-down",
            Self::Superseded => "superseded",
            Self::PlanRemoved => "plan-removed",
            Self::PlanMoved => "plan-moved",
            Self::ConfigUnusable => "config-unusable",
            Self::KeyFilesNotLeftOut => "key-files-not-left-out",
            Self::NoLocationUsable => "no-location-usable",
            Self::RestoreTargetUnwritable => "restore-target-unwritable",
            Self::NoLocationRuleHolds => "no-location-rule-holds",
            Self::PasswordNotAccepted => "password-not-accepted",
            Self::SecretRefused { refusal, .. } => refusal.code(),
            Self::LeaseLapsed => "maintenance-lease-lapsed",
            Self::DeletionUnanswered => "maintenance-deletion-unanswered",
        }
    }
}

/// One half of a hot/cold repository: the cold half is its `repository`,
/// which holds everything, and the hot half its `repo-hot`, which holds all
/// but the data packs (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepositoryHalf {
    Cold,
    Hot,
}

impl RepositoryHalf {
    /// The half's name on the wire and in the operation log: `cold` or `hot`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Hot => "hot",
        }
    }

    /// The half [`code`](Self::code) names, or `None` for any other text.
    pub fn parse(code: &str) -> Option<Self> {
        [Self::Cold, Self::Hot]
            .into_iter()
            .find(|half| half.code() == code)
    }
}

/// Which hook of a run's lists it is (§6.1): the level whose list holds it
/// and its place in that list, counted from 1, so a person finds it in their
/// configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPlace {
    pub level: HookLevel,
    pub position: std::num::NonZeroU32,
}

/// Whose hooks a hook is one of (§6.1): the agent's, the repository's or the
/// plan's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookLevel {
    Agent,
    Repository,
    Plan,
}

impl HookLevel {
    /// The level's name on the wire and in the operation log: `machine`,
    /// `repository` or `plan`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Agent => "machine",
            Self::Repository => "repository",
            Self::Plan => "plan",
        }
    }

    /// The level [`code`](Self::code) names, or `None` for any other text.
    pub fn parse(code: &str) -> Option<Self> {
        [Self::Agent, Self::Repository, Self::Plan]
            .into_iter()
            .find(|level| level.code() == code)
    }
}

/// How a failing hook ended: it exited with a status other than zero, a
/// signal ended it, it ran out of time, or it could not be started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookEnd {
    Exited { code: i32 },
    Signalled,
    TimedOut,
    NotStarted,
}

impl HookEnd {
    /// How it ended, as the wire and the operation log name it: `exit`,
    /// `signal`, `timeout` or `not-started`. An exit's status is
    /// [`exit_code`](Self::exit_code).
    pub fn code(self) -> &'static str {
        match self {
            Self::Exited { .. } => "exit",
            Self::Signalled => "signal",
            Self::TimedOut => "timeout",
            Self::NotStarted => "not-started",
        }
    }

    /// The exit status, for a hook that exited.
    pub fn exit_code(self) -> Option<i32> {
        match self {
            Self::Exited { code } => Some(code),
            Self::Signalled | Self::TimedOut | Self::NotStarted => None,
        }
    }

    /// The end [`code`](Self::code) names, with `exit_code` for an exit, or
    /// `None` for any other text, an exit without its status or a status on
    /// anything but an exit.
    pub fn parse(code: &str, exit_code: Option<i32>) -> Option<Self> {
        match (code, exit_code) {
            ("exit", Some(code)) => Some(Self::Exited { code }),
            ("signal", None) => Some(Self::Signalled),
            ("timeout", None) => Some(Self::TimedOut),
            ("not-started", None) => Some(Self::NotStarted),
            _ => None,
        }
    }
}

/// A problem with a single item that doesn't fail the operation, such as a
/// file that couldn't be read during a backup.
///
/// Each text field is as the engine reported it until an operation keeps the
/// warning, which cuts a field longer than [`MAX_WARNING_TEXT_BYTES`]
/// ([`Operation::record_warning`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    /// The affected item, usually a path.
    pub item: Option<String>,
    /// What the engine was doing.
    pub during: String,
    /// The engine's own message, verbatim.
    pub detail: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    SourceUnreadable,
    /// A hook command around the operation failed where its `on-failure`
    /// says to go on: the operation carries it as a warning.
    Hook,
    /// A forget or an adoption that ran as several engine jobs ended before
    /// the last: the jobs that finished stay done. `during` says which
    /// (`forget` or `adopt`) and `detail` is `<done>/<total>` snapshots.
    MaintenancePartial,
    /// A check found something to tidy, such as a pack no index lists, which
    /// a backup killed before it saved its index leaves: what rustic reports
    /// at its warning level. Unless the check also found an error, no
    /// snapshot's data depends on it. `during` is `check` and `detail`
    /// rustic's own words, which name what it found.
    CheckWarning,
    /// A check found an error: damage in the repository (something in it
    /// that is not as its index or its snapshots say), or a part of it, or of
    /// the agent's cache of it, that could not be read or did not match, so a
    /// snapshot may not restore whole. What rustic reports at its error
    /// level, worded as [`Self::CheckWarning`].
    CheckError,
    /// A forget, an adoption or a prune ran on a repository maintained here
    /// that holds snapshots from other hosts, any of which may maintain it
    /// too (I1, §7.3). `during` is the run (`forget`, `adopt` or `prune`)
    /// and `detail` is `<count>: <hosts>`, how many other hosts there are and
    /// the first five of them by name, separated by `, `
    /// ([`OtherHosts`](crate::maintenance::OtherHosts)).
    OtherHosts,
    /// A location of the repository was skipped where it shouldn't have
    /// been (decision 80): it held another repository, a secret of it was
    /// unavailable, or its rule held and it couldn't be used all the same.
    /// `item` is the location's position, from 1, `during` is `location`
    /// and `detail` the outcome's code; never an address or a network's
    /// name.
    LocationSkipped,
    /// A secret the operation read came from a file other users of the
    /// machine can read (#386). `item` is the file's path, `during` is
    /// `secret` and `detail` the file's mode in octal (`0644`); never the
    /// secret (I4).
    SecretFileReadable,
    /// A secret the operation read came from a file other users of the
    /// machine can change, so they can choose the secret (#1137). `item`,
    /// `during` and `detail` as for [`Self::SecretFileReadable`].
    SecretFileWritable,
    /// On macOS, a source item the system refused (`EPERM`) during a backup
    /// whose Full Disk Access probe, run before it, found the grant missing
    /// (§16, decision 115): a [`Self::SourceUnreadable`] the grant explains.
    /// `item`, `during` and `detail` are the engine's, as for that kind.
    FullDiskAccessMissing,
    /// A warning this version doesn't know, for example from a newer worker.
    Other,
}

impl WarningKind {
    /// The warning's stored code (§9): the kebab-case name the operation
    /// log keeps it under, which the history's `warning-codes` and a
    /// notification's `{code}` carry too.
    pub fn code(self) -> &'static str {
        match self {
            Self::SourceUnreadable => "source-unreadable",
            Self::Hook => "hook-warning",
            Self::MaintenancePartial => "maintenance-partial",
            Self::CheckWarning => "check-warning",
            Self::CheckError => "check-error",
            Self::OtherHosts => "maintenance-other-hosts",
            Self::LocationSkipped => "location-skipped",
            Self::SecretFileReadable => "secret-file-readable",
            Self::SecretFileWritable => "secret-file-writable",
            Self::FullDiskAccessMissing => "full-disk-access-missing",
            Self::Other => "unknown",
        }
    }
}

/// The kinds of `warnings`, each once, in the order the first of each came:
/// what the history names an operation's warnings by (#508).
pub fn warning_kinds(warnings: &[Warning]) -> Vec<WarningKind> {
    let mut kinds = Vec::new();
    for warning in warnings {
        if !kinds.contains(&warning.kind) {
            kinds.push(warning.kind);
        }
    }
    kinds
}

/// What was attempted when a lifecycle transition was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transition {
    Start,
    ReportProgress,
    RecordWarning,
    Succeed,
    Fail,
    Cancel,
}

/// A refused lifecycle transition: a programming or protocol error, never
/// shown to users as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleError {
    pub transition: Transition,
    pub status: OperationStatus,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for LifecycleError {}

/// What an operation does: the `kind` the operation log stores (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationKind {
    /// A scripted job that touches no repository, for tests and diagnostics.
    Fake,
    Identify,
    /// Initializes a repository where nothing is yet, and reports its ID.
    Init,
    Backup,
    /// Removes the snapshots a forget run's selection named (I6).
    Forget,
    /// Rewrites foreign history with a plan of this agent's tags (§7.3):
    /// new snapshot files over the same trees, the originals removed.
    Adopt,
    Prune,
    Check,
    /// Lists a repository's snapshots. A read, not an operation: it never
    /// takes an I7 slot and is never written to the operation log (§9).
    Ls,
    /// Restores an entry of a snapshot into a new folder or in place
    /// (§10.4). It only reads the repository: no I7 slot.
    Restore,
    /// Hands an entry of a snapshot to the browser that asked for it
    /// (§10.4). A read: no I7 slot, never queued; the log keeps it so the
    /// history says who read what out of a repository.
    Download,
}

impl OperationKind {
    /// Whether the operation maintains a repository: forget, prune and check.
    /// Maintenance operations on one repository run one at a time (I7).
    pub fn is_maintenance(self) -> bool {
        match self {
            Self::Forget | Self::Adopt | Self::Prune | Self::Check => true,
            Self::Fake
            | Self::Identify
            | Self::Init
            | Self::Backup
            | Self::Ls
            | Self::Restore
            | Self::Download => false,
        }
    }
}

/// What started an operation: the `trigger` the operation log stores (§9).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trigger {
    Schedule,
    /// The user asked for it.
    Manual,
    /// A scheduled run that was missed while the agent was off, started late.
    CatchUp,
    Retry,
    Hub,
    Mcp,
    /// A program asked for it with an API token: the token's ID and label,
    /// never its secret (§10.1, decision 93).
    ApiToken {
        id: crate::access::api_token::ApiTokenId,
        label: crate::access::api_token::ApiTokenLabel,
    },
}

impl Trigger {
    /// Whether nobody is waiting on the run's answer: the schedule started
    /// it, late after a wake or as a retry. Only such a run waits for the
    /// session's keyring service while the daemon starts (§5).
    pub fn is_unattended(&self) -> bool {
        matches!(self, Self::Schedule | Self::CatchUp | Self::Retry)
    }
}

/// What an operation works on, as far as its job says: a backup names its
/// plan, and a job that opens a known repository names it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub plan: Option<PlanId>,
    pub repository: Option<RepositoryId>,
}

impl Target {
    /// Whether a job that opens `repository` (`None`: a repository whose ID
    /// isn't known yet) works on the repository this target names. A target
    /// that names none was made before its repository's ID was known, so no
    /// repository can be told apart from it.
    pub fn is_on(&self, repository: Option<&RepositoryId>) -> bool {
        self.repository
            .as_ref()
            .is_none_or(|named| Some(named) == repository)
    }
}

/// One run of one action. Aggregate root of the Execution context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    id: OperationId,
    kind: OperationKind,
    trigger: Trigger,
    target: Target,
    status: OperationStatus,
    queued_at: Timestamp,
    started_at: Option<Timestamp>,
    finished_at: Option<Timestamp>,
    warnings: Vec<Warning>,
    /// Every warning recorded; `warnings` keeps only the first
    /// [`MAX_STORED_WARNINGS`].
    warning_count: u64,
    /// The alias this agent gave the repository a maintenance operation runs
    /// on, or a restore or a download reads, as it stood when the log first
    /// wrote the run: a repository renamed since keeps its old name in the
    /// log (§16, decision 52). `None` for a backup, which its plan names, and
    /// for an operation an earlier build logged.
    #[serde(default)]
    repository_alias: Option<RepositoryAlias>,
    /// Which location of a repository reached at several the run used, and
    /// why each earlier one was skipped (decision 80). `None` for a
    /// repository of one location and an operation that opens none.
    #[serde(default)]
    locations: Option<locations::LocationsRecord>,
    /// The entry of a snapshot a restore or a download read, as the log
    /// names it (#847). `None` for every other operation, and for one an
    /// earlier build logged.
    #[serde(default)]
    entry: Option<logged_entry::LoggedEntry>,
    /// Where a restore wrote what it restored (#813). `None` for every
    /// other operation, and for a restore an earlier build logged.
    #[serde(default)]
    restored_to: Option<logged_entry::RestoredTo>,
}

impl Operation {
    pub fn queue(
        id: OperationId,
        kind: OperationKind,
        trigger: Trigger,
        target: Target,
        at: Timestamp,
    ) -> Self {
        Self {
            id,
            kind,
            trigger,
            target,
            status: OperationStatus::Queued,
            queued_at: at,
            started_at: None,
            finished_at: None,
            warnings: Vec::new(),
            warning_count: 0,
            repository_alias: None,
            locations: None,
            entry: None,
            restored_to: None,
        }
    }

    /// A run refused before anything ran (§5): queued and failed with
    /// `reason` at `at`, never started.
    pub fn refused(
        id: OperationId,
        kind: OperationKind,
        trigger: Trigger,
        target: Target,
        reason: FailureReason,
        at: Timestamp,
    ) -> Self {
        let mut operation = Self::queue(id, kind, trigger, target, at);
        operation.finish(OperationStatus::Failed { reason }, at);
        operation
    }

    /// The operation, named with the alias of the repository it runs on.
    pub fn with_repository_alias(self, alias: RepositoryAlias) -> Self {
        Self {
            repository_alias: Some(alias),
            ..self
        }
    }

    /// The alias the repository had when the operation was queued, when one
    /// was given ([`Self::with_repository_alias`]).
    pub fn repository_alias(&self) -> Option<&RepositoryAlias> {
        self.repository_alias.as_ref()
    }

    /// The operation, naming the entry of a snapshot it reads.
    pub fn with_entry(self, entry: logged_entry::LoggedEntry) -> Self {
        Self {
            entry: Some(entry),
            ..self
        }
    }

    /// The entry of a snapshot the operation reads, when it was given
    /// ([`Self::with_entry`]).
    pub fn entry(&self) -> Option<&logged_entry::LoggedEntry> {
        self.entry.as_ref()
    }

    /// The operation, naming where the restore it is writes.
    pub fn with_restored_to(self, restored_to: logged_entry::RestoredTo) -> Self {
        Self {
            restored_to: Some(restored_to),
            ..self
        }
    }

    /// Where the restore it is writes, when it was given
    /// ([`Self::with_restored_to`]).
    pub fn restored_to(&self) -> Option<&logged_entry::RestoredTo> {
        self.restored_to.as_ref()
    }

    /// Which location the run used and why each earlier one was skipped,
    /// for a repository reached at several.
    pub fn locations(&self) -> Option<&locations::LocationsRecord> {
        self.locations.as_ref()
    }

    /// Records the run's choice of location, and the warnings it gives
    /// (`rule_held` says, by position, whether a location's rule held).
    pub fn record_locations(
        &mut self,
        record: locations::LocationsRecord,
        rule_held: impl Fn(u8) -> bool,
    ) -> Result<(), LifecycleError> {
        self.require(Transition::RecordWarning, |s| !s.is_finished())?;
        for warning in record.warnings(rule_held) {
            self.keep(warning);
        }
        self.locations = Some(record);
        Ok(())
    }

    pub fn id(&self) -> OperationId {
        self.id
    }

    pub fn kind(&self) -> OperationKind {
        self.kind
    }

    pub fn trigger(&self) -> &Trigger {
        &self.trigger
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn status(&self) -> &OperationStatus {
        &self.status
    }

    pub fn queued_at(&self) -> Timestamp {
        self.queued_at
    }

    pub fn started_at(&self) -> Option<Timestamp> {
        self.started_at
    }

    pub fn finished_at(&self) -> Option<Timestamp> {
        self.finished_at
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Every warning recorded, kept or not: at least `warnings().len()`.
    pub fn warning_count(&self) -> u64 {
        self.warning_count
    }

    /// Time from start to finish; `None` until the operation has both started and finished.
    pub fn duration(&self) -> Option<SignedDuration> {
        Some(self.finished_at?.duration_since(self.started_at?))
    }

    pub fn start(&mut self, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Start, |s| *s == OperationStatus::Queued)?;
        self.status = OperationStatus::Running;
        self.started_at = Some(at);
        Ok(())
    }

    /// Starts a queued operation whose job opened `opened`, as the engine
    /// reported when it started. A target queued before its repository's ID
    /// was known names no repository, and names `opened` from now on, so the
    /// log says which repository the run is on (§9). A target that names a
    /// repository keeps it: the engine opens only the repository a job
    /// names (§7.3).
    pub fn start_on(
        &mut self,
        at: Timestamp,
        opened: Option<&RepositoryId>,
    ) -> Result<(), LifecycleError> {
        self.start(at)?;
        if self.target.repository.is_none() {
            self.target.repository = opened.cloned();
        }
        Ok(())
    }

    /// Progress only comes from a running operation.
    pub fn report_progress(&self) -> Result<(), LifecycleError> {
        self.require(Transition::ReportProgress, |s| {
            *s == OperationStatus::Running
        })
    }

    /// Records a warning. Only the first [`MAX_STORED_WARNINGS`] are kept,
    /// but every one is counted ([`Self::warning_count`]). A kept warning's
    /// text is cut to [`MAX_WARNING_TEXT_BYTES`] in each field, ending in
    /// [`WARNING_TEXT_CUT`] where it was.
    pub fn record_warning(&mut self, warning: Warning) -> Result<(), LifecycleError> {
        self.require(Transition::RecordWarning, |s| {
            *s == OperationStatus::Running
        })?;
        self.keep(warning);
        Ok(())
    }

    /// Records a warning from outside the running phase, as a hook does:
    /// before the operation starts, or after it finished. A success that had
    /// no warnings until then ends as [`OperationStatus::SucceededWithWarnings`];
    /// a canceled operation takes none, since its run was given up.
    pub fn attach_warning(&mut self, warning: Warning) -> Result<(), LifecycleError> {
        self.require(Transition::RecordWarning, |s| {
            !matches!(s, OperationStatus::Canceled)
        })?;
        self.keep(warning);
        if self.status == OperationStatus::Succeeded {
            self.status = OperationStatus::SucceededWithWarnings;
        }
        Ok(())
    }

    /// Counts `warning` and keeps a cut copy of it while there is room
    /// ([`Self::record_warning`]). An operation names a secret file others
    /// can read or change once, however many of its reads met it (#386):
    /// such a warning for a path already kept is neither kept nor counted
    /// again.
    fn keep(&mut self, warning: Warning) {
        let item = warning.item.map(cut_warning_text);
        if matches!(
            warning.kind,
            WarningKind::SecretFileReadable | WarningKind::SecretFileWritable
        ) && self
            .warnings
            .iter()
            .any(|kept| kept.kind == warning.kind && kept.item == item)
        {
            return;
        }
        self.warning_count = self.warning_count.saturating_add(1);
        if self.warnings.len() < MAX_STORED_WARNINGS {
            // Every field is named, so a text field added to `Warning` must
            // be given its cut, or left whole on purpose, to compile.
            self.warnings.push(Warning {
                kind: warning.kind,
                item,
                during: cut_warning_text(warning.during),
                detail: cut_warning_text(warning.detail),
            });
        }
    }

    /// Ends a running operation successfully: `SucceededWithWarnings` if any
    /// warning was recorded, otherwise `Succeeded`.
    pub fn succeed(&mut self, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Succeed, |s| *s == OperationStatus::Running)?;
        let status = if self.warning_count == 0 {
            OperationStatus::Succeeded
        } else {
            OperationStatus::SucceededWithWarnings
        };
        self.finish(status, at);
        Ok(())
    }

    /// Ends a queued or running operation as failed. A queued operation can
    /// fail when its engine can't be started at all.
    pub fn fail(&mut self, reason: FailureReason, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Fail, |s| !s.is_finished())?;
        self.finish(OperationStatus::Failed { reason }, at);
        Ok(())
    }

    pub fn cancel(&mut self, at: Timestamp) -> Result<(), LifecycleError> {
        self.require(Transition::Cancel, |s| !s.is_finished())?;
        self.finish(OperationStatus::Canceled, at);
        Ok(())
    }

    fn require(
        &self,
        transition: Transition,
        allowed: impl FnOnce(&OperationStatus) -> bool,
    ) -> Result<(), LifecycleError> {
        if allowed(&self.status) {
            Ok(())
        } else {
            Err(LifecycleError {
                transition,
                status: self.status.clone(),
            })
        }
    }

    fn finish(&mut self, status: OperationStatus, at: Timestamp) {
        self.status = status;
        self.finished_at = Some(at);
    }
}

/// How much of a cut text is kept: the cap, less the room the marker takes.
const KEPT_WHEN_CUT: usize = MAX_WARNING_TEXT_BYTES - WARNING_TEXT_CUT.len();

/// `text` as a warning keeps it: whole when it fits [`MAX_WARNING_TEXT_BYTES`],
/// otherwise its longest prefix that ends on a character boundary and leaves
/// room for [`WARNING_TEXT_CUT`], which follows it. A cut text also gives back
/// the memory of the text it was cut from, since a kept warning stays in the
/// operation until it's saved.
fn cut_warning_text(mut text: String) -> String {
    if text.len() > MAX_WARNING_TEXT_BYTES {
        text.truncate(text.floor_char_boundary(KEPT_WHEN_CUT));
        text.push_str(WARNING_TEXT_CUT);
        text.shrink_to_fit();
    }
    text
}

#[cfg(test)]
mod tests;
