//! Finding entries by name across a repository's snapshots (§3.1, "Snapshot
//! search"; §16, decision 132): which snapshots to search, what a name
//! pattern matches, and the caps that bound a search. A pattern and the
//! caps come from outside the process, so each is bounded where it is made.

use crate::browse::{DisplayName, EntryKind, FolderPath};
use crate::execution::plan_tags::PLAN_TAG_PREFIX;
use crate::{PlanId, SnapshotId, Timestamp};

/// The longest name pattern, in characters: no file system writes a longer
/// name, so a longer pattern could match nothing.
pub const MAX_PATTERN_CHARS: usize = 255;

/// How many snapshots a search reads when it isn't told.
pub const DEFAULT_SNAPSHOTS: u32 = 20;

/// The most snapshots a search reads, however many it is told.
pub const MAX_SNAPSHOTS: u32 = 100;

/// How many matches a search returns when it isn't told.
pub const DEFAULT_MATCHES: u32 = 100;

/// The most matches a search returns, however many it is told.
pub const MAX_MATCHES: u32 = 500;

/// How long a search walks the snapshots' trees, in seconds, before it
/// stops with what it found. Short enough that a search through a running
/// daemon is answered within the 30 s a client waits (§4.6).
pub const SEARCH_SECONDS: u64 = 15;

/// A pattern for an entry's name: `*` stands for any run of characters,
/// `?` for any one, and every other character for itself, whatever its
/// case. It matches a whole name, never a path, so it finds an entry at any
/// depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamePattern {
    written: String,
    folded: Vec<char>,
}

/// Why a text is no name pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidPattern {
    Empty,
    /// Longer than [`MAX_PATTERN_CHARS`].
    TooLong,
    /// Holds `/`, which no name does, or a NUL.
    NotAName,
}

impl NamePattern {
    /// The pattern `text` is.
    pub fn parse(text: &str) -> Result<Self, InvalidPattern> {
        if text.is_empty() {
            return Err(InvalidPattern::Empty);
        }
        // Counted before anything is copied, so a hostile text costs no more
        // than the longest pattern.
        if text.chars().nth(MAX_PATTERN_CHARS).is_some() {
            return Err(InvalidPattern::TooLong);
        }
        if text.contains(['/', '\0']) {
            return Err(InvalidPattern::NotAName);
        }
        Ok(Self {
            written: text.to_owned(),
            folded: text.to_lowercase().chars().collect(),
        })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.written
    }

    /// Whether `name`, as the file system wrote it, matches the whole
    /// pattern, whatever its case. Never worse than the name's length times
    /// the pattern's: only the last `*` seen is ever made to take more.
    pub fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.to_lowercase().chars().collect();
        let pattern = self.folded.as_slice();
        let (mut at_pattern, mut at_name) = (0_usize, 0_usize);
        // The last `*` seen, and where in the name it was last made to end.
        let mut star: Option<(usize, usize)> = None;
        while let Some(character) = name.get(at_name) {
            match pattern.get(at_pattern) {
                Some('*') => {
                    star = Some((at_pattern, at_name));
                    at_pattern = at_pattern.saturating_add(1);
                }
                Some(wanted) if *wanted == '?' || wanted == character => {
                    at_pattern = at_pattern.saturating_add(1);
                    at_name = at_name.saturating_add(1);
                }
                _ => {
                    let Some((star_at, ended)) = star else {
                        return false;
                    };
                    // The `*` takes one more character, and the rest of the
                    // pattern is tried after it.
                    let ended = ended.saturating_add(1);
                    star = Some((star_at, ended));
                    at_pattern = star_at.saturating_add(1);
                    at_name = ended;
                }
            }
        }
        pattern
            .get(at_pattern..)
            .is_some_and(|rest| rest.iter().all(|wanted| *wanted == '*'))
    }
}

/// The caps a search is told: how many snapshots it reads and how many
/// matches it returns, each from 1 to its most. A number past either end is
/// clamped, never refused, as a page's length is (§4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchCaps {
    snapshots: u32,
    matches: u32,
}

impl SearchCaps {
    /// The caps `snapshots` and `matches` make, each its default when not
    /// given.
    pub fn clamped(snapshots: Option<u64>, matches: Option<u64>) -> Self {
        let clamp = |given: Option<u64>, default: u32, most: u32| {
            given.map_or(default, |given| {
                u32::try_from(given.clamp(1, u64::from(most))).unwrap_or(most)
            })
        };
        Self {
            snapshots: clamp(snapshots, DEFAULT_SNAPSHOTS, MAX_SNAPSHOTS),
            matches: clamp(matches, DEFAULT_MATCHES, MAX_MATCHES),
        }
    }

    /// The most snapshots the search reads.
    pub fn snapshots(self) -> u32 {
        self.snapshots
    }

    /// The most matches the search returns: an entry at one path is one,
    /// however many snapshots hold it.
    pub fn matches(self) -> u32 {
        self.matches
    }
}

/// A snapshot as a search chooses among them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: SnapshotId,
    pub time: Timestamp,
    pub tags: Vec<String>,
}

/// A snapshot a search reads, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Searched {
    pub snapshot: SnapshotId,
    pub time: Timestamp,
}

/// The snapshots a search reads, and how many it leaves out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chosen {
    /// Newest first.
    pub searched: Vec<Searched>,
    pub left_out: u64,
}

/// The snapshots of `listed` a search reads: those of `plan` when it names
/// one (the first `ladderback-plan:` tag, as the snapshot list reads it),
/// newest first, at most `most`; and how many more there were.
pub fn choose(mut listed: Vec<Listed>, plan: Option<&PlanId>, most: u32) -> Chosen {
    listed.retain(|snapshot| {
        plan.is_none_or(|plan| {
            snapshot
                .tags
                .iter()
                .find_map(|tag| tag.strip_prefix(PLAN_TAG_PREFIX))
                == Some(plan.as_str())
        })
    });
    // Newest first; two of the same time in the order of their IDs, so a
    // search reads the same ones each time.
    listed.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.id.cmp(&b.id)));
    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let left_out = u64::try_from(listed.len().saturating_sub(most)).unwrap_or(u64::MAX);
    listed.truncate(most);
    Chosen {
        searched: listed
            .into_iter()
            .map(|snapshot| Searched {
                snapshot: snapshot.id,
                time: snapshot.time,
            })
            .collect(),
        left_out,
    }
}

/// Which cap ended a search before it had read every snapshot it chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchStop {
    /// The snapshots to read: more were there than it was told to read.
    Snapshots,
    /// The matches to return: it found as many as it was told to return
    /// and there were more, or the matches reached the most one answer
    /// carries.
    Matches,
    /// Its time: [`SEARCH_SECONDS`] passed.
    Time,
}

/// One snapshot that holds a match, and the entry as it holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub snapshot: SnapshotId,
    /// A file's size in bytes.
    pub size: Option<u64>,
    pub modified: Option<Timestamp>,
}

/// An entry whose name matches, at one path, and each snapshot it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// The entry's path from the snapshot's top, by its names as rustic
    /// records them.
    pub path: FolderPath,
    /// The path as a person reads it.
    pub display: DisplayName,
    pub kind: EntryKind,
    /// Newest snapshot first.
    pub seen: Vec<Seen>,
}

/// What a search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchAnswer {
    /// The snapshots read to the end, newest first.
    pub searched: Vec<Searched>,
    /// How many snapshots it chose from weren't read to the end.
    pub left_out: u64,
    /// In the order the newest snapshot holding each records them.
    pub matches: Vec<Match>,
    /// The cap that ended it, if one did.
    pub stopped: Option<SearchStop>,
}

#[cfg(test)]
mod tests;
