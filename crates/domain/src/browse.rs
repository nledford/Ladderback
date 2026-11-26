//! A snapshot's tree, browsed one folder at a time (§3.1, "Snapshot tree";
//! §16, decision 102). An entry is named as rustic records it, so a name is
//! the key the next request opens it by; a folder is read in pages, each
//! continuing from the position the last one stopped at.

use serde::{Deserialize, Serialize};

use crate::Timestamp;

/// The most entries one page of a folder holds. A folder is an input-derived
/// size (§13.5): a hostile or merely huge one never sizes an answer.
pub const PAGE_ENTRIES: usize = 1_000;

/// The longest entry name, in bytes. No file system writes one this long
/// (255 bytes, or 255 UTF-16 units, at most), even in rustic's escaped form,
/// which spends up to four bytes on a byte that isn't UTF-8.
pub const MAX_ENTRY_NAME_BYTES: usize = 4 * 1024;

/// The deepest folder a path names. Deeper than any tree a backup makes; a
/// bound so a request can't make the engine walk without end.
pub const MAX_FOLDER_DEPTH: usize = 256;

/// One entry's name in its folder, as rustic records it: its escaped form,
/// which is valid UTF-8 whatever bytes the file system had. Never empty and
/// never holding `/`, so a path is its names joined by `/`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EntryName(String);

/// Why a text is no entry name, or no folder path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidName {
    /// The name is empty: a path with `//`, or one that starts or ends
    /// with `/`.
    Empty,
    /// The name holds a `/`.
    Slash,
    /// The name is longer than [`MAX_ENTRY_NAME_BYTES`].
    TooLong,
    /// The path names more than [`MAX_FOLDER_DEPTH`] folders.
    TooDeep,
}

impl std::fmt::Display for InvalidName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "an entry name is empty",
            Self::Slash => "an entry name holds a slash",
            Self::TooLong => "an entry name is longer than any file system writes",
            Self::TooDeep => "a folder path is deeper than any backup makes",
        })
    }
}

impl EntryName {
    /// `name`, when it can name an entry.
    pub fn parse(name: &str) -> Result<Self, InvalidName> {
        if name.is_empty() {
            Err(InvalidName::Empty)
        } else if name.contains('/') {
            Err(InvalidName::Slash)
        } else if name.len() > MAX_ENTRY_NAME_BYTES {
            Err(InvalidName::TooLong)
        } else {
            Ok(Self(name.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name as the file system wrote it: rustic's escaped form undone,
    /// as rustic's `unescape_filename` reads it, and a byte that isn't
    /// UTF-8 as U+FFFD, as `rustic ls` prints it. Not yet made visible: see
    /// [`DisplayName`]. A name holding an escape rustic can't read, which no
    /// snapshot records, is kept as it is written.
    pub fn unescaped(&self) -> String {
        unescape(&self.0).map_or_else(
            || self.0.clone(),
            |bytes| String::from_utf8_lossy(&bytes).into_owned(),
        )
    }
}

/// The bytes `name` escapes, as rustic's `unescape_filename` reads them;
/// `None` for an escape it refuses.
fn unescape(name: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(name.len());
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        match chars.next()? {
            '\\' => bytes.push(b'\\'),
            '"' => bytes.push(b'"'),
            '\'' => bytes.push(b'\''),
            '`' => bytes.push(b'`'),
            'a' => bytes.push(0x07),
            'b' => bytes.push(0x08),
            'f' => bytes.push(0x0c),
            'n' => bytes.push(b'\n'),
            'r' => bytes.push(b'\r'),
            't' => bytes.push(b'\t'),
            'v' => bytes.push(0x0b),
            'x' => bytes.push(u8::from_str_radix(&digits(&mut chars, 2)?, 16).ok()?),
            escape @ ('u' | 'U') => {
                let count = if escape == 'u' { 4 } else { 8 };
                let code = u32::from_str_radix(&digits(&mut chars, count)?, 16).ok()?;
                let unicode = char::from_u32(code)?;
                bytes.extend_from_slice(unicode.encode_utf8(&mut [0; 4]).as_bytes());
            }
            _ => return None,
        }
    }
    Some(bytes)
}

/// The next `count` characters of `chars`, when there are as many.
fn digits(chars: &mut std::str::Chars<'_>, count: usize) -> Option<String> {
    let taken: String = chars.take(count).collect();
    (taken.chars().count() == count).then_some(taken)
}

impl TryFrom<String> for EntryName {
    type Error = InvalidName;

    fn try_from(name: String) -> Result<Self, InvalidName> {
        Self::parse(&name)
    }
}

impl From<EntryName> for String {
    fn from(name: EntryName) -> Self {
        name.0
    }
}

/// One entry's name as a person reads it: rustic's escaped form undone, a
/// byte that isn't UTF-8 as U+FFFD, as `rustic ls` prints it, and every
/// character that would break the line or reorder it made visible (§16,
/// decision 102): a C0 control as its Control Picture (U+2400–U+241F, a
/// line feed as `␊`), DEL as `␡`, and a C1 control, a bidirectional control
/// (U+061C, U+200E, U+200F, U+202A–U+202E, U+2066–U+2069) or a line or
/// paragraph separator as its code point in brackets (`[U+202E]`). So two
/// names that differ only in such a character still look different. It
/// names nothing: a request opens an entry by its [`EntryName`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct DisplayName(String);

impl DisplayName {
    /// `name`, as the file system wrote it, with what can't be shown as it
    /// is made visible.
    pub fn new(name: &str) -> Self {
        use std::fmt::Write as _;

        let mut shown = String::with_capacity(name.len());
        for c in name.chars() {
            match c {
                '\u{0}'..='\u{1f}' => shown.push(control_picture(c)),
                '\u{7f}' => shown.push('\u{2421}'),
                '\u{80}'..='\u{9f}'
                | '\u{61c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}' => {
                    // Writing to a String can't fail.
                    let _ = write!(shown, "[U+{:04X}]", u32::from(c));
                }
                _ => shown.push(c),
            }
        }
        Self(shown)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The Control Picture of a C0 control: U+2400 plus the control's code.
fn control_picture(control: char) -> char {
    char::from_u32(0x2400 + u32::from(control)).unwrap_or(char::REPLACEMENT_CHARACTER)
}

/// Any text becomes a name to show, so a worker's answer is made visible
/// again as the daemon reads it.
impl From<String> for DisplayName {
    fn from(name: String) -> Self {
        Self::new(&name)
    }
}

impl From<DisplayName> for String {
    fn from(name: DisplayName) -> Self {
        name.0
    }
}

/// A folder of a snapshot, by the names that lead to it from the snapshot's
/// top. The top itself names none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FolderPath(Vec<EntryName>);

impl FolderPath {
    /// The snapshot's top folder.
    pub fn top() -> Self {
        Self::default()
    }

    /// The folder `path` names, its names joined by `/`: `""` is the top.
    pub fn parse(path: &str) -> Result<Self, InvalidName> {
        if path.is_empty() {
            return Ok(Self::top());
        }
        // Counted before a name is copied, so a hostile path costs no more
        // than the deepest one.
        if path.split('/').nth(MAX_FOLDER_DEPTH).is_some() {
            return Err(InvalidName::TooDeep);
        }
        path.split('/')
            .map(EntryName::parse)
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// The folder's names, from the top down.
    pub fn names(&self) -> &[EntryName] {
        &self.0
    }

    /// The folder `name` within this one, unless that is past the deepest
    /// a path names.
    pub fn join(&self, name: EntryName) -> Result<Self, InvalidName> {
        if self.0.len() >= MAX_FOLDER_DEPTH {
            return Err(InvalidName::TooDeep);
        }
        let mut names = self.0.clone();
        names.push(name);
        Ok(Self(names))
    }
}

impl std::fmt::Display for FolderPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (at, name) in self.0.iter().enumerate() {
            if at > 0 {
                f.write_str("/")?;
            }
            f.write_str(name.as_str())?;
        }
        Ok(())
    }
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    Folder,
    File,
    /// A symbolic link, recorded as a link, never followed.
    Link,
    /// A device, a pipe or a socket.
    Other,
}

/// One entry of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The name a request opens the entry by.
    pub name: EntryName,
    /// The name a person reads.
    pub display: DisplayName,
    pub kind: EntryKind,
    /// A file's size in bytes, as rustic recorded it; `None` for anything
    /// else.
    pub size: Option<u64>,
    /// When the entry was last modified, when rustic recorded it.
    pub modified: Option<Timestamp>,
}

/// One page of a folder's entries, in the folder's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderPage {
    pub entries: Vec<Entry>,
    /// How many entries the folder holds in all.
    pub total: u64,
    /// The position the next page starts at; `None` on the folder's last.
    pub next: Option<u64>,
}

impl FolderPage {
    /// The page of `entries` read from position `from`, having gone through
    /// `consumed` positions of a folder of `total`: more follow while the
    /// position reached is short of the total. An entry left out of a page
    /// (a name too long to carry) still takes its position.
    pub fn continuing(entries: Vec<Entry>, from: u64, consumed: u64, total: u64) -> Self {
        let reached = from.saturating_add(consumed);
        Self {
            entries,
            total,
            next: (reached < total).then_some(reached),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> EntryName {
        EntryName::parse(text).expect("a name")
    }

    #[test]
    fn a_name_is_kept_as_rustic_wrote_it() {
        assert_eq!(name("notes.md").as_str(), "notes.md");
        // rustic's escaped form of a name that isn't UTF-8 is a name too.
        assert_eq!(name("caf\\xe9").as_str(), "caf\\xe9");
        assert_eq!(name("..").as_str(), "..");
    }

    /// A name read from JSON that is no name is refused saying why.
    #[test]
    fn a_name_read_from_json_that_is_no_name_says_why() {
        let refused = serde_json::from_str::<EntryName>(r#""a/b""#)
            .expect_err("a slash is no name")
            .to_string();
        assert!(refused.contains("an entry name holds a slash"), "{refused}");
    }

    #[test]
    fn a_name_is_never_empty_and_never_holds_a_slash() {
        assert_eq!(EntryName::parse(""), Err(InvalidName::Empty));
        assert_eq!(EntryName::parse("a/b"), Err(InvalidName::Slash));
    }

    #[test]
    fn a_name_is_at_most_the_longest_a_name_can_be() {
        let longest = "x".repeat(MAX_ENTRY_NAME_BYTES);
        assert_eq!(name(&longest).as_str(), longest);
        assert_eq!(
            EntryName::parse(&"x".repeat(MAX_ENTRY_NAME_BYTES + 1)),
            Err(InvalidName::TooLong)
        );
    }

    #[test]
    fn a_name_of_four_kib_is_a_name() {
        let four_kib = "x".repeat(4_096);
        assert_eq!(EntryName::parse(&four_kib).map(String::from), Ok(four_kib));
    }

    #[test]
    fn a_name_is_written_as_its_key() {
        assert_eq!(String::from(name(r#"say \"hi\""#)), r#"say \"hi\""#);
    }

    #[test]
    fn the_empty_path_is_the_top_folder() {
        assert_eq!(FolderPath::parse(""), Ok(FolderPath::top()));
        assert!(FolderPath::top().names().is_empty());
    }

    #[test]
    fn a_path_is_its_names_joined_by_slashes() {
        let path = FolderPath::parse("source/docs").expect("a path");
        assert_eq!(path.names(), [name("source"), name("docs")]);
        assert_eq!(path.to_string(), "source/docs");
    }

    #[test]
    fn a_path_with_an_empty_name_names_no_folder() {
        for path in ["/source", "source/", "source//docs", "/"] {
            assert_eq!(FolderPath::parse(path), Err(InvalidName::Empty), "{path}");
        }
    }

    #[test]
    fn a_path_is_at_most_the_deepest_a_path_names() {
        let deepest = vec!["d"; MAX_FOLDER_DEPTH].join("/");
        assert_eq!(
            FolderPath::parse(&deepest).map(|path| path.names().len()),
            Ok(MAX_FOLDER_DEPTH)
        );
        let deeper = vec!["d"; MAX_FOLDER_DEPTH + 1].join("/");
        assert_eq!(FolderPath::parse(&deeper), Err(InvalidName::TooDeep));
    }

    #[test]
    fn a_folder_within_a_path_is_one_deeper_up_to_the_deepest() {
        let path = FolderPath::top().join(name("source")).expect("a folder");
        assert_eq!(path.to_string(), "source");
        let deepest = FolderPath::parse(&vec!["d"; MAX_FOLDER_DEPTH].join("/")).expect("a path");
        assert_eq!(deepest.join(name("e")), Err(InvalidName::TooDeep));
    }

    #[test]
    fn a_page_short_of_the_folders_end_says_where_the_next_starts() {
        let page = FolderPage::continuing(Vec::new(), 1_000, 1_000, 2_345);
        assert_eq!(page.next, Some(2_000));
        assert_eq!(page.total, 2_345);
    }

    #[test]
    fn the_page_that_reaches_the_folders_end_is_its_last() {
        assert_eq!(
            FolderPage::continuing(Vec::new(), 2_000, 345, 2_345).next,
            None
        );
        assert_eq!(FolderPage::continuing(Vec::new(), 0, 0, 0).next, None);
    }

    #[test]
    fn a_page_asked_past_the_folders_end_is_its_last() {
        assert_eq!(
            FolderPage::continuing(Vec::new(), 5_000, 0, 2_345).next,
            None
        );
        assert_eq!(
            FolderPage::continuing(Vec::new(), u64::MAX, u64::MAX, 3).next,
            None
        );
    }

    fn shown(text: &str) -> String {
        DisplayName::new(text).as_str().to_owned()
    }

    #[test]
    fn a_name_without_a_hidden_character_is_shown_as_it_is() {
        for name in [
            "say \"hi\".txt",
            "back\\slash.txt",
            "café.txt",
            "bad\u{fffd}byte.txt",
        ] {
            assert_eq!(shown(name), name);
        }
    }

    #[test]
    fn a_c0_control_or_del_is_shown_as_its_control_picture() {
        assert_eq!(shown("new\nline.txt"), "new\u{240a}line.txt");
        assert_eq!(shown("tab\there.txt"), "tab\u{2409}here.txt");
        assert_eq!(shown("\u{0}\u{1b}\u{1f}"), "\u{2400}\u{241b}\u{241f}");
        assert_eq!(shown("a\u{7f}b"), "a\u{2421}b");
    }

    #[test]
    fn a_c1_bidirectional_or_separator_character_is_shown_as_its_code_point() {
        assert_eq!(shown("abc\u{202e}txt.exe"), "abc[U+202E]txt.exe");
        for (hidden, code) in [
            ('\u{80}', "[U+0080]"),
            ('\u{85}', "[U+0085]"),
            ('\u{9f}', "[U+009F]"),
            ('\u{61c}', "[U+061C]"),
            ('\u{200e}', "[U+200E]"),
            ('\u{200f}', "[U+200F]"),
            ('\u{2028}', "[U+2028]"),
            ('\u{2029}', "[U+2029]"),
            ('\u{202a}', "[U+202A]"),
            ('\u{2066}', "[U+2066]"),
            ('\u{2069}', "[U+2069]"),
        ] {
            assert_eq!(
                shown(&format!("a{hidden}b")),
                format!("a{code}b"),
                "{hidden:?}"
            );
        }
        // Their neighbors are shown as they are.
        for kept in [
            '\u{a0}', '\u{200d}', '\u{2027}', '\u{202f}', '\u{2065}', '\u{206a}', '\u{61b}',
        ] {
            assert_eq!(shown(&format!("a{kept}b")), format!("a{kept}b"), "{kept:?}");
        }
    }

    #[test]
    fn names_that_differ_only_in_a_hidden_character_are_shown_differently() {
        for hidden in ['\n', '\u{7f}', '\u{85}', '\u{202e}', '\u{200f}'] {
            assert_ne!(shown(&format!("ab{hidden}c")), shown("abc"), "{hidden:?}");
            assert_ne!(
                shown(&format!("ab{hidden}c")),
                shown(&format!("a{hidden}bc"))
            );
        }
    }

    #[test]
    fn a_name_read_back_from_a_worker_is_made_visible_again() {
        assert_eq!(
            DisplayName::from("new\nline".to_owned()),
            DisplayName::new("new\nline")
        );
        assert_eq!(
            String::from(DisplayName::new("new\nline")),
            "new\u{240a}line"
        );
        let shown_twice = DisplayName::new(DisplayName::new("a\u{202e}\n").as_str());
        assert_eq!(shown_twice, DisplayName::new("a\u{202e}\n"));
        // As the daemon decodes a worker's answer.
        let decoded: DisplayName = serde_json::from_str(r#""a\u202eb\n""#).expect("a JSON string");
        assert_eq!(decoded.as_str(), "a[U+202E]b\u{240a}");
    }
}
