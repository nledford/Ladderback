//! Plans, their dry runs and proposals on the wire: the Planning context
//! (technical design §3.1).

use serde::{Deserialize, Serialize};

use crate::{ErrorDto, ForgetPlanDto, WarningDto};

#[cfg(doc)]
use crate::ErrorCode;

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
