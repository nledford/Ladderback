//! Operations on the wire: the Execution context (technical design §3.1).

use serde::{Deserialize, Serialize};

use crate::{ErrorCode, ErrorDto, LocationTriedDto, WarningCode};

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
    /// a time that isn't RFC 3339, like
    /// [`SnapshotDto::new`](crate::SnapshotDto::new). The arity is the wire's:
    /// one argument per field of the published shape, except
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

#[cfg(test)]
mod tests;
