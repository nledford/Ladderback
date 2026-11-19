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

mod access;
mod codes;
mod events;
mod execution;
mod interop;
mod planning;
mod repositories;
mod snapshots;
mod status;
pub mod sync;
#[cfg(test)]
mod tests;
mod updates;

pub use access::*;
pub use codes::*;
pub use events::*;
pub use execution::*;
pub use interop::*;
pub use planning::*;
pub use repositories::*;
pub use snapshots::*;
pub use status::*;
pub use updates::*;

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
