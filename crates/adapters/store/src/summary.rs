//! The engine's result as `operations.summary_json` keeps it: the app has no
//! serde, and this is the persistence format, so it is spelled out here
//! rather than taken from a type that may change with the app.
//!
//! Reading ignores fields it doesn't know, so a database written by a newer
//! version that added one can still be read; a missing field or an unknown
//! `type` is an error.

use app::ports::{BackupSummary, CheckSummary, EngineResult, PruneSummary};
use domain::{RepositoryId, SnapshotId};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Summary {
    Fake {
        steps_completed: u32,
    },
    Repository {
        id: RepositoryId,
    },
    Backup {
        repository: RepositoryId,
        snapshot: SnapshotId,
        parent: Option<SnapshotId>,
        files_new: u64,
        files_changed: u64,
        files_unmodified: u64,
        data_added: u64,
        data_added_packed: u64,
        error_count: u64,
    },
    Prune {
        repository: RepositoryId,
        packs_marked: u64,
        packs_deleted: u64,
        packs_recovered: u64,
    },
    Check {
        repository: RepositoryId,
        errors: u64,
        warnings: u64,
    },
}

impl From<&EngineResult> for Summary {
    fn from(result: &EngineResult) -> Self {
        match result {
            EngineResult::Fake { steps_completed } => Self::Fake {
                steps_completed: *steps_completed,
            },
            EngineResult::Repository { id } => Self::Repository { id: id.clone() },
            EngineResult::Backup(summary) => Self::Backup {
                repository: summary.repository.clone(),
                snapshot: summary.snapshot.clone(),
                parent: summary.parent.clone(),
                files_new: summary.files_new,
                files_changed: summary.files_changed,
                files_unmodified: summary.files_unmodified,
                data_added: summary.data_added,
                data_added_packed: summary.data_added_packed,
                error_count: summary.error_count,
            },
            EngineResult::Prune(summary) => Self::Prune {
                repository: summary.repository.clone(),
                packs_marked: summary.packs_marked,
                packs_deleted: summary.packs_deleted,
                packs_recovered: summary.packs_recovered,
            },
            EngineResult::Check(summary) => Self::Check {
                repository: summary.repository.clone(),
                errors: summary.errors,
                warnings: summary.warnings,
            },
        }
    }
}

impl From<Summary> for EngineResult {
    fn from(summary: Summary) -> Self {
        match summary {
            Summary::Fake { steps_completed } => Self::Fake { steps_completed },
            Summary::Repository { id } => Self::Repository { id },
            Summary::Backup {
                repository,
                snapshot,
                parent,
                files_new,
                files_changed,
                files_unmodified,
                data_added,
                data_added_packed,
                error_count,
            } => Self::Backup(BackupSummary {
                repository,
                snapshot,
                parent,
                files_new,
                files_changed,
                files_unmodified,
                data_added,
                data_added_packed,
                error_count,
            }),
            Summary::Prune {
                repository,
                packs_marked,
                packs_deleted,
                packs_recovered,
            } => Self::Prune(PruneSummary {
                repository,
                packs_marked,
                packs_deleted,
                packs_recovered,
            }),
            Summary::Check {
                repository,
                errors,
                warnings,
            } => Self::Check(CheckSummary {
                repository,
                errors,
                warnings,
            }),
        }
    }
}
