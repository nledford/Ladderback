//! Wire types shared by the daemon, the hub, the worker and the web UI: the
//! published language (technical design §3.1). Builds for `wasm32`.
//!
//! Errors and warnings cross every boundary as codes with typed arguments,
//! never as sentences; the presentation layer renders them (§8.4).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use serde::{Deserialize, Serialize};

mod codes;
mod interop;
mod snapshots;
pub mod sync;
#[cfg(test)]
mod tests;

pub use codes::*;
pub use interop::*;
pub use snapshots::*;

/// The step a running operation is in (technical design §3.1, §4.4; §16,
/// decision 82), on the wire as its kebab-case name: `reading-index`. Closed
/// like [`WarningCode`]: a page that reads a code it doesn't know has
/// `Unknown`, which it shows as it shows no step. The daemon never sends
/// `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStep {
    ReadingIndex,
    GettingSnapshots,
    BackingUp,
    ListingPacks,
    CheckingCache,
    CheckingHotRepository,
    CheckingTrees,
    ReadingPackData,
    FindingUsedData,
    MarkingUnusedPacks,
    Repacking,
    RebuildingIndex,
    DeletingUnusedData,
    DeletingSnapshots,
    SavingSnapshots,
    #[serde(other)]
    Unknown,
}

/// What a progress bar counts (technical design §4.4; §16, decision 83), on
/// the wire as its kebab-case name: bytes, which a page shows as sizes, or
/// things such as files or packs, which it shows as plain numbers. Closed
/// like [`OperationStep`]: a page that reads a unit it doesn't know has
/// `Unknown`, which it shows as a plain number. The daemon never sends
/// `Unknown`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressUnit {
    #[default]
    Count,
    Bytes,
    #[serde(other)]
    Unknown,
}

/// One live update the daemon pushes to a connected page over the events
/// WebSocket (technical design §4.4): what an operation reported, or that an
/// agent came or went. Identifiers are text, counts are numbers, and times
/// are RFC 3339 strings. The daemon forwards an operation's progress and
/// warning events unchanged: coalescing is the worker's, once (§4.1). How an
/// operation started and how it ended are announced by the run that recorded
/// them, once the history has each (§4.4): a page that reads the history on
/// hearing either finds it there. A read that is no operation tells nothing:
/// no start, end, progress or warning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UiEvent {
    /// An operation left the queue and is running, and the history says so.
    /// `started_at` is the history's start time.
    OperationStarted {
        operation: String,
        repository: Option<String>,
        started_at: String,
    },
    /// A running operation moved forward: in `step`, `current` of `total`,
    /// when it says, counted in `unit`. What a page is shown is one progress
    /// bar, the one the engine began last and still has open (§4.4), so its
    /// numbers are that step's. `step` is `null` when the engine names no
    /// step this build knows, and is left out by a daemon older than the
    /// field; `unit` is left out by a daemon older than its field, and then
    /// reads as a count. Progress is transient: it is never stored, and a
    /// subscriber whose buffer fills loses the oldest of these first.
    OperationProgress {
        operation: String,
        #[serde(default)]
        step: Option<OperationStep>,
        #[serde(default)]
        unit: ProgressUnit,
        current: u64,
        total: Option<u64>,
    },
    /// A problem with one item that doesn't fail the operation.
    OperationWarning {
        operation: String,
        code: WarningCode,
        args: Vec<MsgArg>,
    },
    /// The operation ended well. The run that recorded it announces this once,
    /// after its ending hooks and its last write to the operation log, so a
    /// page that reads the history on hearing it finds the end there, unless
    /// the log lost that write. A cancelled operation has no such event.
    OperationSucceeded {
        operation: String,
        finished_at: String,
    },
    /// The operation ended badly: announced like [`Self::OperationSucceeded`],
    /// for a failure the run found out itself as well as the engine's own.
    /// `args` are the failure's arguments by position, then the engine's own
    /// words ([`ErrorDto::positional`]).
    OperationFailed {
        operation: String,
        code: ErrorCode,
        args: Vec<MsgArg>,
        finished_at: String,
    },
    /// A plan's backup failed, and the agent retries it (`backup.retrying`,
    /// §5, §11): `operation` is the run that failed, `attempt` which of the
    /// plan's `attempts` retries this is, and `at` when it runs. Told once,
    /// when the retry is planned.
    BackupRetrying {
        operation: String,
        plan: String,
        attempt: u32,
        attempts: u32,
        at: String,
    },
    /// An agent joined the fleet.
    AgentOnline { agent: String, at: String },
    /// An agent left the fleet.
    AgentOffline { agent: String, at: String },
    /// A proposal's status changed (decision 89, point 14): facts only,
    /// never its rationale, its changes or its proposer's label. A page
    /// that shows proposals reads them again on hearing it.
    ProposalChanged {
        proposal: String,
        status: ProposalStatusDto,
    },
}

/// One operation of the history page, as the daemon tells the web UI: what
/// a list view needs — ids, the kind and status, the lifecycle times, and
/// the typed outcome. Times are RFC 3339, UTC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct OperationSummaryDto {
    /// A UUID, hyphenated.
    pub id: String,
    /// The plan the operation belongs to.
    pub plan: String,
    /// `backup`, `prune`, `check`, …
    pub kind: String,
    /// `queued`, `running`, `succeeded`, `succeeded-with-warnings`,
    /// `failed`, `cancelled`.
    pub status: String,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// The codes of the operation's warnings, each code once, from the kept
    /// warnings (the first ten thousand), in the order the first of each
    /// was recorded (#508). The operation's detail lists the warnings.
    pub warning_codes: Vec<WarningCode>,
    /// Set when the operation failed.
    pub failure_code: Option<ErrorCode>,
    /// Why the operation failed, set when it did: the failure's code, the
    /// arguments its message takes, and the engine's own words, as the
    /// engine masked them of every secret of the job (I4). Added beside
    /// `failure-code`, which keeps its place in the published shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ErrorDto>,
    /// What the engine reported when the operation succeeded.
    pub result: Option<OperationResultDto>,
    /// The alias of the repository a maintenance operation ran on, or a
    /// restore or a download read, as it stood when the log first wrote the
    /// run. Absent for a backup, which `plan` names, and for an operation an
    /// earlier version logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// For a repository reached at several locations: every location as the
    /// run's choice saw it, in order, so a page says "Used location 2 of 2"
    /// and why each earlier one was skipped (decision 80). Left out for a
    /// repository of one location and an operation that opens none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<LocationTriedDto>,
    /// The entry of a snapshot a restore or a download read, as the log
    /// kept it: its path from the snapshot's top, `/` first, with every
    /// secret of the repository masked and a path past 1 KiB cut to its end
    /// after `…` (#847). Left out for every other operation and for one an
    /// earlier version logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

impl OperationSummaryDto {
    /// The same summary, with the locations the run's choice saw.
    #[must_use]
    pub fn with_locations(self, locations: Vec<LocationTriedDto>) -> Self {
        Self { locations, ..self }
    }

    /// The same summary, naming the entry of a snapshot a restore or a
    /// download read, as the log kept it (#847).
    #[must_use]
    pub fn with_entry(self, entry: String) -> Self {
        Self {
            entry: Some(entry),
            ..self
        }
    }

    /// The location the run used, and how many the repository had then,
    /// when it had several.
    pub fn used_location(&self) -> Option<(&LocationTriedDto, usize)> {
        self.locations
            .iter()
            .find(|tried| tried.outcome == "used")
            .map(|tried| (tried, self.locations.len()))
    }
}

/// Why an operation DTO can't be: an ID that isn't a UUID, or a time that
/// isn't RFC 3339.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidOperation {
    Id,
    Time,
}

impl std::fmt::Display for InvalidOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvalidOperation {}

impl OperationSummaryDto {
    /// Builds the summary, refusing an ID that isn't a hyphenated UUID and
    /// a time that isn't RFC 3339, like [`SnapshotDto::new`]. The arity is
    /// the wire's: one argument per field of the published shape, except
    /// `failure-code`, which is the code of `failure`.
    #[expect(
        clippy::too_many_arguments,
        reason = "the arity is the wire's: one argument per field of the published shape"
    )]
    pub fn new(
        id: String,
        plan: String,
        kind: String,
        status: String,
        queued_at: String,
        started_at: Option<String>,
        finished_at: Option<String>,
        warning_codes: Vec<WarningCode>,
        failure: Option<ErrorDto>,
        result: Option<OperationResultDto>,
        repository: Option<String>,
    ) -> Result<Self, InvalidOperation> {
        if !is_uuid(&id) {
            return Err(InvalidOperation::Id);
        }
        for time in std::iter::once(queued_at.as_str())
            .chain(started_at.as_deref())
            .chain(finished_at.as_deref())
        {
            if time.parse::<jiff::Timestamp>().is_err() {
                return Err(InvalidOperation::Time);
            }
        }
        Ok(Self {
            id,
            plan,
            kind,
            status,
            queued_at,
            started_at,
            finished_at,
            warning_codes,
            failure_code: failure.as_ref().map(|failure| failure.code),
            failure,
            result,
            repository,
            locations: Vec::new(),
            entry: None,
        })
    }
}

/// A hyphenated UUID: hex digits, with hyphens where a UUID has them.
fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// One page of the operation history: newest first, and `next` set when
/// older operations remain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct OperationsPageDto {
    pub operations: Vec<OperationSummaryDto>,
    pub next: Option<String>,
}

/// One plan of the plans list, as the daemon tells the web UI: the ID a
/// start request names, the repository its backups go to, and when it runs
/// on its own (§5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct PlanSummaryDto {
    pub id: String,
    pub repository: String,
    /// Whether the plan has a schedule. A plan without one runs only when
    /// it is started.
    pub scheduled: bool,
    /// The scheduler's planned run of a scheduled plan, RFC 3339 in UTC;
    /// `None` while the scheduler has planned none (it hasn't yet, or the
    /// schedule has no run left).
    pub next_run: Option<String>,
    /// What the configuration holds that the user should know about this
    /// plan: one `glob-never-matches` per glob that can never match. Left
    /// out when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
}

/// The plans list: what the operations page offers to start.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlansDto {
    pub plans: Vec<PlanSummaryDto>,
}

/// One plan as the Plans page edits it: the plan's table in the
/// configuration file's keys, as a `[[plans]]` entry writes it, each default
/// left out (decision 96). The daemon reads it as the file is read when it
/// loads, so the wire holds no second schema of a plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct PlanDto(pub serde_json::Map<String, serde_json::Value>);

/// What is wrong with a plan's setting a [`ErrorCode::PlanInvalid`] names,
/// sent as its `problem` argument by its wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanProblem {
    /// A setting the plan needs isn't there.
    Missing,
    /// A setting a plan doesn't have.
    UnknownKey,
    /// A value of another type than the setting takes: a number where it
    /// wants text, say.
    WrongType,
    /// A value the setting doesn't take: a schedule that is not one, a tag
    /// Ladderback keeps for its own, an ID that is not a name.
    Invalid,
    /// The plan's ID is another plan's.
    Taken,
    /// The plan's repository is not one the configuration has.
    UnknownRepository,
    /// The plan backs up nothing: it has no source.
    NoSources,
}

impl PlanProblem {
    /// The name the `problem` argument carries, which the message selects
    /// on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::UnknownKey => "unknown-key",
            Self::WrongType => "wrong-type",
            Self::Invalid => "invalid",
            Self::Taken => "taken",
            Self::UnknownRepository => "unknown-repository",
            Self::NoSources => "no-sources",
        }
    }
}

/// What is wrong with a proposal's change a [`ErrorCode::ProposalInvalid`]
/// names, sent as its `problem` argument by its wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeProblem {
    /// The change's plan isn't one the configuration file would load.
    Plan(PlanProblem),
    /// The change names a plan the configuration doesn't have.
    UnknownPlan,
    /// The change removes a plan a notification rule names.
    InUse,
    /// The change gives a plan hooks: a new plan any, a changed plan others
    /// than it has. Hooks come with approval (decision 89).
    Hooks,
}

impl ChangeProblem {
    /// The name the `problem` argument carries, which the message selects
    /// on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan(problem) => problem.as_str(),
            Self::UnknownPlan => "unknown-plan",
            Self::InUse => "in-use",
            Self::Hooks => "hooks",
        }
    }
}

/// One configuration change of a proposal (decision 89), by its `kind`. A
/// plan travels as its table in the configuration file's keys, as the plan
/// routes take it ([`PlanDto`]); a removal names the plan by its ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProposalChangeDto {
    /// Adds the plan after the configuration's last one.
    AddPlan { plan: PlanDto },
    /// Puts the plan in place of the plan with its ID.
    ChangePlan { plan: PlanDto },
    /// Removes the plan with this ID; its snapshots stay.
    RemovePlan { plan: String },
}

/// `POST /api/v1/proposals`: the changes proposed, in the order they apply,
/// and why, in the proposer's own words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct NewProposalDto {
    pub changes: Vec<ProposalChangeDto>,
    #[serde(default)]
    pub rationale: String,
}

/// `POST /api/v1/dry-runs`: the draft plan to walk, its table in the
/// configuration file's keys, as [`PlanDto`] holds one (§16, decision 89,
/// point 13).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DryRunPlanDto {
    pub plan: PlanDto,
}

/// What a plan dry run counted: counts and sizes only, never a path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct PlanDryRunDto {
    /// The files the plan would back up.
    pub files: u64,
    /// Their size in bytes.
    pub bytes: u64,
    /// The entries the walk couldn't read.
    pub unreadable: u64,
    /// What each exclude removes on its own, in the order of the plan's
    /// settings; those the dry run didn't reach before it stopped are left
    /// out.
    #[serde(default)]
    pub excludes: Vec<ExcludeTallyDto>,
    /// Why the dry run stopped before the end, if it did: its counts are
    /// then what it read until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<DryRunStopDto>,
}

/// What one exclude of a plan's source filter removes on its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ExcludeTallyDto {
    /// The plan's setting, as the configuration file names it
    /// (`globs`, `exclude-if-present`, `git-ignore`).
    pub setting: String,
    /// The setting's entry, from 1, for a setting that is a list; none for
    /// a switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<u64>,
    /// The files it removes, and their bytes.
    pub files: u64,
    pub bytes: u64,
}

/// Why a plan dry run stopped before the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DryRunStopDto {
    /// A walk read as many entries as one may.
    Entries,
    /// Its time ran out.
    Time,
    /// A backup took a place in the queue, and it gave way.
    GaveWay,
}

/// `GET` and `PUT /api/v1/settings/dry-run-allowlist`: the folders a plan
/// dry run may read, a person's setting only (§16, decision 89, point 13).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DryRunAllowlistDto {
    /// The folders, absolute paths, as the configuration file holds them.
    pub folders: Vec<String>,
    /// The daemon's home folder, which "Add home folder" adds; sent by the
    /// daemon only, and none when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
}

/// Where a proposal is in its life (decision 89).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ProposalStatusDto {
    Pending,
    Applying,
    Applied,
    Failed,
    Rejected,
    Withdrawn,
    Expired,
    Stale,
}

/// One proposal as the proposal routes answer it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ProposalDto {
    pub id: String,
    /// The label of the API token that proposed it, as it was then; `null`
    /// for a client with no token.
    #[serde(default)]
    pub proposed_by: Option<String>,
    pub status: ProposalStatusDto,
    pub created_at: String,
    /// 48 hours after `created-at`.
    pub expires_at: String,
    pub changes: Vec<ProposalChangeDto>,
    pub rationale: String,
    /// When a person approved or rejected it; `null` until one did.
    #[serde(default)]
    pub decided_at: Option<String>,
    /// A person's reason for rejecting each change, in order, `""` for a
    /// change given none; empty unless it was rejected.
    #[serde(default)]
    pub reasons: Vec<String>,
    /// What the submission checks warn of, in the answer to its creation
    /// only: the proposal was kept all the same.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ProposalWarningDto>,
}

/// What the submission checks found that doesn't refuse a proposal
/// (decision 89, point 5), by its `kind`. `change` counts from 1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProposalWarningDto {
    /// The change leaves its plan `plan` backing up the same sources as
    /// the plan `other`.
    SameSources {
        change: u64,
        plan: String,
        other: String,
    },
    /// The change leaves its plan with the glob `glob`, which can never
    /// match, read against its sources as written.
    GlobNeverMatches { change: u64, glob: String },
}

/// `GET /api/v1/proposals`: the proposals the asker may see, newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProposalsDto {
    pub proposals: Vec<ProposalDto>,
}

/// A plan's setting, as a summary line names what a change changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanSettingDto {
    Repository,
    Host,
    Label,
    Sources,
    Tags,
    Schedule,
    Overlap,
    Retry,
    Filter,
    Retention,
}

/// One line of the summary Ladderback writes of a proposal's changes
/// (decision 89, point 4), by its `kind`; never the AI agent's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SummaryLineDto {
    AddsPlan {
        plan: String,
    },
    /// Changes these settings of the plan.
    ChangesPlan {
        plan: String,
        settings: Vec<PlanSettingDto>,
    },
    RemovesPlan {
        plan: String,
    },
}

/// `GET /api/v1/proposals/{id}/review`, a browser session's only: the
/// proposal as the "Pending changes" page shows it (decision 89, point 6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProposalReviewDto {
    pub proposal: ProposalDto,
    /// What its changes do, one line each, in order.
    pub summary: Vec<SummaryLineDto>,
    /// The unified diff of `config.toml` an approval applies, each secret
    /// the file holds masked; empty unless the proposal is pending.
    pub diff: String,
    /// The SHA-256, in hex, of the file the diff was rebuilt against, which
    /// an approval sends back.
    pub revision: String,
    /// The SHA-256, in hex, of `diff`, which an approval sends back.
    pub diff_hash: String,
    /// Whether the file has changed since the proposal was drafted.
    pub changed_since_drafted: bool,
    /// Whether a change of it could delete snapshots: such a proposal is
    /// approved only with the forget preview, so until then it is only
    /// rejected (decision 89, point 8).
    pub needs_forget_preview: bool,
    /// How many of its proposer's kept proposals a person rejected.
    pub rejected: u64,
    /// The ID's first characters, which confirm an approval while sign-in
    /// is off.
    pub short_id: String,
}

/// `POST /api/v1/proposals/{id}/forget-preview`, a browser session's only:
/// what the pending proposal would change about the snapshots retention
/// forgets, for each repository its changes touch, in the order they name
/// them (decision 89, point 8; I6). Computed when the person asks: it reads
/// each repository as a forget's preview does, and changes nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProposalForgetPreviewDto {
    pub repositories: Vec<RepositoryForgetDto>,
    /// What an approval of the proposal brings to say this preview is the
    /// one the person saw; none while a repository couldn't be read, so
    /// the page offers no approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// What a proposal would change about what retention forgets in one
/// repository, or why the repository couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum RepositoryForgetDto {
    /// What a forget of this agent's would do under the configuration as it
    /// is (`before`) and as the proposal makes it (`after`).
    Compared {
        repository: String,
        before: ForgetSideDto,
        after: ForgetSideDto,
        /// What `after` removes and `before` doesn't, per plan that removes
        /// any such: `remove` counts them and `listed` names them.
        forgets_more: Vec<ForgetPlanDto>,
        /// What `before` removes and `after` doesn't, per plan, the same.
        forgets_less: Vec<ForgetPlanDto>,
    },
    /// The repository couldn't be read: why, as the maintenance routes tell
    /// it.
    Refused { repository: String, error: ErrorDto },
}

/// What a forget of this agent's would do to a repository under one
/// configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum ForgetSideDto {
    /// This agent doesn't maintain the repository: no forget of its runs
    /// there, so its retention forgets nothing.
    NotMaintained,
    /// What it would remove: each plan with history there, as a forget's
    /// preview tells it.
    Forgets { plans: Vec<ForgetPlanDto> },
    /// It would be refused, and so remove nothing: the policy of `plan`,
    /// which has history there, keeps nothing.
    KeepsNothing { plan: String },
}

/// `POST /api/v1/proposals/{id}/approve`, a browser session's only: what
/// the page showed, from [`ProposalReviewDto`], and the step-up
/// confirmation: the admin password with sign-in on, the proposal's short
/// ID otherwise. The confirmation is never logged or shown (I4): its
/// `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ApproveProposalDto {
    pub revision: String,
    pub diff_hash: String,
    /// The digest of the forget preview the page showed, from
    /// [`ProposalForgetPreviewDto`]; a proposal that could delete
    /// snapshots is approved only with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forget_digest: Option<String>,
    pub confirmation: String,
}

impl std::fmt::Debug for ApproveProposalDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApproveProposalDto")
            .field("revision", &self.revision)
            .field("diff_hash", &self.diff_hash)
            .field("forget_digest", &self.forget_digest)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// `POST /api/v1/proposals/{id}/reject`, a browser session's only: a
/// reason for each change, in order, `""` for none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RejectProposalDto {
    pub reasons: Vec<String>,
}

/// What is wrong with an argument an [`ErrorCode::InvalidArguments`] names,
/// sent as its `problem` argument by its wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArgumentProblem {
    /// An argument the tool needs isn't there.
    Missing,
    /// An argument the tool doesn't take.
    UnknownKey,
    /// A value the argument doesn't take: one of another type, or a number
    /// out of its range.
    Invalid,
}

impl ArgumentProblem {
    /// The name the `problem` argument carries, which the message selects
    /// on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::UnknownKey => "unknown-key",
            Self::Invalid => "invalid",
        }
    }
}

/// The repositories list: what the Repositories page shows and the snapshots
/// page offers to pick from, in the order of their aliases.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoriesDto {
    pub repositories: Vec<RepositoryDto>,
}

/// What the Repositories page notes above its list, of the machine rather
/// than of one repository: `keyring-not-activatable` while the keyring runs
/// but can't be started on demand (#923). Left empty when there is nothing
/// to note.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryNotesDto {
    pub notes: Vec<WarningDto>,
}

/// One configured repository, as a list shows it: where it is, with any
/// password in the location masked, and where its password comes from.
/// Never the password itself (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryDto {
    pub alias: String,
    /// The location, with the password of a URL in it masked.
    pub repository: String,
    /// The hot half of a hot/cold repository, masked the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    pub password: PasswordSourceDto,
    /// Whether this agent maintains the repository: forgets, prunes and
    /// checks it (I1, §7.3). A row that doesn't say is maintained elsewhere,
    /// so a client offers no maintenance for it.
    #[serde(default)]
    pub maintained_here: bool,
    /// Whether the row offers to create the repository where its entry says:
    /// its entry records no repository ID, its latest operation failed with
    /// `repository-not-found`, and none of them ever succeeded, so never
    /// while none is on record (decision 53). Left out when it doesn't.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub create_offered: bool,
    /// What the configuration holds that the user should know about this
    /// repository: one per secret written inline, and one
    /// `location-plain-http-credentials` per location that sends its
    /// credentials where an impostor could read them (decision 80). Left
    /// out when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
    /// Every location of a repository reached at several, in the order
    /// they are tried, the first first (decision 80). Left out for a
    /// repository of one location, which `repository` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<LocationDto>,
}

/// One location of a repository reached at several, as the Repositories
/// page lists it: its address with the credentials written in it masked,
/// and its options by name. Never a secret (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationDto {
    /// Its place in the order, from 1.
    pub position: u8,
    /// `repository` as the entry writes it, masked.
    pub repository: String,
    /// `repo-hot`, masked the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    /// Its options, set by set, each by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<LocationOptionDto>,
    /// Its rule's alternatives, in the order written, of which any one
    /// holding lets it be tried. Left out for a location with no rule,
    /// which is tried on every network.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<LocationAlternativeDto>,
}

/// One alternative of a location's rule (decision 80).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationAlternativeDto {
    /// The kind of network, as the configuration writes it: `wired` or
    /// `wifi`.
    pub network: String,
    /// The Wi-Fi networks, by name, it holds on alone (`only`); empty when
    /// it lists none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
    /// The Wi-Fi networks, by name, it doesn't hold on (`except`); empty
    /// when it lists none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<String>,
}

/// One option of a location: its name, and its value only when the value
/// is references alone (`${LAN_KEY}`), which say where a secret is and
/// never hold it. A value written in the configuration is never shown: it
/// may be the secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationOptionDto {
    /// `options`, `options-hot` or `options-cold`, as the configuration
    /// names the set.
    pub set: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// What became of one location of a repository reached at several: in an
/// operation, as the run's choice saw it; in a Test, as its probe found it
/// (decision 80). Named by its position, never a network's name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationTriedDto {
    /// Its place in the order, from 1.
    pub position: u8,
    /// Its address as the entry wrote it then, credentials masked, at most
    /// 256 bytes.
    pub address: String,
    /// `used` (in a Test: it answers and holds the repository),
    /// `not-tried`, `rule-not-met`, `rule-unknown`, `silent`, `refused`,
    /// `not-a-repository`, `other-repository`, `password-not-accepted`,
    /// `secret-unavailable`, or the refusal code of a setting that can't be
    /// used (`template-invalid`, `repository-path-relative`,
    /// `secret-expansion-too-long`, `secret-in-process-arguments`,
    /// `location-too-many-credentials`).
    pub outcome: String,
    /// For a location whose settings didn't resolve: where the secret was
    /// looked for (`env LAN_KEY`), or the setting that can't be used. Never
    /// a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The answer to a Test of a saved repository's locations
/// (`POST /api/v1/repositories/{alias}/test`): each location's outcome, in
/// order, none `not-tried`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationsTestDto {
    pub locations: Vec<LocationTriedDto>,
}

/// Where a configured repository's password comes from, without its value:
/// all five sources the configuration takes, the command by its program
/// only, since its arguments may hold the secret itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum PasswordSourceDto {
    /// The configuration file holds the password.
    Inline,
    /// A file holds it.
    File { path: String },
    /// An entry of the OS keyring, as `service/name`.
    Keyring { entry: String },
    /// An environment variable of the daemon.
    Env { name: String },
    /// The output of a program.
    Command { program: String },
}

/// Where a request says the repository's password is: the four sources the
/// API takes. A `command` source is not one of them: it runs a program, and
/// the first route that may run one is the one behind authentication
/// (technical design §10.1). Its `Debug` form never shows a typed password.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PasswordDto {
    /// The password itself, as the person typed it.
    Inline { value: String },
    /// A file that holds it, by its absolute path.
    File { path: String },
    /// An entry of the OS keyring, as `service/name`.
    Keyring { entry: String },
    /// An environment variable of the daemon that holds it.
    Env { name: String },
}

impl std::fmt::Debug for PasswordDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline { .. } => f.write_str("Inline { value: *** }"),
            Self::File { path } => f.debug_struct("File").field("path", path).finish(),
            Self::Keyring { entry } => f.debug_struct("Keyring").field("entry", entry).finish(),
            Self::Env { name } => f.debug_struct("Env").field("name", name).finish(),
        }
    }
}

/// What a request says about one repository: where it is and how to open it.
/// The field names are the configuration's own, so a repository written in
/// the file and one typed into the page mean the same. Its `Debug` form shows
/// no text a person could have put a secret in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RepositorySettingsDto {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    pub password: PasswordDto,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options_hot: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options_cold: std::collections::BTreeMap<String, String>,
}

impl std::fmt::Debug for RepositorySettingsDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A location can carry a password in a URL and an option can be a
        // credential, so only what is not text a person typed is shown.
        f.debug_struct("RepositorySettingsDto")
            .field("repository", &"***")
            .field("repo_hot", &self.repo_hot.as_ref().map(|_| "***"))
            .field("password", &self.password)
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .field("options_hot", &self.options_hot.keys().collect::<Vec<_>>())
            .field(
                "options_cold",
                &self.options_cold.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// `POST /api/v1/repositories/_test`: open the repository these settings
/// describe, and save nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TestRepositoryDto {
    pub settings: RepositorySettingsDto,
}

/// `POST /api/v1/repositories`: test the repository, and save it under
/// `alias` once the test passed. `maintain-here` asks this agent to maintain
/// it (`maintenance = "local"`, §7.3); left out or `false`, nobody is asked.
/// `keep-in-keyring` keeps a typed (inline) password in the system keyring
/// entry `ladderback/<alias>` and saves a reference to it (§10.2); left out
/// or `false`, the password is saved as it was sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConnectRepositoryDto {
    pub alias: String,
    pub settings: RepositorySettingsDto,
    #[serde(default)]
    pub maintain_here: bool,
    #[serde(default)]
    pub keep_in_keyring: bool,
}

/// `PUT /api/v1/repositories/{alias}/maintainer`: turn maintenance of a
/// saved repository from this agent on (`maintain-here: true`, its entry
/// then asks for `local`) or off (`none`), §7.3. Answers 204; turning it off
/// while a forget, prune, check or adoption of it runs here is refused with
/// `maintenance-busy` (I1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MaintainerDto {
    pub maintain_here: bool,
}

/// `POST /api/v1/repositories/_init`: create a repository where there is
/// none, and save it under `alias` when one is given. `keep-in-keyring`, as
/// for [`ConnectRepositoryDto`], needs an `alias`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct InitRepositoryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub settings: RepositorySettingsDto,
    #[serde(default)]
    pub keep_in_keyring: bool,
}

/// What testing or connecting a repository answers: its ID, as rustic
/// knows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryIdDto {
    pub id: String,
}

/// The header that marks a request the web UI sent on its own, such as a
/// re-read after an event or a poll, rather than one a person caused
/// (§10.1). With sign-in on, a marked request never uses its session: only
/// what a person does keeps one from ending unused. Its value is ignored.
/// It can only shorten a session's life, so the daemon trusts it as sent.
pub const BACKGROUND_HEADER: &str = "x-ladderback-background";

/// `GET /api/v1/session` and a sign-in's answer (§10.1): whether sign-in is
/// on, and whether the request's session is live. A live session's answer
/// carries the CSRF token its changed requests send as `x-csrf-token`, how
/// long the session lasts unused (`idle-seconds`, left out for a session
/// signed in with Keep me signed in on this device, which has no idle
/// limit), and how long it has left however it is used (`ends-in-seconds`,
/// left out when nothing but its idle limit, a sign-out or a new admin
/// password ends it): the web UI warns before the first ends it (components
/// §4.5). A signed-out answer says whether the sign-in form offers Keep me
/// signed in on this device (`keep-signed-in-offered`), which it doesn't
/// under Stay signed in for Until I close the browser. `exposure` is
/// `network` when the daemon listens beyond loopback, and left out on
/// loopback, as before it existed: with sign-in off, the web UI then warns
/// on every page. With sign-in off, `admin-password` says an admin password
/// `ladderback set-password` has kept since the daemon started, looked for as
/// the route is read: sign-in turns on only at the next start (decision 70),
/// and it is left out while none is kept. The build the daemon runs
/// (`version`, `commit`, `dirty`, `release`) is told to a page that may see
/// the pages, with sign-in off or a live session, and left out for a page
/// that has yet to sign in. A request sending a `read` API token is answered
/// `signed-out` with the build, without `keep-signed-in-offered` or a CSRF
/// token, whatever session cookie it also sends (decision 95). With the
/// build, and only with it, comes the `available-update` the agent's release
/// check found, when there is one (decision 107).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionDto {
    pub sign_in: SignInStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_in_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "ExposureDto::is_loopback")]
    pub exposure: ExposureDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_password: Option<AdminPasswordDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_signed_in_offered: Option<bool>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_update: Option<AvailableUpdateDto>,
}

/// A newer release of Ladderback than the one the daemon runs, which the
/// header shows (decision 107): its version, and the address of its notes,
/// built from the version, never from what the release feed answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AvailableUpdateDto {
    pub version: String,
    pub release_notes: String,
}

/// What `GET /api/v1/restart` answers, and `POST` once it has taken a
/// request (decision 94): whether Settings offers a restart, which it does
/// only when a service manager runs the daemon and starts it again once it
/// exits; whether a restart asked for once the running operations finish is
/// pending, neither come nor cancelled, whatever runs (it stays pending
/// until the daemon stops); and how many operations are running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestartDto {
    pub offered: bool,
    pub pending: bool,
    pub running: u32,
}

/// The body of `POST /api/v1/restart`: when the daemon restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestartRequestDto {
    pub when: RestartWhenDto,
}

/// When a restart happens: once no operation is running, starting no queued
/// one meanwhile (`finished`); or at once, stopping the running operations
/// in order, as any stop does (`now`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartWhenDto {
    Finished,
    Now,
}

/// What `GET /api/v1/full-disk-access` answers, and `POST …/check` once the
/// probe has run again (§16, decision 115): what the agent's Full Disk Access
/// probe found last, and whether this browser may ask the agent to open
/// System Settings' Full Disk Access pane, which it may only from the
/// agent's own Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FullDiskAccessDto {
    pub state: FullDiskAccessStateDto,
    pub opens_settings: bool,
}

/// What the Full Disk Access probe found: `not-applicable` where the agent
/// isn't on macOS, which has no such grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FullDiskAccessStateDto {
    Granted,
    Missing,
    Unknown,
    NotApplicable,
}

/// What `GET /api/v1/status` answers: the status summary (§3.1, §4.6). Each
/// plan, in the order the configuration writes them; the operations queued
/// or running among the newest 200, newest first; the repositories
/// that couldn't be read at their last attempt, in the order of their
/// aliases; and what the Full Disk Access probe found. Plans and
/// repositories are named by their IDs and aliases, never by an address or
/// anything else a secret could be in (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct StatusDto {
    pub plans: Vec<PlanStatusDto>,
    /// Queued or running, as each one's `status` says.
    pub running: Vec<OperationSummaryDto>,
    pub unreadable_repositories: Vec<UnreadableRepositoryDto>,
    pub full_disk_access: FullDiskAccessStateDto,
}

/// One plan in the status summary: the plans list's entry, its last
/// operation, its last successful backup, however long ago,
/// and whether it is overdue (§3.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct PlanStatusDto {
    #[serde(flatten)]
    pub plan: PlanSummaryDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<OperationSummaryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success: Option<LastSuccessDto>,
    pub overdue: bool,
}

/// A plan's last successful backup: its operation's ID, when it finished,
/// and how many whole seconds ago that was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LastSuccessDto {
    pub operation: String,
    pub finished_at: String,
    pub ago_seconds: u64,
}

/// A repository, by its alias, that couldn't be read at its last attempt,
/// and the operation that couldn't read it, its `failure` saying why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct UnreadableRepositoryDto {
    pub repository: String,
    pub operation: OperationSummaryDto,
}

/// The build a program came from (§3.1, decision 90): its version, the
/// commit it was built from (`null` when the build had no git to ask), whether
/// the tree had uncommitted changes, and whether the commit is the one its
/// version's release tag names. Separate fields, so no reader parses the
/// shown form ([`BuildDto::display`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct BuildDto {
    /// The workspace version, `0.3.0`.
    pub version: String,
    /// The full commit hash.
    pub commit: Option<String>,
    /// Whether tracked files differed from the commit.
    pub dirty: bool,
    /// Whether the commit is the one the tag `v<version>` names.
    pub release: bool,
}

/// How many of a commit's hex digits the shown form keeps, as `git`'s short
/// hash does.
const SHORT_COMMIT: usize = 7;

impl BuildDto {
    /// The build as people read it, a SemVer version with build metadata:
    /// `0.3.0` for a clean build of its release tag or when the commit is
    /// unknown; otherwise the version and the short commit, `0.3.0+3f937cf`,
    /// with `.dirty` after it when the tree had changes.
    pub fn display(&self) -> String {
        match &self.commit {
            Some(commit) if self.dirty || !self.release => {
                let short: String = commit.chars().take(SHORT_COMMIT).collect();
                let dirty = if self.dirty { ".dirty" } else { "" };
                format!("{}+{short}{dirty}", self.version)
            }
            _ => self.version.clone(),
        }
    }
}

/// The recent log (§3.1, #618): the newest lines of the daemon log, newest
/// first, as `GET /api/v1/log` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct RecentLogDto {
    pub lines: Vec<LogLineDto>,
}

/// One line of the daemon log, as the log wrote it, its secrets masked (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LogLineDto {
    /// When it was written: RFC 3339 in UTC, to the millisecond
    /// (`2026-11-15T09:00:01.250Z`), as the log writes it.
    pub at: String,
    pub level: LogLevelDto,
    /// The part of Ladderback, or the crate, that wrote it
    /// (`ladderback::scheduler`).
    pub source: String,
    pub message: String,
}

/// How much a line of the daemon log matters, most severe first, so that
/// `Error < Warn`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LogLevelDto {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Where a request stands with sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SignInStateDto {
    /// No admin password is set: there is nothing to sign in to.
    Off,
    /// Sign-in is on and the request carries no live session.
    SignedOut,
    /// The request's session is live.
    SignedIn,
}

/// With sign-in off, the admin password kept since the daemon started
/// (§10.1, decision 70), which only the next start turns sign-in on for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdminPasswordDto {
    /// A hash the daemon can check: a restart turns sign-in on.
    Set,
    /// A file the daemon can't read or check: a restart would refuse to start
    /// until `ladderback set-password` writes it anew.
    Unreadable,
}

/// Who can reach the daemon at the address it listens on (§10.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ExposureDto {
    /// This machine only: a loopback address.
    #[default]
    Loopback,
    /// The network: any other address.
    Network,
}

impl ExposureDto {
    /// Whether this is loopback, which the session's answer leaves out.
    pub fn is_loopback(&self) -> bool {
        *self == Self::Loopback
    }
}

/// `POST /api/v1/session`: the password, as typed, and whether to **Keep me
/// signed in on this device** (`keep-signed-in`, false when left out). The
/// password is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInDto {
    pub password: String,
    #[serde(default)]
    pub keep_signed_in: bool,
}

impl std::fmt::Debug for SignInDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SignInDto {{ password: [hidden], keep_signed_in: {} }}",
            self.keep_signed_in
        )
    }
}

/// `POST /api/v1/admin-password`: the new admin password, as typed, and, with
/// sign-in on, the current one, which a change needs (§10.1, decision 70).
/// With sign-in off the current one is not asked for, and not read. Never
/// logged or shown (I4): its `Debug` hides both.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SetAdminPasswordDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_password: Option<String>,
    pub new_password: String,
}

impl std::fmt::Debug for SetAdminPasswordDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = if self.current_password.is_some() {
            "Some([hidden])"
        } else {
            "None"
        };
        write!(
            f,
            "SetAdminPasswordDto {{ current_password: {current}, new_password: [hidden] }}"
        )
    }
}

/// The session length, as **Stay signed in for** names it (§10.1): how long
/// every session lasts after its sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionLengthDto {
    #[serde(rename = "until-browser-closes")]
    UntilBrowserCloses,
    #[serde(rename = "7-days")]
    SevenDays,
    #[serde(rename = "30-days")]
    ThirtyDays,
    #[serde(rename = "90-days")]
    NinetyDays,
    #[serde(rename = "1-year")]
    OneYear,
    #[serde(rename = "until-sign-out")]
    UntilSignOut,
}

/// `GET` and `PUT /api/v1/settings/sign-in`: the sign-in settings, which
/// only a session reads or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInSettingsDto {
    pub session_length: SessionLengthDto,
}

/// A scope of an API token (§10.1, decision 93), as the token routes name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiTokenScopeDto {
    Read,
    Run,
    Preview,
    Propose,
}

/// One API token as the token routes list it: who it is, what it may do,
/// when it was created and last used. Never its secret (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApiTokenDto {
    pub id: String,
    pub label: String,
    pub scopes: Vec<ApiTokenScopeDto>,
    pub created_at: String,
    /// `null` for a token never used.
    #[serde(default)]
    pub last_used_at: Option<String>,
}

/// `GET /api/v1/tokens`: the tokens kept, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApiTokensDto {
    pub tokens: Vec<ApiTokenDto>,
}

/// `POST /api/v1/tokens`: the label and scopes of the token to create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CreateApiTokenDto {
    pub label: String,
    pub scopes: Vec<ApiTokenScopeDto>,
}

/// What creating a token answers: the token as it is listed, and its whole
/// text, `ladderback_<id>_<secret>`, shown this once. Its `Debug` hides the
/// secret (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CreatedApiTokenDto {
    pub token: ApiTokenDto,
    pub secret: String,
}

impl std::fmt::Debug for CreatedApiTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedApiTokenDto")
            .field("token", &self.token)
            .field("secret", &"[hidden]")
            .finish()
    }
}

/// What initialising a repository answers: its ID, and whether it was saved
/// into the configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryInitDto {
    pub id: String,
    pub saved: bool,
}

/// One reading of a repository's stats (§16, decision 104): when it was taken
/// and its figures.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StatsReadingDto {
    /// RFC 3339, UTC: when the stats were read.
    pub taken_at: String,
    /// How many snapshots the repository holds.
    pub snapshots: u64,
    /// The sources the snapshots hold, added up, in bytes: a file that ten
    /// snapshots hold counts ten times.
    pub total_size: u64,
    /// What the repository's packs store for them, in bytes, after
    /// deduplication and compression.
    pub stored_size: u64,
    /// The share of the data's raw size compression saved, a whole percent
    /// rounded down; absent when the repository stores nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression_saved_percent: Option<u8>,
}

/// A repository's stats as last read and kept: `latest` is absent when none
/// have been read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryStatsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<StatsReadingDto>,
}

/// How many snapshots one group of [`SnapshotGroupsDto`] lists. An
/// input-derived size, capped (§13.5): a repository can hold thousands of
/// snapshots of one plan, and a page that lists them all is no use to the
/// person reading it. The `count` next to a capped list is the real total.
pub const MAX_GROUP_SNAPSHOTS: usize = 500;

/// How many snapshots one list of a [`MaintenancePreviewDto`] names: a
/// review is read in one sitting, so it names fewer than a group's page.
pub const MAX_PREVIEW_SNAPSHOTS: usize = 50;

/// One plan of this agent on a repository, and the snapshots it owns, newest
/// first, at most [`MAX_GROUP_SNAPSHOTS`] of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlanGroupDto {
    pub plan: String,
    /// How many snapshots the plan owns, listed or not.
    pub count: u64,
    pub snapshots: Vec<SnapshotDto>,
}

/// Snapshots a `ladderback-` tag owns that no plan of this agent does:
/// another agent's plan, or a plan this configuration no longer has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OrphanedGroupDto {
    /// The `ladderback-agent:` tag's text, `null` when the snapshots carry
    /// none, or two that disagree.
    pub agent: Option<String>,
    /// The `ladderback-plan:` tag's text, `null` when the snapshots carry
    /// none, or two that disagree.
    pub plan: Option<String>,
    pub count: u64,
    pub snapshots: Vec<SnapshotDto>,
    /// The plans of this repository the group can be adopted into: all of
    /// them, since which one takes the history is the person's choice.
    pub adopt_into: Vec<String>,
}

/// Untagged snapshots, grouped by host, label and paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct UnmanagedGroupDto {
    pub hostname: String,
    /// Empty when the snapshots carry no label.
    pub label: String,
    pub paths: Vec<String>,
    pub count: u64,
    pub snapshots: Vec<SnapshotDto>,
    /// The plans whose host, label and sources equal the group's: none when
    /// no plan would adopt it, one when it is clear which does, several when
    /// it is ambiguous.
    pub adopt_into: Vec<String>,
}

/// One repository's snapshots grouped by who owns them (technical design
/// §7.3): what the Snapshots page shows beside its maintenance buttons.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotGroupsDto {
    /// Every plan of this agent on the repository, in the configuration's
    /// order, the ones with no snapshots too.
    pub plans: Vec<PlanGroupDto>,
    pub orphaned: Vec<OrphanedGroupDto>,
    pub unmanaged: Vec<UnmanagedGroupDto>,
    /// What the page warns of the repository: the other hosts it holds
    /// snapshots from, when it is maintained here (§7.3). Left out when
    /// there is nothing to warn of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
}

/// How many of a list's snapshots are shown, and which: how many `snapshots`
/// there are in all, and the first `max` of them as `dto` shows them. Only
/// those are built, so a group of thousands costs no more than a page of it
/// (an input-derived size, capped, §13.5). A snapshot `dto` can't show is
/// left out of the list and still counted.
fn listed<T>(
    snapshots: &[T],
    max: usize,
    dto: impl FnMut(&T) -> Option<SnapshotDto>,
) -> (u64, Vec<SnapshotDto>) {
    (
        u64::try_from(snapshots.len()).unwrap_or(u64::MAX),
        snapshots.iter().take(max).filter_map(dto).collect(),
    )
}

impl PlanGroupDto {
    /// The group of `plan`, which owns `snapshots`, newest first: `count` is
    /// all of them and at most [`MAX_GROUP_SNAPSHOTS`] are listed. `dto`
    /// shows one snapshot, or `None` for one the wire can't carry.
    pub fn new<T>(
        plan: String,
        snapshots: &[T],
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, snapshots) = listed(snapshots, MAX_GROUP_SNAPSHOTS, dto);
        Self {
            plan,
            count,
            snapshots,
        }
    }
}

impl OrphanedGroupDto {
    /// The orphaned group of `agent` and `plan`, which holds `snapshots`,
    /// newest first, and may be adopted into `adopt_into`: `count` is all of
    /// the snapshots and at most [`MAX_GROUP_SNAPSHOTS`] are listed. `dto`
    /// shows one snapshot, or `None` for one the wire can't carry.
    pub fn new<T>(
        agent: Option<String>,
        plan: Option<String>,
        snapshots: &[T],
        adopt_into: Vec<String>,
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, snapshots) = listed(snapshots, MAX_GROUP_SNAPSHOTS, dto);
        Self {
            agent,
            plan,
            count,
            snapshots,
            adopt_into,
        }
    }
}

impl UnmanagedGroupDto {
    /// The unmanaged group of `hostname`, `label` and `paths`, which holds
    /// `snapshots`, newest first, and may be adopted into `adopt_into`:
    /// `count` is all of the snapshots and at most [`MAX_GROUP_SNAPSHOTS`]
    /// are listed. `dto` shows one snapshot, or `None` for one the wire
    /// can't carry.
    pub fn new<T>(
        hostname: String,
        label: String,
        paths: Vec<String>,
        snapshots: &[T],
        adopt_into: Vec<String>,
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, snapshots) = listed(snapshots, MAX_GROUP_SNAPSHOTS, dto);
        Self {
            hostname,
            label,
            paths,
            count,
            snapshots,
            adopt_into,
        }
    }
}

/// What a forget run would do to one plan's history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ForgetPlanDto {
    pub plan: String,
    /// How many snapshots the plan owns.
    pub total: u64,
    /// How many of them the plan's retention lets go.
    pub remove: u64,
    /// The ones it lets go, newest first, at most [`MAX_PREVIEW_SNAPSHOTS`].
    pub listed: Vec<SnapshotDto>,
}

impl ForgetPlanDto {
    /// What a forget would do to `plan`, which owns `total` snapshots and
    /// whose policy lets `remove` go, newest first: at most
    /// [`MAX_PREVIEW_SNAPSHOTS`] are listed. `dto` shows one snapshot, or
    /// `None` for one the wire can't carry.
    pub fn new<T>(
        plan: String,
        total: u64,
        remove: &[T],
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (remove, listed) = listed(remove, MAX_PREVIEW_SNAPSHOTS, dto);
        Self {
            plan,
            total,
            remove,
            listed,
        }
    }
}

/// How many groups and snapshots a forget leaves alone of one kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct TallyDto {
    pub groups: u64,
    pub snapshots: u64,
}

/// The history a forget leaves alone (I6): groups no plan of this agent
/// owns, which only an adoption or an explicit deletion touches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LeftAloneDto {
    pub orphaned: TallyDto,
    pub unmanaged: TallyDto,
}

/// What a maintenance run would do, told before it starts: a read, so it
/// takes no maintenance slot and writes no log row (§7.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum MaintenancePreviewDto {
    /// Which snapshots of each plan the plan's retention lets go, and what
    /// is left alone. `selection` names that selection: a start that carries
    /// it is refused when the repository has changed since.
    Forget {
        plans: Vec<ForgetPlanDto>,
        left_alone: LeftAloneDto,
        selection: String,
        /// What the review warns of: the other hosts the repository holds
        /// snapshots from, when it is maintained here (§7.3). Left out when
        /// there is nothing to warn of.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningDto>,
    },
    /// The snapshots an adoption would rewrite into the plan `into`.
    Adopt {
        into: String,
        /// How many snapshots the adoption rewrites.
        count: u64,
        /// The first of them, newest first, at most
        /// [`MAX_PREVIEW_SNAPSHOTS`].
        listed: Vec<SnapshotDto>,
        selection: String,
        /// As a forget's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningDto>,
    },
    /// What a prune would do, from a dry run of the engine's own plan. Its
    /// figures are estimates: the plan is computed again when the prune
    /// runs.
    Prune {
        packs_marked: u64,
        packs_deleted: u64,
        packs_recovered: u64,
        bytes_deleted: u64,
        /// How long a pack stays marked before a later prune deletes it
        /// (I2).
        keep_delete_hours: u64,
        /// As a forget's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningDto>,
    },
    /// A check changes nothing; it holds the repository while it runs.
    Check { read_data: bool },
}

impl MaintenancePreviewDto {
    /// The preview of an adoption into `into` of `snapshots`, newest first:
    /// `count` is all of them and at most [`MAX_PREVIEW_SNAPSHOTS`] are
    /// listed. `dto` shows one snapshot, or `None` for one the wire can't
    /// carry. It warns of nothing until [`warned`](Self::warned) says.
    pub fn adopt<T>(
        into: String,
        snapshots: &[T],
        selection: String,
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, listed) = listed(snapshots, MAX_PREVIEW_SNAPSHOTS, dto);
        Self::Adopt {
            into,
            count,
            listed,
            selection,
            warnings: Vec::new(),
        }
    }

    /// The same preview, warning of `warnings`. A check's warns of nothing:
    /// it only reads.
    #[must_use]
    pub fn warned(mut self, warned: Vec<WarningDto>) -> Self {
        match &mut self {
            Self::Forget { warnings, .. }
            | Self::Adopt { warnings, .. }
            | Self::Prune { warnings, .. } => *warnings = warned,
            Self::Check { .. } => {}
        }
        self
    }

    /// What the review warns of.
    pub fn warnings(&self) -> &[WarningDto] {
        match self {
            Self::Forget { warnings, .. }
            | Self::Adopt { warnings, .. }
            | Self::Prune { warnings, .. } => warnings,
            Self::Check { .. } => &[],
        }
    }
}

/// How a request names the group an adoption rewrites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "from", from = "AdoptionFromWire")]
pub enum AdoptionFromDto {
    /// The orphaned group these two tags name.
    Orphaned {
        /// The `ladderback-agent:` tag's text.
        agent: String,
        /// The `ladderback-plan:` tag's text.
        plan: String,
    },
    /// The unmanaged group whose host, label and paths equal the
    /// destination plan's host, label and sources.
    Unmanaged,
}

/// What [`AdoptionFromDto`] is read from. Serde's internally tagged unit
/// variant skips every key it doesn't know, so the read goes through a
/// struct variant that refuses them: the wire is the same, and a key the
/// body doesn't know is an invalid body.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", tag = "from", deny_unknown_fields)]
enum AdoptionFromWire {
    Orphaned { agent: String, plan: String },
    Unmanaged {},
}

impl From<AdoptionFromWire> for AdoptionFromDto {
    fn from(wire: AdoptionFromWire) -> Self {
        match wire {
            AdoptionFromWire::Orphaned { agent, plan } => Self::Orphaned { agent, plan },
            AdoptionFromWire::Unmanaged {} => Self::Unmanaged,
        }
    }
}

/// The body of a maintenance preview and of a maintenance start: the kind,
/// for an adoption its destination plan and the group it rewrites, and, on a
/// start, the selection a preview named.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MaintenanceRequestDto {
    /// `forget`, `prune`, `check` or `adopt`.
    pub kind: String,
    /// The plan an adoption's history becomes part of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    /// The group an adoption rewrites: another owner's tagged history, or
    /// the unmanaged history that matches the destination plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<AdoptionFromDto>,
    /// The selection a preview named, for a forget or an adoption. Absent,
    /// the run selects as it finds the repository; present, it is refused
    /// when what it would select is not what the preview showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
}

/// Where a restore writes (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreDestinationDto {
    /// A folder made for this restore under the agent's restore folder.
    NewFolder,
    /// Over the original paths: adds and overwrites, never deletes.
    InPlace,
}

/// `POST /api/v1/repositories/{alias}/snapshots/{snapshot}/restore/preview`:
/// the entry whose restore in place a dry run is to tell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestorePreviewRequestDto {
    /// The entry's path in the snapshot, its names joined by `/`; empty for
    /// the snapshot's top.
    pub path: String,
}

/// What restoring an entry in place would do, from a dry run that wrote
/// nothing (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestorePreviewDto {
    /// Files it would add, where nothing is.
    pub added: u64,
    /// Files it would overwrite, which differ from the snapshot's.
    pub overwritten: u64,
    /// Files already as the snapshot holds them.
    pub unchanged: u64,
    /// Entries the original holds and the snapshot doesn't: they stay. A
    /// folder of them counts once.
    pub kept: u64,
    /// Bytes it would write.
    pub bytes: u64,
}

/// `POST /api/v1/repositories/{alias}/snapshots/{snapshot}/restore`: the
/// entry to restore, where to, and the step-up confirmation: the admin
/// password with sign-in on, the snapshot's short ID otherwise. The
/// confirmation is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestoreRequestDto {
    /// The entry's path in the snapshot, as [`RestorePreviewRequestDto`]
    /// gives it.
    pub path: String,
    pub destination: RestoreDestinationDto,
    pub confirmation: String,
}

impl std::fmt::Debug for RestoreRequestDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreRequestDto")
            .field("path", &self.path)
            .field("destination", &self.destination)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// What `POST …/snapshots/{snapshot}/restore` answers once the restore
/// started: its operation's ID and, for a restore into a new folder, that
/// folder, so the person is told where its files are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestoreStartedDto {
    pub operation: String,
    /// The folder made for the restore; absent when it restores over the
    /// original paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// The archive a downloaded folder is handed over as (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveFormatDto {
    Tar,
    Zip,
}

/// `POST /api/v1/repositories/{alias}/snapshots/{snapshot}/download`: the
/// entry to download, the archive a folder goes as, and the step-up
/// confirmation, as a restore asks for it. The confirmation is never
/// logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DownloadRequestDto {
    /// The entry's path in the snapshot, as [`RestorePreviewRequestDto`]
    /// gives it; empty for the snapshot's top.
    pub path: String,
    /// The archive a folder is handed over as; a file goes as itself.
    pub format: ArchiveFormatDto,
    pub confirmation: String,
}

impl std::fmt::Debug for DownloadRequestDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadRequestDto")
            .field("path", &self.path)
            .field("format", &self.format)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// What `POST …/snapshots/{snapshot}/download` answers once the download
/// is confirmed: the ticket its browser fetches it by, once, within a
/// minute, at `GET /api/v1/downloads/{ticket}`. A capability: never logged
/// or shown, and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloadStartedDto {
    pub ticket: String,
}

impl std::fmt::Debug for DownloadStartedDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadStartedDto")
            .field("ticket", &"[hidden]")
            .finish()
    }
}

/// One recorded warning of an operation's detail: its typed code and the
/// free-form fields the engine reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct OperationWarningDto {
    pub code: WarningCode,
    /// The affected item, usually a path.
    pub item: Option<String>,
    /// What the engine was doing.
    pub during: String,
    /// The engine's own message, verbatim.
    pub detail: String,
}

/// One operation's detail: its summary and the warnings it recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct OperationDetailDto {
    pub operation: OperationSummaryDto,
    pub warnings: Vec<OperationWarningDto>,
}

/// What a successful operation produced, as the history page shows it: a
/// small typed summary per kind, mapped from the app's engine result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum OperationResultDto {
    Backup {
        /// Files the backup went through: new, changed and unmodified.
        files: u64,
        /// Bytes of new data the backup added to the repository, before
        /// compression.
        bytes: u64,
        duration_ms: u64,
        /// The snapshot the backup wrote: 64 hex digits. Absent from a
        /// result an earlier version sent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
        /// Bytes of the source the backup went through, changed or not.
        /// Absent when the log didn't keep them, for a backup an earlier
        /// version ran.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes_processed: Option<u64>,
    },
    Prune {
        packs_deleted: u64,
        duration_ms: u64,
    },
    Check {
        errors: u64,
        warnings: u64,
        duration_ms: u64,
    },
    Forget {
        removed: u64,
        duration_ms: u64,
    },
    /// What an adoption produced: the snapshots rewritten.
    Adopt {
        rewritten: u64,
        duration_ms: u64,
    },
    /// What a restore wrote: the files written, new or overwritten, and
    /// their bytes.
    Restore {
        files: u64,
        bytes: u64,
        duration_ms: u64,
    },
}
