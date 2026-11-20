//! Snapshots on the wire: one snapshot, a repository's list of them, and the
//! folders and entries a snapshot holds.

use serde::{Deserialize, Serialize};

/// How many paths one listed snapshot may carry. An input-derived size,
/// capped (§13.5): a snapshot lists at most a handful of roots.
pub const MAX_SNAPSHOT_PATHS: usize = 64;

/// How many tags one listed snapshot may carry. An input-derived size,
/// capped (§13.5) like the paths.
pub const MAX_SNAPSHOT_TAGS: usize = 64;

/// One snapshot of a repository's list, as the daemon tells the web UI:
/// what a list view needs — id, when it was taken (RFC 3339) and by a clock
/// at which offset, which host, its label, tags and paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SnapshotDto {
    /// 64 hex digits, as rustic writes a snapshot's ID.
    pub id: String,
    /// RFC 3339, UTC.
    pub time: String,
    /// The offset of the clock that took the snapshot: with `time`, the
    /// snapshot time (§3.1), whose day is the one a calendar retention rule
    /// counts the snapshot on (§16, decision 56). Absent from a daemon older
    /// than the field, which said only the instant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<ClockOffset>,
    pub hostname: String,
    /// Empty when the snapshot carries no label.
    pub label: String,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    /// The snapshot has more paths than `paths` lists, or one of them is cut
    /// short: a list shows at most [`MAX_SNAPSHOT_PATHS`] of them, each of a
    /// bounded length, and says so here instead of leaving them out silently.
    #[serde(default)]
    pub paths_truncated: bool,
    /// The same for `tags`, which a list shows at most [`MAX_SNAPSHOT_TAGS`]
    /// of.
    #[serde(default)]
    pub tags_truncated: bool,
}

/// The offset from UTC of the clock that took a snapshot. On the wire it is
/// written as RFC 3339 writes an offset, `-06:00` or `+00:00`, with seconds
/// (`+00:19:32`) only when it has some, and read back only in that form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(try_from = "String", into = "String")]
pub struct ClockOffset(jiff::tz::Offset);

/// The text is not an offset as RFC 3339 writes one, or is beyond the range
/// of an offset (±25:59:59).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidOffset;

impl std::fmt::Display for InvalidOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not an offset from UTC")
    }
}

impl std::error::Error for InvalidOffset {}

impl ClockOffset {
    pub const fn new(offset: jiff::tz::Offset) -> Self {
        Self(offset)
    }

    pub const fn get(self) -> jiff::tz::Offset {
        self.0
    }
}

impl std::str::FromStr for ClockOffset {
    type Err = InvalidOffset;

    /// `±HH:MM` or `±HH:MM:SS`, nothing else: no `Z`, no zone name, no
    /// space.
    fn from_str(text: &str) -> Result<Self, InvalidOffset> {
        let bytes = text.as_bytes();
        let sign = match bytes.first() {
            Some(b'+') => 1,
            Some(b'-') => -1,
            _ => return Err(InvalidOffset),
        };
        // Two digits at `at`, at most `max`.
        let pair = |at: usize, max: i32| -> Result<i32, InvalidOffset> {
            match (bytes.get(at), bytes.get(at + 1)) {
                (Some(&high), Some(&low)) if high.is_ascii_digit() && low.is_ascii_digit() => {
                    let value = i32::from(high - b'0') * 10 + i32::from(low - b'0');
                    if value <= max {
                        Ok(value)
                    } else {
                        Err(InvalidOffset)
                    }
                }
                _ => Err(InvalidOffset),
            }
        };
        let seconds = match bytes.len() {
            6 | 9 if bytes.get(3) == Some(&b':') => {
                let mut seconds = pair(1, 25)? * 3600 + pair(4, 59)? * 60;
                if bytes.len() == 9 {
                    if bytes.get(6) != Some(&b':') {
                        return Err(InvalidOffset);
                    }
                    seconds += pair(7, 59)?;
                }
                seconds
            }
            _ => return Err(InvalidOffset),
        };
        jiff::tz::Offset::from_seconds(sign * seconds)
            .map(Self)
            .map_err(|_| InvalidOffset)
    }
}

impl std::fmt::Display for ClockOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = self.0.seconds();
        let sign = if seconds < 0 { '-' } else { '+' };
        let magnitude = seconds.unsigned_abs();
        let (hours, minutes, rest) = (magnitude / 3600, magnitude / 60 % 60, magnitude % 60);
        write!(f, "{sign}{hours:02}:{minutes:02}")?;
        if rest != 0 {
            write!(f, ":{rest:02}")?;
        }
        Ok(())
    }
}

impl TryFrom<String> for ClockOffset {
    type Error = InvalidOffset;

    fn try_from(text: String) -> Result<Self, InvalidOffset> {
        text.parse()
    }
}

impl From<ClockOffset> for String {
    fn from(offset: ClockOffset) -> String {
        offset.to_string()
    }
}

/// Why a snapshot DTO can't be: an ID that isn't 64 hex digits, a time that
/// isn't RFC 3339, or more paths than [`MAX_SNAPSHOT_PATHS`] or tags than
/// [`MAX_SNAPSHOT_TAGS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidSnapshot {
    Id,
    Time,
    Paths,
    Tags,
}

impl std::fmt::Display for InvalidSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvalidSnapshot {}

impl SnapshotDto {
    /// Builds the DTO from what the engine reported, refusing an ID that
    /// isn't 64 hex digits, a time that isn't RFC 3339, and more paths than
    /// [`MAX_SNAPSHOT_PATHS`] or tags than [`MAX_SNAPSHOT_TAGS`] (input-derived
    /// sizes, capped). The truncation flags start false: the engine's list
    /// says which snapshots it cut, and [`SnapshotDto::truncated`] carries that
    /// over.
    pub fn new(
        id: String,
        time: String,
        hostname: String,
        label: String,
        tags: Vec<String>,
        paths: Vec<String>,
    ) -> Result<Self, InvalidSnapshot> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(InvalidSnapshot::Id);
        }
        if time.parse::<jiff::Timestamp>().is_err() {
            return Err(InvalidSnapshot::Time);
        }
        if paths.len() > MAX_SNAPSHOT_PATHS {
            return Err(InvalidSnapshot::Paths);
        }
        if tags.len() > MAX_SNAPSHOT_TAGS {
            return Err(InvalidSnapshot::Tags);
        }
        Ok(Self {
            id,
            time,
            offset: None,
            hostname,
            label,
            tags,
            paths,
            paths_truncated: false,
            tags_truncated: false,
        })
    }

    /// The snapshot with what the engine reported it cut: some of its paths,
    /// some of its tags.
    #[must_use]
    pub fn truncated(mut self, paths: bool, tags: bool) -> Self {
        self.paths_truncated = paths;
        self.tags_truncated = tags;
        self
    }

    /// The snapshot with the offset of the clock that took it, which the
    /// engine read from the snapshot file.
    #[must_use]
    pub fn taken_at_offset(mut self, offset: jiff::tz::Offset) -> Self {
        self.offset = Some(ClockOffset::new(offset));
        self
    }
}

/// One repository's snapshots, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotsDto {
    pub snapshots: Vec<SnapshotDto>,
}

/// The most entries one [`FolderDto`] lists: a folder is read in pages
/// (§16, decision 102), and a page is an input-derived size, capped (§13.5).
pub const MAX_FOLDER_ENTRIES: usize = 1_000;

/// One page of one folder of a snapshot, in the order the snapshot records
/// its entries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct FolderDto {
    /// The folder's path in the snapshot, its names joined by `/`; empty
    /// for the snapshot's top folder.
    pub path: String,
    /// At most [`MAX_FOLDER_ENTRIES`].
    pub entries: Vec<EntryDto>,
    /// How many entries the folder holds in all.
    pub total: u64,
    /// The position the next page starts at, sent back as `from`; absent
    /// on the folder's last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
}

/// One entry of a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct EntryDto {
    /// The name as rustic records it: the key a request opens the entry by,
    /// never shown. A byte the file system's name had that isn't UTF-8 is
    /// written as `\xNN`, and a backslash, a quote and the controls
    /// `\a\b\f\n\r\t\v` escaped as Go's `strconv.Quote` does.
    pub name: String,
    /// The name as a person reads it: as the file system wrote it, a byte
    /// that isn't UTF-8 as U+FFFD, and a control, bidirectional or
    /// separator character made visible (`domain::browse::DisplayName`).
    pub display: String,
    pub kind: EntryKindDto,
    /// A file's size in bytes; absent for anything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// When the entry was last modified, RFC 3339 in UTC, when the snapshot
    /// recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum EntryKindDto {
    Folder,
    File,
    /// A symbolic link, recorded as a link.
    Link,
    /// A device, a pipe or a socket.
    Other,
}

/// What a search of a repository's snapshots found: the entries whose names
/// match the pattern, each at its path with the snapshots that hold it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct FindDto {
    /// The pattern searched for, as given.
    pub pattern: String,
    /// The snapshots read to the end, newest first.
    pub searched: Vec<SearchedDto>,
    /// How many snapshots it chose from weren't read to the end: older
    /// ones past the cap, and any a cap stopped it before.
    pub left_out: u64,
    /// In the order the newest snapshot holding each records them.
    pub matches: Vec<MatchDto>,
    /// The cap that cut the search short; absent when it read every
    /// snapshot it could.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<SearchStopDto>,
}

/// A snapshot a search read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct SearchedDto {
    pub snapshot: String,
    /// RFC 3339 in UTC.
    pub time: String,
}

/// An entry whose name matched, at one path, and the snapshots it is in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct MatchDto {
    /// The entry's path from the snapshot's top, by the names rustic
    /// records (each an [`EntryDto::name`]), joined by `/`: the folder that
    /// holds it is the path without its last name.
    pub path: String,
    /// The path as a person reads it, its names made visible as
    /// [`EntryDto::display`] is.
    pub display: String,
    pub kind: EntryKindDto,
    /// Newest snapshot first.
    pub seen: Vec<SeenDto>,
}

/// One snapshot that holds a match, and the entry as it holds it there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct SeenDto {
    pub snapshot: String,
    /// A file's size in bytes; absent for anything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// RFC 3339 in UTC, when the snapshot recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// Which cap cut a search short.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SearchStopDto {
    /// The repository has more snapshots than it was told to read.
    Snapshots,
    /// It found as many matches as it was told to return, and there were
    /// more.
    Matches,
    /// Its time ran out.
    Time,
}

#[cfg(test)]
mod tests;
