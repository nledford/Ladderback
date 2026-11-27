//! The composition root's tests, one file for each area; `support` and
//! `service_support` hold what more than one of them shares.

use app::browse::BrowseError;
use app::conditions::NameWatch;
use app::event_bus::BroadcastEventBus;
use app::execution::Finished;
use app::history::GetOperation;
use app::locations::LocationReport;
use app::notifications::Report;
use app::ports::{
    BackupJob, ConfigError, ConfigStore, HookRunner, Keyring, KeyringError, OperationStore,
    ScheduleStore, SnapshotSummary,
};
use app::queue::RunQueue;
use app::repositories::Recording;
use app::schedule::PlanRuns;
use app::secrets::{Field, ResolveRepository};
use app::snapshots::SnapshotError;
use config_adapter::FileConfigStore;
use domain::browse::FolderPath;
use domain::execution::locations::LocationOutcome;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{Target, Trigger};
use domain::planning::{InterfaceKind, Link, Location, LocationRule, Maintenance, NameFilter};
use domain::secrets::FileRead;
use domain::{AgentId, PlanId, RepositoryAlias, SnapshotId};
use hooks::ProcessHookRunner;
use http_adapter::{
    Catalog, CatalogError, FolderError, MaintenanceRefusal, Operations, Repositories, SignIn,
    SnapshotsError, StartError,
};
use platform::agent_id;
use platform::data_dir_lock::LockError;
use secrets::SystemSecrets;
use uuid::Uuid;

use super::catalog::ConfigCatalog;
use super::key_files::KeyFilesLeftOut;
use super::plan_runner::{BuiltBackup, PlanRunner, Unbuilt, log_ended, operation_of, run_named};
use super::repositories::RepositoryService;
use super::startup::{
    DATA_DIR_IN_USE, WithinLimit, daemon_at, daemon_resolve, lock_refused, log_location,
    log_recording, recovery_refused, report_line, resume_queued, workers_cache,
};
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
mod hub_leases;
mod hub_runs;
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
mod upgrades;
