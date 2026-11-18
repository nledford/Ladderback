//! The plan dry run's walk (§16, decision 89, point 13): rustic's own
//! source walker, built from a plan's source filter as a backup builds it,
//! read entry by entry and counted. It runs in the daemon, not in a
//! worker: it opens no repository and runs no hook, and stops at its
//! limits or to give way between two entries. Its records stay out of the
//! daemon's log, its warnings counted ([`unlogged`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use app::ports::{GiveWay, WalkError, WalkLimits, WalkSources, WalkStop, Walked};
use async_trait::async_trait;
use domain::planning::{Source, SourceFilter};
use rustic_core::{LocalSource, ReadSource};

use crate::client::filter_spec;
use crate::log_masking::unlogged;
use crate::worker::backup_options;

/// The most a `glob-files` or `iglob-files` file may hold for a dry run:
/// each is read whole as the walk is built (the glob files by rustic, the
/// iglob files by `backup_options`), before any limit.
const MAX_GLOB_FILE_BYTES: u64 = 1024 * 1024;

/// Walks sources with rustic's `LocalSource`, the walker a backup reads.
pub struct RusticWalk;

#[async_trait]
impl WalkSources for RusticWalk {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        left_out: &[String],
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        let paths: Vec<PathBuf> = sources.iter().map(|s| PathBuf::from(s.as_str())).collect();
        if paths.is_empty() {
            return Ok(Walked::default());
        }
        let glob_files: Vec<PathBuf> = filter
            .glob_files
            .iter()
            .chain(&filter.iglob_files)
            .map(|file| PathBuf::from(file.as_str()))
            .collect();
        let spec = filter_spec(filter.clone(), left_out.to_vec());
        let time = Duration::try_from(limits.time).unwrap_or(Duration::ZERO);
        tokio::task::spawn_blocking(move || {
            let (walked, warnings) = unlogged(|| {
                if !glob_files.iter().all(|file| fits(file)) {
                    return Err(WalkError::Filter);
                }
                let options = backup_options(&spec).map_err(|_| WalkError::Filter)?;
                let walker = LocalSource::new(
                    options.ignore_save_opts,
                    &options.excludes,
                    &options.ignore_filter_opts,
                    &paths,
                )
                .map_err(|_| WalkError::Filter)?;
                Ok(counted(walker.entries(), limits.entries, time, &*give_way))
            });
            walked.map(|walked| Walked { warnings, ..walked })
        })
        .await
        .map_err(|error| WalkError::Engine {
            detail: error.to_string(),
        })?
    }
}

/// Whether rustic may read the glob file `file` whole: a regular file, its
/// links followed, of at most [`MAX_GLOB_FILE_BYTES`], as its metadata
/// says, so a device or a large file is refused before anything is read.
fn fits(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_GLOB_FILE_BYTES)
}

/// Counts `entries`, before each asking whether to give way and checking
/// the time and the entries left.
fn counted<O>(
    entries: impl Iterator<Item = rustic_core::RusticResult<rustic_core::ReadSourceEntry<O>>>,
    limit: u64,
    time: Duration,
    give_way: &(dyn Fn() -> bool + Send + Sync),
) -> Walked {
    let started = Instant::now();
    let mut walked = Walked::default();
    let mut read: u64 = 0;
    let mut entries = entries.peekable();
    while entries.peek().is_some() {
        if give_way() {
            walked.stopped = Some(WalkStop::GaveWay);
            break;
        }
        if started.elapsed() >= time {
            walked.stopped = Some(WalkStop::Time);
            break;
        }
        if read >= limit {
            walked.stopped = Some(WalkStop::Entries);
            break;
        }
        read = read.saturating_add(1);
        match entries.next() {
            Some(Ok(entry)) if !entry.node.is_dir() => {
                walked.files = walked.files.saturating_add(1);
                walked.bytes = walked.bytes.saturating_add(entry.node.meta.size);
            }
            Some(Ok(_)) | None => {}
            Some(Err(_)) => walked.unreadable = walked.unreadable.saturating_add(1),
        }
    }
    walked
}
