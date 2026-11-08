//! What can go wrong when a secret is read, and what is worth a warning
//! although it worked. Codes and values, never sentences (§8.4): the
//! presentation layers word them.

use super::source::SourceName;
use super::{MAX_SECRET_BYTES, SecretValue};

/// A secret that couldn't be read. It names the source, so a job's failure
/// can say which reference is at fault, and never holds a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretError {
    pub source: SourceName,
    pub problem: Problem,
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for SecretError {}

impl SecretError {
    /// There is nothing at `source`.
    pub fn missing(_source: SourceName) -> Self {
        todo!("an error that says there is nothing at the source")
    }
}

/// What was wrong with a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// There is nothing there: the variable isn't set, the file doesn't
    /// exist, the keyring has no such entry. Never read as an empty secret.
    Missing,
    /// The source exists but couldn't be reached: a file that can't be read,
    /// a program that can't be started, a keyring that can't be opened. The
    /// platform's own words, as they came.
    Unreachable { detail: String },
    /// A secret has to be text, and this isn't valid UTF-8.
    NotUnicode,
    /// More than [`MAX_SECRET_BYTES`](super::MAX_SECRET_BYTES), the cap on
    /// what is read from a file or from a command's output.
    TooLarge { limit: usize },
    /// The command ran and failed: its exit status (`None` when a signal
    /// ended it) and what it wrote to stderr, cut short.
    CommandFailed { status: Option<i32>, detail: String },
    /// The command ran for longer than it may.
    TimedOut { seconds: u64 },
}

/// The most bytes of what a failed command wrote that an error keeps.
pub const MAX_DETAIL_BYTES: usize = 512;

impl Problem {
    /// A command that exited with `status` (`None` when a signal ended it)
    /// after writing `stderr`. The detail is the first line of stderr that has
    /// something on it, cut to [`MAX_DETAIL_BYTES`] at a character boundary.
    /// It is what the command chose to say: a caller masks the value the
    /// command printed on stdout (where a secret would be) in `stderr` first,
    /// and stdout is never part of the detail.
    pub fn command_failed(_status: Option<i32>, _stderr: &[u8]) -> Self {
        todo!("a failed command with the first line it said")
    }
}

/// A secret that was read, with something the user should know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretWarning {
    /// Other users of the machine can read the file (group or world read
    /// permission).
    FileReadableByOthers { path: String },
}

/// The secret that a source gave as `bytes`. Every source that reads bytes
/// goes through here, so they all draw the same limits: at most
/// [`MAX_SECRET_BYTES`], and valid UTF-8. `extract` picks the secret out of
/// the text: its first line, all of it minus a newline, or as it is.
///
/// A reader may stop at `MAX_SECRET_BYTES + 1` bytes: that is already too
/// large, and it need not read the rest.
pub fn decode(
    _source: &SourceName,
    _bytes: &[u8],
    _extract: impl FnOnce(&str) -> &str,
) -> Result<SecretValue, SecretError> {
    todo!("the secret in the bytes, within the limits")
}

/// The warning for a secret file with Unix permission bits `mode`, if it
/// deserves one: it is group- or world-readable and isn't under one of the
/// directories where a container runtime or systemd sets permissions.
/// `credentials_directory` is the value of `$CREDENTIALS_DIRECTORY`, which
/// only counts when it is a real absolute directory.
pub fn file_warning(
    _path: &str,
    _mode: u32,
    _credentials_directory: Option<&str>,
) -> Option<SecretWarning> {
    todo!("a warning for a file that others can read")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::first_line;

    fn warns(path: &str, mode: u32, credentials: Option<&str>) -> bool {
        file_warning(path, mode, credentials).is_some()
    }

    #[test]
    fn a_file_others_can_read_is_warned_about() {
        let path = "/home/demo/secrets/nas";
        assert_eq!(
            file_warning(path, 0o644, None),
            Some(SecretWarning::FileReadableByOthers {
                path: path.to_owned()
            })
        );
        // Group read alone, world read alone.
        assert!(warns(path, 0o640, None));
        assert!(warns(path, 0o604, None));
        assert!(warns(path, 0o040, None));
        assert!(warns(path, 0o004, None));
        // The type bits of a real `st_mode` don't matter.
        assert!(warns(path, 0o100_644, None));
    }

    #[test]
    fn a_file_only_its_owner_can_read_is_not() {
        let path = "/home/demo/secrets/nas";
        for mode in [0o600, 0o400, 0o700, 0o000, 0o100_600] {
            assert!(!warns(path, mode, None), "{mode:o}");
        }
    }

    /// Writable or executable by others is another matter than readable.
    #[test]
    fn only_the_read_permission_of_group_and_world_counts() {
        for mode in [0o620, 0o602, 0o610, 0o601, 0o622, 0o022, 0o011] {
            assert!(!warns("/home/demo/nas", mode, None), "{mode:o}");
        }
    }

    #[test]
    fn files_where_container_runtimes_set_permissions_are_not_warned_about() {
        for path in [
            "/run/secrets/nas_password",
            "/var/run/secrets/kubernetes.io/serviceaccount/token",
            "/run/secrets/a/b/c",
        ] {
            assert!(!warns(path, 0o644, None), "{path}");
        }
    }

    #[test]
    fn only_those_directories_and_what_is_under_them_are_exempt() {
        for path in [
            "/run/secretsx/nas",
            "/run/secret/nas",
            "/var/run/secretsx/nas",
            "/home/run/secrets/nas",
            "/tmp/run/secrets/nas",
            "run/secrets/nas",
            "/run/nas",
            "/run",
        ] {
            assert!(warns(path, 0o644, None), "{path}");
        }
    }

    #[test]
    fn a_path_that_climbs_out_of_an_exempt_directory_is_not_exempt() {
        for path in [
            "/run/secrets/../../home/demo/key",
            "/run/secrets/x/../../../etc/shadow",
            "/var/run/secrets/..",
        ] {
            assert!(warns(path, 0o644, None), "{path}");
        }
        assert!(!warns("/run/secrets/a.b..c/key", 0o644, None));
    }

    #[test]
    fn the_credentials_directory_is_exempt_when_there_is_one() {
        let directory = Some("/run/credentials/steading.service");
        assert!(!warns(
            "/run/credentials/steading.service/nas",
            0o644,
            directory
        ));
        assert!(!warns(
            "/run/credentials/steading.service/sub/nas",
            0o644,
            directory
        ));
        // Not under it, or no directory set.
        assert!(warns(
            "/run/credentials/other.service/nas",
            0o644,
            directory
        ));
        assert!(warns("/run/credentials/steading.service/nas", 0o644, None));
        // Still exempt without it: the fixed directories.
        assert!(!warns("/run/secrets/nas", 0o644, directory));
    }

    #[test]
    fn a_credentials_directory_that_is_not_a_real_directory_exempts_nothing() {
        for directory in ["", "/", "relative/dir", "//", "."] {
            assert!(
                warns("/home/demo/nas", 0o644, Some(directory)),
                "{directory:?}"
            );
            assert!(warns("/nas", 0o644, Some(directory)), "{directory:?}");
        }
        // A relative directory doesn't exempt a relative path either.
        assert!(warns("relative/dir/nas", 0o644, Some("relative/dir")));
    }

    fn file(path: &str) -> SourceName {
        SourceName::File {
            path: path.to_owned(),
        }
    }

    #[test]
    fn a_source_gives_its_secret_as_the_extraction_picks_it() {
        let source = file("/home/demo/nas");
        let first = decode(&source, b"hunter2\nsecond\n", first_line).unwrap();
        assert_eq!(first.expose(), "hunter2");
        let as_it_is = decode(&source, b"  as is \n", |text| text).unwrap();
        assert_eq!(as_it_is.expose(), "  as is \n");
        let nothing = decode(&source, b"", |text| text).unwrap();
        assert_eq!(nothing.expose(), "");
    }

    #[test]
    fn a_source_may_give_the_secret_size_and_no_more() {
        // The limit is the one the design and the scenarios give: 64 KiB.
        assert_eq!(MAX_SECRET_BYTES, 64 * 1024);
        let source = file("/home/demo/nas");
        let at_the_limit = vec![b'x'; MAX_SECRET_BYTES];
        assert_eq!(
            decode(&source, &at_the_limit, |text| text)
                .unwrap()
                .expose()
                .len(),
            MAX_SECRET_BYTES
        );
        // Too large even when the secret is the first line, which is short.
        let mut over = b"hunter2\n".to_vec();
        over.resize(MAX_SECRET_BYTES + 1, b'x');
        assert_eq!(
            decode(&source, &over, first_line),
            Err(SecretError {
                source: source.clone(),
                problem: Problem::TooLarge {
                    limit: MAX_SECRET_BYTES
                },
            })
        );
    }

    #[test]
    fn a_source_that_is_not_text_is_not_a_secret() {
        let source = SourceName::Env {
            name: "DEMO_PASSWORD".to_owned(),
        };
        assert_eq!(
            decode(&source, &[b'a', 0xff, b'b'], |text| text),
            Err(SecretError {
                source,
                problem: Problem::NotUnicode,
            })
        );
    }

    #[test]
    fn a_missing_source_is_named() {
        assert_eq!(
            SecretError::missing(file("/home/demo/nas")),
            SecretError {
                source: file("/home/demo/nas"),
                problem: Problem::Missing,
            }
        );
    }

    fn detail_of(stderr: &[u8]) -> String {
        match Problem::command_failed(Some(1), stderr) {
            Problem::CommandFailed { detail, .. } => detail,
            other => panic!("not a command failure: {other:?}"),
        }
    }

    #[test]
    fn a_failed_command_is_told_by_its_status_and_the_first_line_of_its_stderr() {
        assert_eq!(
            Problem::command_failed(Some(3), b"gpg: decryption failed\nsecond line\n"),
            Problem::CommandFailed {
                status: Some(3),
                detail: "gpg: decryption failed".to_owned(),
            }
        );
        // A signal ended it: there is no status.
        assert_eq!(
            Problem::command_failed(None, b"killed"),
            Problem::CommandFailed {
                status: None,
                detail: "killed".to_owned(),
            }
        );
    }

    #[test]
    fn the_detail_is_the_first_line_with_something_on_it() {
        assert_eq!(
            detail_of(b"\n  \r\n\t error: no such entry \r\nnext\n"),
            "error: no such entry"
        );
        assert_eq!(detail_of(b"no newline at all"), "no newline at all");
        assert_eq!(detail_of(b""), "");
        assert_eq!(detail_of(b" \n\n \t\n"), "");
    }

    #[test]
    fn text_that_is_not_utf8_is_shown_with_replacement_characters() {
        assert_eq!(detail_of(&[0xff, b'a', b'b']), "\u{fffd}ab");
    }

    #[test]
    fn a_long_detail_is_cut_at_a_character_boundary_and_marked() {
        let exactly = "x".repeat(MAX_DETAIL_BYTES);
        assert_eq!(detail_of(exactly.as_bytes()), exactly);

        let over = "x".repeat(MAX_DETAIL_BYTES + 1);
        assert_eq!(
            detail_of(over.as_bytes()),
            format!("{}…", "x".repeat(MAX_DETAIL_BYTES))
        );

        // `é` is two bytes: 256 of them fill the limit, a 257th doesn't fit,
        // and a lone `x` in front moves every boundary by one.
        let accents = "é".repeat(300);
        assert_eq!(
            detail_of(accents.as_bytes()),
            format!("{}…", "é".repeat(256))
        );
        let shifted = format!("x{accents}");
        assert_eq!(
            detail_of(shifted.as_bytes()),
            format!("x{}…", "é".repeat(255))
        );
    }
}
