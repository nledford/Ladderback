//! Execution context: one operation's lifecycle (technical design §3.1).

pub mod conditions;
pub mod deadline;
pub mod locations;
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
/// `Queued` → `Running` → `Succeeded` / `SucceededWithWarnings` / `Failed` / `Cancelled`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    Queued,
    Running,
    Succeeded,
    SucceededWithWarnings,
    Failed { reason: FailureReason },
    Cancelled,
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
    /// A repository already exists where one was to be initialised, so
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
    /// The server the repository is on answered and refused the credentials
    /// it was sent (HTTP 401 or 403), so nothing was opened or written. Not
    /// a missing repository: the server never said what is there.
    CredentialsRefused,
    /// The engine's process didn't start the operation in time, so it never
    /// ran: it didn't read its job, or read it and sent nothing (not even a
    /// log line) before the start timeout, and it was killed.
    EngineStartTimedOut,
    /// The engine stopped reporting, but its process stayed alive and didn't
    /// exit in time; it was killed.
    EngineExitTimedOut,
    /// A `run-before` hook with `on-failure = "error"` stopped the operation
    /// before the engine started (I4, §6.1). `exit_code` is the hook's exit
    /// status, or `None` when it timed out or could not be started.
    HookFailed { exit_code: Option<i32> },
    /// The engine refused to start the job: its secrets, with the proxy
    /// credentials of the engine's environment, come to more than `limit`
    /// bytes ([`MAX_MASKED_BYTES`](crate::secrets::MAX_MASKED_BYTES)), more
    /// than it masks in what the job shows (I4, §10.2). `field` names the
    /// setting at which they passed it, the proxy variable such as
    /// `HTTPS_PROXY`, and is `None` when the job's own secrets did. Neither is
    /// a secret or tells one.
    SecretsTooLargeToMask { field: Option<String>, limit: usize },
    /// The daemon stopped while the operation was queued or running, so the
    /// operation never reached its end: a crash, a kill, a power cut or a
    /// restart left it unfinished in the log, and the next start ended it as
    /// failed (§9). What it did before it stopped is unknown, so it is never
    /// recorded as succeeded.
    DaemonStopped,
    /// The repository is reached at several locations and none could be
    /// used (decision 80): each was silent, refused, held no repository or
    /// another one, or a secret of it was unavailable. The operation's
    /// locations say why, by position.
    NoLocationUsable,
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
            Self::CredentialsRefused => "credentials-refused",
            Self::DaemonStopped => "daemon-stopped",
            Self::NoLocationUsable => "no-location-usable",
            Self::NoLocationRuleHolds => "no-location-rule-holds",
            Self::PasswordNotAccepted => "password-not-accepted",
            Self::SecretRefused { refusal, .. } => refusal.code(),
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
    /// A warning this version doesn't know, for example from a newer worker.
    Other,
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
    /// Initialises a repository where nothing is yet, and reports its ID.
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
}

impl OperationKind {
    /// Whether the operation maintains a repository: forget, prune and check.
    /// Maintenance operations on one repository run one at a time (I7).
    pub fn is_maintenance(self) -> bool {
        match self {
            Self::Forget | Self::Adopt | Self::Prune | Self::Check => true,
            Self::Fake | Self::Identify | Self::Init | Self::Backup | Self::Ls => false,
        }
    }
}

/// What started an operation: the `trigger` the operation log stores (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trigger {
    Schedule,
    /// The user asked for it.
    Manual,
    /// A scheduled run that was missed while the agent was off, started late.
    CatchUp,
    Retry,
    Hub,
    Mcp,
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
    /// on, as it stood when the run started: a repository renamed since keeps
    /// its old name in the log (§16, decision 52). `None` for a backup, which
    /// its plan names, and for an operation an earlier build logged.
    #[serde(default)]
    repository_alias: Option<RepositoryAlias>,
    /// Which location of a repository reached at several the run used, and
    /// why each earlier one was skipped (decision 80). `None` for a
    /// repository of one location and an operation that opens none.
    #[serde(default)]
    locations: Option<locations::LocationsRecord>,
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

    pub fn trigger(&self) -> Trigger {
        self.trigger
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
    /// a cancelled operation takes none, since its run was given up.
    pub fn attach_warning(&mut self, warning: Warning) -> Result<(), LifecycleError> {
        self.require(Transition::RecordWarning, |s| {
            !matches!(s, OperationStatus::Cancelled)
        })?;
        self.keep(warning);
        if self.status == OperationStatus::Succeeded {
            self.status = OperationStatus::SucceededWithWarnings;
        }
        Ok(())
    }

    /// Counts `warning` and keeps a cut copy of it while there is room
    /// ([`Self::record_warning`]).
    fn keep(&mut self, warning: Warning) {
        self.warning_count = self.warning_count.saturating_add(1);
        if self.warnings.len() < MAX_STORED_WARNINGS {
            // Every field is named, so a text field added to `Warning` must
            // be given its cut, or left whole on purpose, to compile.
            self.warnings.push(Warning {
                kind: warning.kind,
                item: warning.item.map(cut_warning_text),
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
        self.finish(OperationStatus::Cancelled, at);
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
mod tests {
    use super::*;
    use uuid::Uuid;

    fn t(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).unwrap()
    }

    fn queued() -> Operation {
        Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(7)),
            OperationKind::Backup,
            Trigger::Manual,
            Target::default(),
            t(0),
        )
    }

    fn running() -> Operation {
        let mut op = queued();
        op.start(t(10)).unwrap();
        op
    }

    fn unreadable(path: &str) -> Warning {
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some(path.to_owned()),
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        }
    }

    /// The history names each kind once, in the order its first warning
    /// came, however many of it there are (#508).
    #[test]
    fn an_operations_warnings_are_named_by_kind_each_once_in_the_order_they_came() {
        let check = Warning {
            kind: WarningKind::CheckWarning,
            item: None,
            during: "check".to_owned(),
            detail: "pack not referenced in index".to_owned(),
        };
        let warnings = [
            unreadable("/home/demo/a"),
            check.clone(),
            unreadable("/home/demo/b"),
            check,
            unreadable("/home/demo/c"),
        ];
        assert_eq!(
            warning_kinds(&warnings),
            [WarningKind::SourceUnreadable, WarningKind::CheckWarning]
        );
        assert_eq!(warning_kinds(&[]), Vec::new());
    }

    /// An operation names a readable secret file once, however many of its
    /// reads met it: at its start and again in its choice of location
    /// (#386). Another file, or another kind of warning on the same path,
    /// is still said.
    #[test]
    fn an_operation_names_a_readable_secret_file_once() {
        let readable = |path: &str| Warning {
            kind: WarningKind::SecretFileReadable,
            item: Some(path.to_owned()),
            during: "secret".to_owned(),
            detail: "0644".to_owned(),
        };
        let mut op = queued();
        op.attach_warning(readable("/home/demo/nas-password"))
            .unwrap();
        op.start(t(10)).unwrap();
        op.record_warning(readable("/home/demo/nas-password"))
            .unwrap();
        op.record_warning(readable("/home/demo/lan-key")).unwrap();
        op.record_warning(unreadable("/home/demo/nas-password"))
            .unwrap();
        op.record_warning(readable("/home/demo/lan-key")).unwrap();
        assert_eq!(
            op.warnings(),
            [
                readable("/home/demo/nas-password"),
                readable("/home/demo/lan-key"),
                unreadable("/home/demo/nas-password"),
            ]
        );
        assert_eq!(op.warning_count(), 3);
    }

    fn failed() -> OperationStatus {
        OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: Some(1) },
        }
    }

    /// A running operation that has recorded `count` distinct warnings.
    fn warned(count: usize) -> Operation {
        let mut op = running();
        for n in 0..count {
            op.record_warning(unreadable(&format!("/home/demo/file-{n}")))
                .unwrap();
        }
        op
    }

    fn repository(digit: &str) -> RepositoryId {
        RepositoryId::parse(&digit.repeat(64)).unwrap()
    }

    fn on(repository: Option<RepositoryId>) -> Target {
        Target {
            plan: None,
            repository,
        }
    }

    #[test]
    fn a_target_is_on_the_repository_it_names_and_on_no_other() {
        let named = on(Some(repository("a")));
        assert!(named.is_on(Some(&repository("a"))));
        assert!(!named.is_on(Some(&repository("b"))));
        assert!(!named.is_on(None));
    }

    #[test]
    fn a_target_that_names_no_repository_is_on_any() {
        let unnamed = on(None);
        assert!(unnamed.is_on(Some(&repository("a"))));
        assert!(unnamed.is_on(None));
    }

    fn queued_on(repository: Option<RepositoryId>) -> Operation {
        Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(7)),
            OperationKind::Backup,
            Trigger::Manual,
            on(repository),
            t(0),
        )
    }

    #[test]
    fn an_operation_queued_before_its_repository_was_known_names_the_one_it_started_on() {
        let mut op = queued_on(None);
        op.start_on(t(10), Some(&repository("a"))).unwrap();
        assert_eq!(op.status(), &OperationStatus::Running);
        assert_eq!(op.started_at(), Some(t(10)));
        assert_eq!(op.target(), &on(Some(repository("a"))));
    }

    #[test]
    fn an_operation_that_names_its_repository_keeps_it_when_it_starts() {
        let mut op = queued_on(Some(repository("a")));
        op.start_on(t(10), Some(&repository("b"))).unwrap();
        assert_eq!(op.target(), &on(Some(repository("a"))));
        let mut op = queued_on(Some(repository("a")));
        op.start_on(t(10), None).unwrap();
        assert_eq!(op.target(), &on(Some(repository("a"))));
    }

    #[test]
    fn a_start_that_opened_no_repository_names_none() {
        let mut op = queued_on(None);
        op.start_on(t(10), None).unwrap();
        assert_eq!(op.status(), &OperationStatus::Running);
        assert_eq!(op.target(), &on(None));
    }

    #[test]
    fn an_operation_that_cannot_start_names_no_repository() {
        let mut op = queued_on(None);
        op.start(t(5)).unwrap();
        let error = op.start_on(t(10), Some(&repository("a"))).unwrap_err();
        assert_eq!(error.transition, Transition::Start);
        assert_eq!(op.target(), &on(None));
        assert_eq!(op.started_at(), Some(t(5)));
    }

    #[test]
    fn an_operation_remembers_what_it_is_for() {
        let target = Target {
            plan: Some(PlanId::parse("demo-home").unwrap()),
            repository: Some(RepositoryId::parse(&"ab".repeat(32)).unwrap()),
        };
        let op = Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(8)),
            OperationKind::Prune,
            Trigger::CatchUp,
            target.clone(),
            t(0),
        );
        assert_eq!(op.kind(), OperationKind::Prune);
        assert_eq!(op.trigger(), Trigger::CatchUp);
        assert_eq!(op.target(), &target);
    }

    #[test]
    fn an_operation_keeps_the_alias_of_the_repository_it_runs_on() {
        let alias = RepositoryAlias::parse("demo").unwrap();
        assert_eq!(queued().repository_alias(), None);
        let mut op = queued().with_repository_alias(alias.clone());
        op.start(t(10)).unwrap();
        op.succeed(t(20)).unwrap();
        assert_eq!(op.repository_alias(), Some(&alias));
    }

    #[test]
    fn warnings_up_to_the_cap_are_all_kept() {
        let op = warned(MAX_STORED_WARNINGS);
        assert_eq!(op.warnings().len(), MAX_STORED_WARNINGS);
        assert_eq!(op.warning_count(), MAX_STORED_WARNINGS as u64);
    }

    #[test]
    fn warnings_past_the_cap_are_counted_but_not_kept() {
        let op = warned(MAX_STORED_WARNINGS + 5);
        assert_eq!(op.warnings().len(), MAX_STORED_WARNINGS);
        assert_eq!(op.warning_count(), MAX_STORED_WARNINGS as u64 + 5);
    }

    #[test]
    fn the_warnings_kept_are_the_first_ones_in_order() {
        let op = warned(MAX_STORED_WARNINGS + 1);
        let last_kept = format!("/home/demo/file-{}", MAX_STORED_WARNINGS - 1);
        assert_eq!(
            op.warnings().first(),
            Some(&unreadable("/home/demo/file-0"))
        );
        assert_eq!(op.warnings().last(), Some(&unreadable(&last_kept)));
    }

    #[test]
    fn a_refused_warning_is_not_counted() {
        let mut op = queued();
        assert!(op.record_warning(unreadable("/home/demo/a")).is_err());
        assert_eq!(op.warning_count(), 0);
    }

    /// The most of a text that survives a cut: the cap less the room the
    /// marker takes.
    const ROOM: usize = MAX_WARNING_TEXT_BYTES - WARNING_TEXT_CUT.len();

    /// `text` cut after its first `bytes` bytes, and marked.
    fn cut_after(text: &str, bytes: usize) -> String {
        format!("{}{WARNING_TEXT_CUT}", &text[..bytes])
    }

    #[test]
    fn text_up_to_the_cap_is_kept_whole() {
        for len in [
            0,
            1,
            ROOM,
            MAX_WARNING_TEXT_BYTES - 1,
            MAX_WARNING_TEXT_BYTES,
        ] {
            let text = "x".repeat(len);
            assert_eq!(cut_warning_text(text.clone()), text, "{len} bytes");
        }
    }

    #[test]
    fn text_one_byte_past_the_cap_is_cut_to_the_cap_and_marked() {
        let text = "x".repeat(MAX_WARNING_TEXT_BYTES + 1);
        let got = cut_warning_text(text.clone());
        assert_eq!(got, cut_after(&text, ROOM));
        assert_eq!(got.len(), MAX_WARNING_TEXT_BYTES);
    }

    /// A kept warning lives in the operation until it's saved, so a cut text
    /// must give back the room of the text it was cut from, not only shorten.
    #[test]
    fn a_cut_text_holds_no_more_memory_than_the_cap() {
        let megabyte = "x".repeat(1024 * 1024);
        let got = cut_warning_text(megabyte);
        assert!(
            got.capacity() <= 2 * MAX_WARNING_TEXT_BYTES,
            "capacity {} after the cut",
            got.capacity()
        );
    }

    /// The other tests state the cap through its constant; this one pins the
    /// value the design gives, 4 KiB, with the marker inside it.
    #[test]
    fn the_cap_is_4096_bytes_and_a_cut_text_is_4093_bytes_and_a_marker() {
        let whole = "x".repeat(4096);
        assert_eq!(cut_warning_text(whole.clone()), whole);

        let cut = cut_warning_text("x".repeat(4097));
        assert_eq!(cut.len(), 4096);
        assert_eq!(cut, format!("{}…", "x".repeat(4093)));
    }

    #[test]
    fn text_as_long_as_a_worker_may_send_is_cut_to_the_cap_too() {
        // One worker protocol message is at most 1 MiB (technical design §4.1).
        let text = "x".repeat(1024 * 1024);
        let got = cut_warning_text(text.clone());
        assert_eq!(got, cut_after(&text, ROOM));
        assert_eq!(got.len(), MAX_WARNING_TEXT_BYTES);
    }

    #[test]
    fn a_cut_never_splits_a_character_and_keeps_every_whole_one_that_fits() {
        // 2, 3 and 4 bytes wide.
        for c in ['é', '€', '😀'] {
            let width = c.len_utf8();
            // Every way the cut can fall: just before a `c`, inside one, and
            // just after one.
            for pad in (ROOM - width)..=ROOM {
                let mut text = "a".repeat(pad);
                while text.len() <= MAX_WARNING_TEXT_BYTES {
                    text.push(c);
                }
                let fits = (ROOM - pad) / width;
                let want = format!(
                    "{}{}{WARNING_TEXT_CUT}",
                    "a".repeat(pad),
                    c.to_string().repeat(fits)
                );

                let got = cut_warning_text(text);

                assert_eq!(got, want, "{c:?} after {pad} bytes");
                assert!(got.len() <= MAX_WARNING_TEXT_BYTES, "{c:?} after {pad}");
            }
        }
    }

    #[test]
    fn text_of_exactly_the_cap_is_kept_whole_with_a_character_where_a_cut_would_fall() {
        for c in ['é', '€', '😀'] {
            let width = c.len_utf8();
            let mut text = c.to_string().repeat(MAX_WARNING_TEXT_BYTES / width);
            text.push_str(&"a".repeat(MAX_WARNING_TEXT_BYTES % width));
            assert_eq!(text.len(), MAX_WARNING_TEXT_BYTES);

            assert_eq!(cut_warning_text(text.clone()), text, "{c:?}");
        }
    }

    /// What a running operation keeps of `warning`, recorded alone.
    fn kept(warning: Warning) -> Warning {
        let mut op = running();
        op.record_warning(warning).unwrap();
        assert_eq!(op.warnings().len(), 1);
        op.warnings().first().cloned().unwrap()
    }

    #[test]
    fn each_text_field_of_a_kept_warning_is_cut_on_its_own() {
        let short = unreadable("/home/demo/locked.txt");
        let long = "y".repeat(MAX_WARNING_TEXT_BYTES + 100);
        let cut = cut_after(&long, ROOM);

        let long_item = Warning {
            item: Some(long.clone()),
            ..short.clone()
        };
        let long_during = Warning {
            during: long.clone(),
            ..short.clone()
        };
        let long_detail = Warning {
            detail: long,
            ..short.clone()
        };

        assert_eq!(
            kept(long_item),
            Warning {
                item: Some(cut.clone()),
                ..short.clone()
            }
        );
        assert_eq!(
            kept(long_during),
            Warning {
                during: cut.clone(),
                ..short.clone()
            }
        );
        assert_eq!(
            kept(long_detail),
            Warning {
                detail: cut,
                ..short
            }
        );
    }

    #[test]
    fn a_missing_item_and_empty_text_stay_as_they_are_when_other_text_is_cut() {
        let long = "x".repeat(MAX_WARNING_TEXT_BYTES + 1);
        let warning = Warning {
            kind: WarningKind::Other,
            item: None,
            during: String::new(),
            detail: long.clone(),
        };

        assert_eq!(
            kept(warning),
            Warning {
                kind: WarningKind::Other,
                item: None,
                during: String::new(),
                detail: cut_after(&long, ROOM),
            }
        );
    }

    #[test]
    fn a_cut_warning_is_counted_once_like_any_other() {
        let mut op = running();
        op.record_warning(unreadable(&"x".repeat(MAX_WARNING_TEXT_BYTES * 2)))
            .unwrap();
        assert_eq!(op.warning_count(), 1);
        assert_eq!(op.warnings().len(), 1);
        assert_eq!(
            op.warnings()
                .first()
                .and_then(|w| w.item.as_deref())
                .map(str::len),
            Some(MAX_WARNING_TEXT_BYTES)
        );
    }

    #[test]
    fn only_a_running_operation_reports_progress() {
        assert_eq!(
            queued().report_progress(),
            Err(LifecycleError {
                transition: Transition::ReportProgress,
                status: OperationStatus::Queued,
            })
        );
        assert_eq!(running().report_progress(), Ok(()));
        let mut done = running();
        done.succeed(t(20)).unwrap();
        assert!(done.report_progress().is_err());
    }

    #[test]
    fn a_new_operation_is_queued_with_no_start_or_finish() {
        let op = queued();
        assert_eq!(op.status(), &OperationStatus::Queued);
        assert_eq!(op.queued_at(), t(0));
        assert_eq!(op.started_at(), None);
        assert_eq!(op.finished_at(), None);
        assert_eq!(op.duration(), None);
    }

    #[test]
    fn starting_a_queued_operation_makes_it_running() {
        let op = running();
        assert_eq!(op.status(), &OperationStatus::Running);
        assert_eq!(op.started_at(), Some(t(10)));
    }

    #[test]
    fn an_operation_without_warnings_succeeds() {
        let mut op = running();
        op.succeed(t(70)).unwrap();
        assert_eq!(op.status(), &OperationStatus::Succeeded);
        assert_eq!(op.finished_at(), Some(t(70)));
        assert_eq!(op.duration(), Some(SignedDuration::from_secs(60)));
    }

    #[test]
    fn warnings_turn_success_into_success_with_warnings() {
        let mut op = running();
        op.record_warning(unreadable("/a")).unwrap();
        op.record_warning(unreadable("/b")).unwrap();
        op.succeed(t(20)).unwrap();
        assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);
        assert_eq!(op.warnings(), &[unreadable("/a"), unreadable("/b")]);
    }

    #[test]
    fn a_choice_of_location_is_recorded_with_its_warnings_until_the_operation_ends() {
        use locations::{LocationOutcome, LocationTried, LocationsRecord};
        let record = LocationsRecord {
            locations: vec![
                LocationTried {
                    position: 1,
                    address: "/srv/a".to_owned(),
                    outcome: LocationOutcome::OtherRepository,
                    source: None,
                },
                LocationTried {
                    position: 2,
                    address: "/srv/b".to_owned(),
                    outcome: LocationOutcome::Used,
                    source: None,
                },
            ],
        };
        let mut op = queued();
        assert_eq!(op.locations(), None);
        op.record_locations(record.clone(), |_| false).unwrap();
        assert_eq!(op.locations(), Some(&record));
        op.start(t(10)).unwrap();
        op.succeed(t(20)).unwrap();
        assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);
        assert_eq!(op.warnings().len(), 1);
        assert!(op.record_locations(record, |_| false).is_err());
    }

    #[test]
    fn a_failed_operation_keeps_its_reason_and_warnings() {
        let mut op = running();
        op.record_warning(unreadable("/a")).unwrap();
        op.fail(FailureReason::EngineExited { exit_code: Some(1) }, t(15))
            .unwrap();
        assert_eq!(op.status(), &failed());
        assert_eq!(op.warnings().len(), 1);
        assert_eq!(op.duration(), Some(SignedDuration::from_secs(5)));
    }

    #[test]
    fn a_queued_operation_can_fail_before_it_starts() {
        let mut op = queued();
        op.fail(
            FailureReason::EngineFailed {
                detail: "worker not found".to_owned(),
            },
            t(1),
        )
        .unwrap();
        assert!(op.status().is_finished());
        assert_eq!(op.finished_at(), Some(t(1)));
        assert_eq!(op.duration(), None);
    }

    #[test]
    fn queued_and_running_operations_can_be_cancelled() {
        for mut op in [queued(), running()] {
            op.cancel(t(30)).unwrap();
            assert_eq!(op.status(), &OperationStatus::Cancelled);
            assert_eq!(op.finished_at(), Some(t(30)));
        }
    }

    #[test]
    fn only_a_queued_operation_can_start() {
        let mut op = running();
        let err = op.start(t(11)).unwrap_err();
        assert_eq!(err.transition, Transition::Start);
        assert_eq!(err.status, OperationStatus::Running);
        assert_eq!(op.started_at(), Some(t(10)), "the first start is kept");
    }

    #[test]
    fn a_queued_operation_can_neither_succeed_nor_record_warnings() {
        let mut op = queued();
        assert_eq!(
            op.succeed(t(1)).unwrap_err().transition,
            Transition::Succeed
        );
        assert_eq!(
            op.record_warning(unreadable("/a")).unwrap_err().transition,
            Transition::RecordWarning
        );
        assert_eq!(op, queued(), "refused transitions change nothing");
    }

    #[test]
    fn a_finished_operation_refuses_every_transition() {
        let mut succeeded = running();
        succeeded.succeed(t(20)).unwrap();
        let mut with_warnings = running();
        with_warnings.record_warning(unreadable("/a")).unwrap();
        with_warnings.succeed(t(20)).unwrap();
        let mut failed = running();
        failed
            .fail(FailureReason::EngineExited { exit_code: None }, t(20))
            .unwrap();
        let mut cancelled = running();
        cancelled.cancel(t(20)).unwrap();

        for finished in [succeeded, with_warnings, failed, cancelled] {
            assert!(finished.status().is_finished());
            let mut op = finished.clone();
            assert!(op.start(t(40)).is_err());
            assert!(op.record_warning(unreadable("/z")).is_err());
            assert!(op.succeed(t(40)).is_err());
            assert!(
                op.fail(FailureReason::EngineExited { exit_code: None }, t(40))
                    .is_err()
            );
            assert!(op.cancel(t(40)).is_err());
            assert_eq!(op, finished, "refused transitions change nothing");
        }
    }

    #[test]
    fn queued_and_running_are_not_finished() {
        assert!(!OperationStatus::Queued.is_finished());
        assert!(!OperationStatus::Running.is_finished());
    }

    #[test]
    fn prune_and_check_are_maintenance() {
        assert!(OperationKind::Prune.is_maintenance());
        assert!(OperationKind::Check.is_maintenance());
    }

    #[test]
    fn backups_repository_identification_and_diagnostic_jobs_are_not_maintenance() {
        for kind in [
            OperationKind::Backup,
            OperationKind::Identify,
            OperationKind::Fake,
        ] {
            assert!(!kind.is_maintenance(), "{kind:?}");
        }
    }

    fn hook_warning(detail: &str) -> Warning {
        Warning {
            kind: WarningKind::Hook,
            item: None,
            during: "before".to_owned(),
            detail: detail.to_owned(),
        }
    }

    #[test]
    fn a_hook_failure_codes_as_hook_failed() {
        assert_eq!(
            FailureReason::HookFailed { exit_code: Some(3) }.code(),
            "hook-failed"
        );
        assert_eq!(
            FailureReason::HookFailed { exit_code: None }.code(),
            "hook-failed"
        );
        assert_eq!(FailureReason::RepositoryExists.code(), "repository-exists");
    }

    #[test]
    fn a_daemon_that_stopped_codes_as_daemon_stopped() {
        assert_eq!(FailureReason::DaemonStopped.code(), "daemon-stopped");
    }

    #[test]
    fn a_job_with_too_much_to_mask_codes_as_secrets_too_large_to_mask() {
        let reason = FailureReason::SecretsTooLargeToMask {
            field: Some("HTTPS_PROXY".to_owned()),
            limit: 262_144,
        };
        assert_eq!(reason.code(), "secrets-too-large-to-mask");
    }

    #[test]
    fn a_replaced_repository_codes_as_repository_replaced() {
        assert_eq!(
            FailureReason::RepositoryReplaced.code(),
            "repository-replaced"
        );
    }

    #[test]
    fn a_refusal_of_the_credentials_codes_as_credentials_refused() {
        assert_eq!(
            FailureReason::CredentialsRefused.code(),
            "credentials-refused"
        );
    }

    #[test]
    fn only_a_failure_for_no_repository_at_the_location_found_none() {
        let failed = |reason| OperationStatus::Failed { reason };
        assert!(failed(FailureReason::RepositoryNotFound).found_no_repository());
        for status in [
            OperationStatus::Queued,
            OperationStatus::Running,
            OperationStatus::Succeeded,
            OperationStatus::SucceededWithWarnings,
            OperationStatus::Cancelled,
            failed(FailureReason::RepositoryExists),
            failed(FailureReason::DaemonStopped),
        ] {
            assert!(!status.found_no_repository(), "{status:?}");
        }
    }

    #[test]
    fn no_repository_at_the_location_codes_as_repository_not_found() {
        assert_eq!(
            FailureReason::RepositoryNotFound.code(),
            "repository-not-found"
        );
    }

    #[test]
    fn an_attached_warning_promotes_only_a_bare_success() {
        let mut op = running();
        op.succeed(t(20)).unwrap();
        op.attach_warning(hook_warning("3")).unwrap();
        assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);

        let mut op = running();
        op.fail(FailureReason::EngineExited { exit_code: Some(1) }, t(15))
            .unwrap();
        op.attach_warning(hook_warning("timeout")).unwrap();
        assert_eq!(op.status(), &failed());
        assert_eq!(op.warnings().len(), 1);
    }

    #[test]
    fn a_cancelled_operation_takes_no_attached_warning() {
        let mut op = running();
        op.cancel(t(20)).unwrap();
        assert!(op.attach_warning(hook_warning("3")).is_err());
    }

    /// A run refused before anything ran is queued and failed in the same
    /// instant, with the refusal as its reason, and never started: no time
    /// of the run is the start of anything that ran (§5).
    #[test]
    fn a_refused_run_is_queued_and_failed_in_one_instant_and_never_started() {
        let reason = FailureReason::SecretRefused {
            field: "password".to_owned(),
            refusal: refusal::SettingRefusal::SecretMissing {
                source: "ladderback/demo".to_owned(),
            },
        };
        assert_eq!(reason.code(), "secret-missing");
        let target = Target {
            plan: Some(PlanId::parse("home").unwrap()),
            repository: None,
        };
        let op = Operation::refused(
            OperationId::from_uuid(Uuid::from_u128(7)),
            OperationKind::Backup,
            Trigger::Schedule,
            target.clone(),
            reason.clone(),
            t(30),
        );
        assert_eq!(op.status(), &OperationStatus::Failed { reason });
        assert_eq!(op.started_at(), None);
        assert_eq!(op.queued_at(), t(30));
        assert_eq!(op.finished_at(), Some(t(30)));
        assert_eq!(op.trigger(), Trigger::Schedule);
        assert_eq!(op.kind(), OperationKind::Backup);
        assert_eq!(op.target(), &target);
    }
}
