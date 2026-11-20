//! A search of a repository's snapshots for entries by name (§16, decision
//! 127): each chosen snapshot's tree walked from its top, newest snapshot
//! first. A tree's ID is the hash of all it holds, so a folder that didn't
//! change between snapshots is read once: what its tree held that matched is
//! kept by its ID, and read from there for every other snapshot that holds
//! it. The search stops at the first cap it meets: the matches it may
//! return, the bytes one message may carry, or its time.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use domain::SnapshotId;
use domain::Timestamp;
use domain::browse::{DisplayName, MAX_ENTRY_NAME_BYTES, MAX_FOLDER_DEPTH};
use domain::find::{NamePattern, SearchStop};

use super::convert::JobError;
use crate::job::{
    EntryResult, FindResult, LISTING_BATCH_BYTES, MatchResult, SearchedResult, SeenResult,
};

/// The most the matches may come to, serialized: the answer is one message
/// of at most [`LISTING_BATCH_BYTES`], and the snapshots it lists take the
/// rest (100 of them, about 120 bytes each).
const MATCH_BYTES: usize = LISTING_BATCH_BYTES / 4 * 3;

/// One entry of a tree, as a search reads it.
pub(crate) struct Child<K> {
    /// The entry as a match reports it: its key, its name to show, its kind,
    /// size and time.
    pub(crate) entry: EntryResult,
    /// The name the file system wrote, as text, for the pattern to match.
    pub(crate) name: String,
    /// The folder's tree, for a folder.
    pub(crate) subtree: Option<K>,
}

/// The trees of a repository, each by its ID.
pub(crate) trait Trees {
    type Id: Clone + Eq + Hash;

    /// The entries of the tree `id`, in the order it records them.
    fn children(&self, id: &Self::Id) -> Result<Vec<Child<Self::Id>>, JobError>;
}

/// A snapshot to search: its ID, its time and its top tree.
pub(crate) struct Root<K> {
    pub(crate) snapshot: SnapshotId,
    pub(crate) time: Timestamp,
    pub(crate) tree: K,
}

/// Searches the tree of each of `roots`, newest first, for entries whose
/// names match `pattern`, returning at most `most` of them (an entry in one
/// snapshot is one). `left_out` snapshots were chosen not to be searched;
/// `out_of_time` says when the search's time is up, asked before each tree
/// is read.
pub(crate) fn search<T: Trees>(
    trees: &T,
    roots: Vec<Root<T::Id>>,
    left_out: u64,
    pattern: &NamePattern,
    most: u32,
    out_of_time: impl FnMut() -> bool,
) -> Result<FindResult, JobError> {
    let _ = (trees, roots, left_out, pattern, most, out_of_time);
    todo!()
}

/// Why a walk ended before its tree's end.
enum Halt {
    Stop(SearchStop),
    Failed(JobError),
}

impl From<SearchStop> for Halt {
    fn from(stop: SearchStop) -> Self {
        Self::Stop(stop)
    }
}

/// What a tree held that matched, in the order it records them: entries
/// whose names match, and folders that hold some.
enum Hit<K> {
    Here(EntryResult),
    Inside { key: String, shown: String, tree: K },
}

struct Search<'a, T: Trees, F> {
    trees: &'a T,
    pattern: &'a NamePattern,
    out_of_time: F,
    /// Every tree read to its end, and what it held that matched.
    read: HashMap<T::Id, Rc<Vec<Hit<T::Id>>>>,
    found: Found,
}

impl<T: Trees, F: FnMut() -> bool> Search<'_, T, F> {
    /// Adds what `tree`, at `path`, holds that matches to the matches as the
    /// snapshot `snapshot`'s, and says whether it held any.
    fn walk(
        &mut self,
        tree: &T::Id,
        path: &mut Vec<(String, String)>,
        snapshot: &SnapshotId,
    ) -> Result<bool, Halt> {
        if let Some(hits) = self.read.get(tree).map(Rc::clone) {
            self.replay(&hits, path, snapshot)?;
            return Ok(!hits.is_empty());
        }
        if (self.out_of_time)() {
            return Err(Halt::Stop(SearchStop::Time));
        }
        let children = self.trees.children(tree).map_err(Halt::Failed)?;
        let mut hits = Vec::new();
        for child in children {
            // No file system writes a longer name: a browse leaves it out.
            if child.entry.name.len() > MAX_ENTRY_NAME_BYTES {
                continue;
            }
            path.push((
                child.entry.name.clone(),
                child.entry.display.as_str().to_owned(),
            ));
            if self.pattern.matches(&child.name) {
                self.found.add(path, &child.entry, snapshot)?;
                hits.push(Hit::Here(child.entry.clone()));
            }
            // A folder deeper than a path names can't be browsed to, so it
            // isn't searched either.
            if let Some(subtree) = child.subtree
                && path.len() < MAX_FOLDER_DEPTH
                && self.walk(&subtree, path, snapshot)?
            {
                hits.push(Hit::Inside {
                    key: child.entry.name,
                    shown: child.entry.display.into(),
                    tree: subtree,
                });
            }
            path.pop();
        }
        let held = !hits.is_empty();
        self.read.insert(tree.clone(), Rc::new(hits));
        Ok(held)
    }

    /// Adds `hits`, what a tree read before held, at `path` to the matches
    /// as the snapshot `snapshot`'s.
    fn replay(
        &mut self,
        hits: &[Hit<T::Id>],
        path: &mut Vec<(String, String)>,
        snapshot: &SnapshotId,
    ) -> Result<(), Halt> {
        for hit in hits {
            match hit {
                Hit::Here(entry) => {
                    path.push((entry.name.clone(), entry.display.as_str().to_owned()));
                    self.found.add(path, entry, snapshot)?;
                }
                Hit::Inside { key, shown, tree } => {
                    path.push((key.clone(), shown.clone()));
                    self.walk(tree, path, snapshot)?;
                }
            }
            path.pop();
        }
        Ok(())
    }
}

/// The matches so far, at most `most` of them and [`MATCH_BYTES`].
#[derive(Default)]
struct Found {
    matches: Vec<MatchResult>,
    /// Each match's place in `matches`, by its path's keys.
    at: HashMap<Vec<String>, usize>,
    /// How many entries in snapshots the matches hold.
    count: u32,
    most: u32,
    bytes: usize,
}

impl Found {
    /// Adds `entry`, at `path` (its own name last), as the snapshot
    /// `snapshot` holds it; or stops the search when that is one more than
    /// it may return.
    fn add(
        &mut self,
        path: &[(String, String)],
        entry: &EntryResult,
        snapshot: &SnapshotId,
    ) -> Result<(), SearchStop> {
        if self.count >= self.most {
            return Err(SearchStop::Matches);
        }
        let seen = SeenResult {
            snapshot: snapshot.clone(),
            size: entry.size,
            modified: entry.modified,
        };
        let keys: Vec<String> = path.iter().map(|(key, _)| key.clone()).collect();
        match self.at.get(&keys) {
            Some(&at) => {
                self.spend(&seen)?;
                if let Some(found) = self.matches.get_mut(at) {
                    found.seen.push(seen);
                }
            }
            None => {
                let shown: Vec<&str> = path.iter().map(|(_, shown)| shown.as_str()).collect();
                let found = MatchResult {
                    path: keys.clone(),
                    display: DisplayName::new(&shown.join("/")),
                    kind: entry.kind,
                    seen: vec![seen],
                };
                self.spend(&found)?;
                self.at.insert(keys, self.matches.len());
                self.matches.push(found);
            }
        }
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// Counts `part` against the matches' bytes, as JSON with a comma after
    /// it; or stops the search when it would take them past
    /// [`MATCH_BYTES`].
    fn spend(&mut self, part: &impl serde::Serialize) -> Result<(), SearchStop> {
        let size = serde_json::to_vec(part)
            .map_or(usize::MAX, |json| json.len())
            .saturating_add(1);
        let bytes = self.bytes.saturating_add(size);
        if bytes > MATCH_BYTES {
            return Err(SearchStop::Matches);
        }
        self.bytes = bytes;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
