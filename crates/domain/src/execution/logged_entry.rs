//! The entry of a snapshot a restore or a download read, as the operation
//! log names it (§9, #847): so a person looking back at the history can tell
//! which file or folder was restored or downloaded.

use serde::{Deserialize, Serialize};

use crate::browse::{DisplayName, FolderPath};
use crate::secrets::Redactor;

/// The longest entry the log names, in bytes. A path may name 256 folders of
/// 4 KiB each ([`crate::browse::MAX_FOLDER_DEPTH`]); the log keeps its end,
/// which names the entry itself.
pub const MAX_LOGGED_ENTRY_BYTES: usize = 1024;

/// What starts an entry the log cut short: the start it left out.
const CUT_MARK: &str = "…";

/// An entry of a snapshot as the operation log keeps it: its path from the
/// snapshot's top as a person reads it, masked of the repository's secrets
/// (I4), at most [`MAX_LOGGED_ENTRY_BYTES`] long.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LoggedEntry(String);

impl LoggedEntry {
    /// `path` as the log names it: `/`, then its names as the file system
    /// wrote them ([`crate::browse::EntryName::unescaped`]) joined by `/`, so
    /// the snapshot's top is `/`; every secret `redactor` knows masked; every
    /// character that would break the line or reorder it made visible, as
    /// the snapshot's tree shows a name ([`DisplayName`]), and masked again,
    /// so a secret that spells such a character's shown form stays hidden;
    /// and a path longer than [`MAX_LOGGED_ENTRY_BYTES`] cut to its end, on a
    /// character boundary, after `…`.
    pub fn of(path: &FolderPath, redactor: &Redactor) -> Self {
        let mut written = String::from("/");
        for (at, name) in path.names().iter().enumerate() {
            if at > 0 {
                written.push('/');
            }
            written.push_str(&name.unescaped());
        }
        let visible = DisplayName::new(&redactor.redact(&written));
        let shown = redactor.redact(visible.as_str());
        if shown.len() <= MAX_LOGGED_ENTRY_BYTES {
            return Self(shown);
        }
        let room = MAX_LOGGED_ENTRY_BYTES.saturating_sub(CUT_MARK.len());
        let start = (shown.len().saturating_sub(room)..shown.len())
            .find(|at| shown.is_char_boundary(*at))
            .unwrap_or(shown.len());
        Self(format!(
            "{CUT_MARK}{}",
            shown.get(start..).unwrap_or_default()
        ))
    }

    /// The entry as the log kept it, read back.
    pub fn kept(text: String) -> Self {
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{REDACTED, SecretValue};

    fn path(text: &str) -> FolderPath {
        FolderPath::parse(text).expect("a path")
    }

    fn masking(secret: &str) -> Redactor {
        Redactor::new(&[SecretValue::new(secret)])
    }

    #[test]
    fn an_entry_is_named_by_its_path_from_the_snapshots_top() {
        let none = Redactor::new(&[]);
        assert_eq!(
            LoggedEntry::of(&path("home/docs/notes.md"), &none).as_str(),
            "/home/docs/notes.md"
        );
        assert_eq!(LoggedEntry::of(&FolderPath::top(), &none).as_str(), "/");
    }

    #[test]
    fn a_secret_in_the_path_is_masked() {
        let entry = LoggedEntry::of(&path("home/canary-pass/notes.md"), &masking("canary-pass"));
        assert_eq!(entry.as_str(), format!("/home/{REDACTED}/notes.md"));
    }

    #[test]
    fn a_secret_across_names_is_masked_whole() {
        let entry = LoggedEntry::of(&path("home/canary/pass"), &masking("canary/pass"));
        assert_eq!(entry.as_str(), format!("/home/{REDACTED}"));
    }

    #[test]
    fn a_redactor_that_masks_everything_masks_the_whole_path() {
        let entry = LoggedEntry::of(&path("home/docs"), &Redactor::masking_everything());
        assert_eq!(entry.as_str(), REDACTED);
    }

    #[test]
    fn a_character_that_would_break_the_line_is_made_visible() {
        // rustic records a line feed and its kin escaped, every other
        // character as it is.
        let key = format!(r"home/a\nb\a\b\f\r\t\v{}c{}", '\u{202e}', '\u{1}');
        let entry = LoggedEntry::of(&path(&key), &Redactor::new(&[]));
        assert_eq!(
            entry.as_str(),
            "/home/a\u{240a}b\u{2407}\u{2408}\u{240c}\u{240d}\u{2409}\u{240b}[U+202E]c\u{2401}"
        );
    }

    #[test]
    fn an_escaped_name_is_named_as_the_file_system_wrote_it() {
        let none = Redactor::new(&[]);
        let named = |key: &str| LoggedEntry::of(&path(key), &none).as_str().to_owned();
        assert_eq!(named(r#"home/say \"hi\".txt"#), r#"/home/say "hi".txt"#);
        assert_eq!(named(r"back\\slash"), r"/back\slash");
        assert_eq!(named(r"it\'s \`here\`"), "/it's `here`");
        assert_eq!(named(r"caf\xc3\xA9"), "/café");
        assert_eq!(named(r"caf\u00e9 \U0001F600"), "/café \u{1f600}");
        assert_eq!(named(r"not\xffutf8"), "/not\u{fffd}utf8");
    }

    #[test]
    fn an_escape_rustic_never_writes_is_kept_as_written() {
        let none = Redactor::new(&[]);
        for key in [
            r#"say \"hi\q"#,
            r"a\x4",
            r"a\xzz",
            r"a\u12",
            r"a\uzzzz",
            r"a\ud800",
            r"a\U0011ffff",
            r"trailing\",
        ] {
            assert_eq!(
                LoggedEntry::of(&path(key), &none).as_str(),
                format!("/{key}"),
                "{key}"
            );
        }
    }

    #[test]
    fn a_secret_rustic_escapes_is_masked() {
        // rustic escapes the quote and leaves the control as it is.
        let key = format!(r#"home/p\"w{}/notes.md"#, '\u{1}');
        let entry = LoggedEntry::of(&path(&key), &masking("p\"w\u{1}"));
        assert_eq!(entry.as_str(), format!("/home/{REDACTED}/notes.md"));
    }

    #[test]
    fn a_secret_a_shown_name_would_spell_is_masked() {
        // A line feed is shown as its picture: a secret holding that picture
        // is masked in the name as shown, too.
        let entry = LoggedEntry::of(&path(r"home/a\nb"), &masking("a\u{240a}b"));
        assert_eq!(entry.as_str(), format!("/home/{REDACTED}"));
    }

    #[test]
    fn a_long_path_keeps_its_end_and_says_it_was_cut() {
        let folder = "f".repeat(300);
        let long = format!("{folder}/{folder}/{folder}/{folder}/notes.md");
        let entry = LoggedEntry::of(&path(&long), &Redactor::new(&[]));
        assert!(
            entry.as_str().len() <= MAX_LOGGED_ENTRY_BYTES,
            "{}",
            entry.as_str().len()
        );
        assert!(entry.as_str().starts_with('…'), "{}", entry.as_str());
        assert!(entry.as_str().ends_with(&format!("{folder}/notes.md")));
        let kept = entry.as_str().len() - '…'.len_utf8();
        assert_eq!(kept, MAX_LOGGED_ENTRY_BYTES - '…'.len_utf8());
    }

    #[test]
    fn a_path_exactly_at_the_limit_is_kept_whole() {
        // "/" and the name come to the limit exactly.
        let name = "a".repeat(MAX_LOGGED_ENTRY_BYTES - 1);
        let entry = LoggedEntry::of(&path(&name), &Redactor::new(&[]));
        assert_eq!(entry.as_str(), format!("/{name}"));
    }

    #[test]
    fn a_long_path_is_cut_between_characters_never_inside_one() {
        // Each "é" is two bytes; the limit falls inside one of them.
        let name = "é".repeat(MAX_LOGGED_ENTRY_BYTES);
        let entry = LoggedEntry::of(&path(&name), &Redactor::new(&[]));
        assert!(entry.as_str().len() <= MAX_LOGGED_ENTRY_BYTES);
        assert!(entry.as_str().starts_with('…'));
        assert!(
            entry.as_str().chars().skip(1).all(|c| c == 'é'),
            "{}",
            entry.as_str()
        );
        // The most whole characters that fit after the mark are kept.
        let room = MAX_LOGGED_ENTRY_BYTES - '…'.len_utf8();
        assert_eq!(entry.as_str().chars().count() - 1, room / 2);
    }
}
