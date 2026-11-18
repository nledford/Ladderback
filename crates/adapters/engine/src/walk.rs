//! The plan dry run's walk (§16, decision 89, point 13): rustic's own
//! source walker, built from a plan's source filter as a backup builds it,
//! read entry by entry and counted. It runs in the daemon, not in a
//! worker: it opens no repository and runs no hook, and stops at its
//! limits or to give way between two entries.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use app::ports::{GiveWay, WalkError, WalkLimits, WalkSources, WalkStop, Walked};
use async_trait::async_trait;
use domain::planning::{Source, SourceFilter};
use rustic_core::{LocalSource, ReadSource};

use crate::client::filter_spec;
use crate::worker::backup_options;

/// Walks sources with rustic's `LocalSource`, the walker a backup reads.
pub struct RusticWalk;

#[async_trait]
impl WalkSources for RusticWalk {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        todo!("{sources:?} {filter:?} {limits:?} {}", give_way())
    }
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
