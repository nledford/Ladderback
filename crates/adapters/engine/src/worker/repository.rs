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
    let backends = BackendOptions::default()
        .repository(spec.repository.clone())
        .options(spec.options.clone())
        .to_backends()?;
    let options = RepositoryOptions::default();
    let options = match cache_dir(ctx)? {
        Some(dir) => options.cache_dir(dir),
        None => options.no_cache(true),
    };
    let repository = Repository::new_with_progress(
        &options,
        &backends,
        EventProgressBars::new(ctx.events.clone()),
    )?;
    let repository = repository.open(&Credentials::password(spec.password.expose()))?;
    // Only a repository that opened is known to be one, so only then is what
    // looks like its leftovers ours to remove.
    remove_leftovers(ctx, &spec.repository)?;
    Ok(repository)
}

/// The cache directory the job may use: the one it was given, if no other
/// worker holds it. Two workers must not share one (§4.1), so a held
/// directory means running without a cache.
fn cache_dir(ctx: &Context) -> io::Result<Option<PathBuf>> {
    let Some(dir) = &ctx.cache_dir else {
        return Ok(None);
    };
    match ctx.lock_cache(dir) {
        Ok(()) => Ok(Some(dir.clone())),
        Err(error) => {
            let why = if error.kind() == io::ErrorKind::WouldBlock {
                "another worker is using it".to_owned()
            } else {
                error.to_string()
            };
            ctx.log(
                Level::Warn,
                format!(
                    "running without a cache: the cache folder {} can't be used: {why}",
                    dir.display()
                ),
            )?;
            Ok(None)
        }
    }
}

/// The repository's ID as Steading knows it: 64 lowercase hex characters
/// (`Id`'s `Display` is the first 8).
pub(crate) fn repository_id(repository: &Repository<OpenStatus>) -> Result<RepositoryId, JobError> {
    RepositoryId::parse(repository.config().id.to_hex().as_str())
        .map_err(|_| JobError::failed("the repository's ID is not 64 hexadecimal characters"))
}

/// A killed worker leaves `<id>-tmp-*` files in a local repository (§4.1).
/// Removing the stale ones is a courtesy, so failing to is only logged.
fn remove_leftovers(ctx: &Context, repository: &str) -> io::Result<()> {
    let Ok((SupportedBackend::Local, location)) = location_to_type_and_path(repository) else {
        return Ok(());
    };
    let cleanup = remove_stale_files(Path::new(location.as_str()), SystemTime::now());
    for path in &cleanup.removed {
        ctx.log(
            Level::Info,
            format!("removed a leftover temporary file: {}", path.display()),
        )?;
    }
    for (path, error) in &cleanup.failed {
        ctx.log(
            Level::Warn,
            format!(
                "could not remove a leftover temporary file {}: {error}",
                path.display()
            ),
        )?;
    }
    Ok(())
}
