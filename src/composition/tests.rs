//! The composition root's tests, one file for each area; `support` and
//! `service_support` hold what more than one of them shares.

use app::browse::BrowseError;
use app::ports::{Keyring, KeyringError, SnapshotSummary};
use app::secrets::Field;
use app::snapshots::SnapshotError;
use domain::browse::FolderPath;
use domain::planning::{InterfaceKind, Link, Location, LocationRule, Maintenance, NameFilter};
use domain::secrets::FileRead;
use domain::{RepositoryAlias, SnapshotId};
use http_adapter::{CatalogError, FolderError, SnapshotsError};

use super::*;

mod announcements;
mod cancels;
mod concurrent_saves;
mod configuration_refusals;
mod connecting;
mod creating;
mod daemon_start;
mod download_sweep;
mod engine_jobs;
mod hook_output;
mod key_files;
mod keyring_passwords;
mod log_lines;
mod maintenance_runs;
mod operation_rows;
mod password_sources;
mod queued_runs;
mod refusals;
mod refused_jobs;
mod refused_runs;
mod repository_reads;
mod repository_requests;
mod repository_rows;
mod secret_reads;
pub(super) mod service_support;
mod stats;
mod support;
