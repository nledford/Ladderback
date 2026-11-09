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

/// Opens the repository, both halves of it when it has a hot one: reads the
/// config and keys, and then (for each half that is a local folder) cleans up
/// after killed workers. Reports through `Context::events` how long opening
/// takes, as progress.
pub(crate) fn open(
    ctx: &Context,
    spec: &RepositorySpec,
) -> Result<Repository<OpenStatus>, JobError> {
    // rustic gives the cold half `options` and `options_cold`, the hot half
    // `options` and `options_hot`.
    let backend_options = BackendOptions::default()
        .repository(spec.repository.clone())
        .options(spec.options.clone())
        .options_hot(spec.options_hot.clone())
        .options_cold(spec.options_cold.clone());
    let backends = match &spec.repo_hot {
        Some(hot) => backend_options.repo_hot(hot.clone()),
        None => backend_options,
    }
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
    for half in std::iter::once(spec.repository.as_str()).chain(spec.repo_hot.as_deref()) {
        remove_leftovers(ctx, half)?;
    }
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

/// A killed worker leaves `<id>-tmp-*` files in a repository folder, the hot
/// half's as well as the cold half's (§4.1). `half` is the repository string
/// of one of them; one that is not a local folder has nothing to clean.
/// Removing the stale ones is a courtesy, so failing to is only logged.
fn remove_leftovers(ctx: &Context, half: &str) -> io::Result<()> {
    let Ok((SupportedBackend::Local, location)) = location_to_type_and_path(half) else {
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
