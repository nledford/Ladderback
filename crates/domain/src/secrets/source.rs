//! Where a secret comes from (technical design §10.2).

use super::SecretValue;
use super::command::CommandLine;

/// One of the five sources a secret-bearing field can name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretRef {
    /// `password-command`: the first line of what the command prints.
    Command(CommandLine),
    /// `password-keyring = "service/name"`: an entry of the OS keyring.
    Keyring(KeyringEntry),
    /// `password-file`, and `${file:/path}` in a repository field.
    File { path: String, read: FileRead },
    /// `password-env`, and `${VAR}` in a repository field: the variable's
    /// value, as it is.
    Env(String),
    /// The secret itself, written in the config.
    Inline(SecretValue),
}

impl SecretRef {
    /// The secret itself when the config holds it. The daemon's redactor is
    /// seeded with these when the config loads, so they can't reach a log
    /// before any job runs (§10.5).
    pub fn inline(&self) -> Option<&SecretValue> {
        todo!("the secret itself, when it is inline")
    }

    /// What names this source in an error. `None` for an inline secret,
    /// which is its own value and can't fail to resolve.
    pub fn source_name(&self) -> Option<SourceName> {
        todo!("what names the source in an error")
    }
}

/// Which source an error is about and where to find it: a variable, a path,
/// a program, a keyring entry. Never a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceName {
    Command { program: String },
    Keyring { entry: String },
    File { path: String },
    Env { name: String },
}

/// How much of a file is the secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileRead {
    /// The first line, as rustic reads `password-file`.
    FirstLine,
    /// All of it, minus one trailing newline: `${file:/path}`.
    Whole,
}

impl FileRead {
    pub fn extract(self, _contents: &str) -> &str {
        todo!("the secret in the file's contents")
    }
}

/// The first line of `text` without its line ending, as rustic reads a
/// password from a file or from a command's output: up to the first `\n`,
/// minus a `\r` before it.
pub fn first_line(_text: &str) -> &str {
    todo!("the first line of the text")
}

/// An entry of the OS keyring: `password-keyring = "service/name"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyringEntry {
    service: String,
    name: String,
}

/// A `password-keyring` that isn't `service/name`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotAKeyringEntry;

impl std::fmt::Display for NotAKeyringEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for NotAKeyringEntry {}

impl KeyringEntry {
    /// Splits at the first `/`; the service and the name may not be empty,
    /// and the name may hold more slashes.
    pub fn parse(_text: &str) -> Result<Self, NotAKeyringEntry> {
        todo!("a keyring entry from service/name")
    }

    pub fn service(&self) -> &str {
        todo!("the service")
    }

    pub fn name(&self) -> &str {
        todo!("the name")
    }
}

impl std::fmt::Display for KeyringEntry {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!("service/name")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_file_gives_its_first_line_without_the_line_ending() {
        for (contents, expected) in [
            ("hunter2\n", "hunter2"),
            ("hunter2\r\n", "hunter2"),
            ("hunter2", "hunter2"),
            ("hunter2\nsecond\nthird\n", "hunter2"),
            ("hunter2\r\nsecond", "hunter2"),
            ("", ""),
            ("\nhunter2", ""),
            ("  hunter2 \n", "  hunter2 "),
            // rustic strips a `\r` only before a `\n`.
            ("hunter2\r", "hunter2\r"),
            ("a\r\r\nb", "a\r"),
        ] {
            assert_eq!(
                FileRead::FirstLine.extract(contents),
                expected,
                "{contents:?}"
            );
            assert_eq!(first_line(contents), expected, "{contents:?}");
        }
    }

    #[test]
    fn a_whole_file_loses_one_trailing_newline_and_nothing_else() {
        for (contents, expected) in [
            ("hunter2\n", "hunter2"),
            ("hunter2\r\n", "hunter2"),
            ("hunter2", "hunter2"),
            ("a\nb\n", "a\nb"),
            ("a\nb", "a\nb"),
            ("a\n\n", "a\n"),
            ("a\r\n\r\n", "a\r\n"),
            ("\n", ""),
            ("", ""),
            (" a \n", " a "),
            ("a\r", "a\r"),
            ("a\n\r", "a\n\r"),
        ] {
            assert_eq!(FileRead::Whole.extract(contents), expected, "{contents:?}");
        }
    }

    #[test]
    fn a_keyring_entry_is_a_service_and_a_name() {
        let entry = KeyringEntry::parse("steading/nas").unwrap();
        assert_eq!(entry.service(), "steading");
        assert_eq!(entry.name(), "nas");
        assert_eq!(entry.to_string(), "steading/nas");
        // Only the first slash separates.
        let entry = KeyringEntry::parse("steading/nas/backup").unwrap();
        assert_eq!(entry.service(), "steading");
        assert_eq!(entry.name(), "nas/backup");
    }

    #[test]
    fn a_keyring_entry_needs_both_parts() {
        for text in ["", "steading", "/nas", "steading/", "/"] {
            assert_eq!(KeyringEntry::parse(text), Err(NotAKeyringEntry), "{text:?}");
        }
    }

    #[test]
    fn only_an_inline_secret_is_held_by_the_config() {
        let inline = SecretRef::Inline(SecretValue::new("hunter2"));
        assert_eq!(inline.inline().map(SecretValue::expose), Some("hunter2"));
        for other in [
            SecretRef::Env("NAS_PASSWORD".to_owned()),
            SecretRef::File {
                path: "/run/secrets/nas".to_owned(),
                read: FileRead::FirstLine,
            },
            SecretRef::Keyring(KeyringEntry::parse("steading/nas").unwrap()),
            SecretRef::Command(CommandLine::parse("pass show nas").unwrap()),
        ] {
            assert_eq!(other.inline(), None, "{other:?}");
        }
    }

    #[test]
    fn a_source_is_named_by_where_it_is_and_never_by_what_it_holds() {
        let named = |secret: SecretRef| secret.source_name();
        assert_eq!(
            named(SecretRef::Env("NAS_PASSWORD".to_owned())),
            Some(SourceName::Env {
                name: "NAS_PASSWORD".to_owned()
            })
        );
        assert_eq!(
            named(SecretRef::File {
                path: "/run/secrets/nas".to_owned(),
                read: FileRead::Whole,
            }),
            Some(SourceName::File {
                path: "/run/secrets/nas".to_owned()
            })
        );
        assert_eq!(
            named(SecretRef::Keyring(
                KeyringEntry::parse("steading/nas").unwrap()
            )),
            Some(SourceName::Keyring {
                entry: "steading/nas".to_owned()
            })
        );
        // The program, not its arguments: `echo hunter2` would leak.
        assert_eq!(
            named(SecretRef::Command(
                CommandLine::parse("echo hunter2").unwrap()
            )),
            Some(SourceName::Command {
                program: "echo".to_owned()
            })
        );
        assert_eq!(named(SecretRef::Inline(SecretValue::new("hunter2"))), None);
    }
}
