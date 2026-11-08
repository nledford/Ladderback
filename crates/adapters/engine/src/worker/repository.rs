//! Opening a repository for a job.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use domain::RepositoryId;
use rustic_backend::util::location_to_type_and_path;
use rustic_backend::{BackendOptions, SupportedBackend};
use rustic_core::{Credentials, OpenStatus, Repository, RepositoryOptions};

use super::Context;
use super::convert::JobError;
use super::progress::EventProgressBars;
use super::tmp_files::remove_stale_files;
use crate::job::{Level, RepositorySpec};

/// Opens the repository: reads the config and keys, and then (a local
/// repository only) cleans up after killed workers. Reports through
/// `Context::events` how long opening takes, as progress.
pub(crate) fn open(
    ctx: &Context,
    spec: &RepositorySpec,
) -> Result<Repository<OpenStatus>, JobError> {
    todo!()
}

/// The cache directory the job may use: the one it was given, if no other
/// worker holds it. Two workers must not share one (§4.1), so a held
/// directory means running without a cache.
fn cache_dir(ctx: &Context) -> io::Result<Option<PathBuf>> {
    todo!()
}

/// The repository's ID as Steading knows it: 64 lowercase hex characters
/// (`Id`'s `Display` is the first 8).
pub(crate) fn repository_id(repository: &Repository<OpenStatus>) -> Result<RepositoryId, JobError> {
    todo!()
}

/// A killed worker leaves `<id>-tmp-*` files in a local repository (§4.1).
/// Removing the stale ones is a courtesy, so failing to is only logged.
fn remove_leftovers(ctx: &Context, repository: &str) -> io::Result<()> {
    todo!()
}
