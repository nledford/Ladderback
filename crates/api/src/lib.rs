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
mod maintenance;
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
pub use maintenance::*;
pub use planning::*;
pub use repositories::*;
pub use snapshots::*;
pub use status::*;
pub use updates::*;

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
