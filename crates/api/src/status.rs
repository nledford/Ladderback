//! The daemon's status on the wire: its plans' last runs, the build it runs, its
//! recent log and its operating-system permissions.

use serde::{Deserialize, Serialize};

use crate::{OperationSummaryDto, PlanSummaryDto};

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

/// What `GET /api/v1/machine` answers, and `POST /api/v1/machine/new-id`
/// once the new ID is taken (§7.1, §16 decision 144): where this machine
/// stands, as the check at start found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MachineDto {
    pub standing: MachineStandingDto,
}

/// Where this machine stands: `copied` when its data folder came from
/// another computer, so it runs no forget, prune or adoption;
/// `new-id-taken` once the person took a new machine ID, used from the next
/// start; `unread` when this computer's identifier couldn't be read, so a
/// copy can't be told and the machine runs as usual.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineStandingDto {
    Same,
    Copied,
    NewIdTaken,
    Unread,
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
    /// The line's number since the daemon started, from 1: a newer line has
    /// a larger one, and `read_log`'s `before` takes it.
    pub number: u64,
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
