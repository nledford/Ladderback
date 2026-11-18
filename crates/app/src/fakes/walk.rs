//! A `WalkSources` fake: a tree held in memory, walked in path order. It
//! applies the one exclude the use cases' tests write, a glob `!name` that
//! leaves out every entry named `name` and what lies under it, and refuses
//! any other filter rather than ignore it.

use async_trait::async_trait;
use domain::planning::{Source, SourceFilter, is_within};

use crate::ports::{GiveWay, WalkError, WalkLimits, WalkSources, WalkStop, Walked};

/// A tree of entries, each an absolute path and, for a file, its size; a
/// folder has none.
#[derive(Default)]
pub struct InMemoryTree {
    entries: Vec<(String, Option<u64>)>,
}

impl InMemoryTree {
    /// The tree of `entries`, walked in path order.
    pub fn new(entries: impl IntoIterator<Item = (String, Option<u64>)>) -> Self {
        let mut entries: Vec<_> = entries.into_iter().collect();
        entries.sort();
        Self { entries }
    }
}

/// The names a `!name` glob leaves out, or none for a filter the fake
/// can't apply.
fn left_out(filter: &SourceFilter) -> Option<Vec<&str>> {
    let only_globs = SourceFilter {
        globs: filter.globs.clone(),
        ..SourceFilter::default()
    };
    if *filter != only_globs {
        return None;
    }
    filter
        .globs
        .iter()
        .map(|glob| {
            glob.strip_prefix('!')
                .filter(|name| !name.contains(['/', '*']))
        })
        .collect()
}

#[async_trait]
impl WalkSources for InMemoryTree {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        let names = left_out(filter).ok_or(WalkError::Filter)?;
        let mut walked = Walked::default();
        let mut read: u64 = 0;
        let below = self.entries.iter().filter(|(path, _)| {
            sources
                .iter()
                .any(|source| path.as_str() != source.as_str() && is_within(path, source.as_str()))
        });
        for (path, size) in below {
            if give_way() {
                walked.stopped = Some(WalkStop::GaveWay);
                break;
            }
            if !limits.time.is_positive() {
                walked.stopped = Some(WalkStop::Time);
                break;
            }
            if read >= limits.entries {
                walked.stopped = Some(WalkStop::Entries);
                break;
            }
            read = read.saturating_add(1);
            if path.split('/').any(|part| names.contains(&part)) {
                continue;
            }
            if let Some(size) = size {
                walked.files = walked.files.saturating_add(1);
                walked.bytes = walked.bytes.saturating_add(*size);
            }
        }
        Ok(walked)
    }
}
