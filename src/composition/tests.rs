//! The composition root's tests, one file for each area; `support` and
//! `service_support` hold what more than one of them shares.

use super::*;

mod announcements;
mod cancels;
mod concurrent_saves;
mod configuration_refusals;
mod connecting;
mod creating;
mod daemon_start;
mod engine_jobs;
mod hook_output;
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
