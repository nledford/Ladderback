//! What can go wrong when a secret is read, and what is worth a warning
//! although it worked. Codes and values, never sentences (§8.4): the
//! presentation layers word them.

use std::path::{Component, Path};

use super::source::{SourceName, first_line};
use super::{MAX_SECRET_BYTES, Redactor, SecretValue};

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
    pub fn missing(source: SourceName) -> Self {
        Self {
            source,
            problem: Problem::Missing,
        }
    }
}

/// What was wrong with a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// There is nothing there: the variable isn't set, the file doesn't
    /// exist, the keyring has no such entry, the command printed nothing.
    /// Never read as an empty secret.
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
    pub fn command_failed(status: Option<i32>, stderr: &[u8]) -> Self {
        let text = String::from_utf8_lossy(stderr);
        let line = text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default();
        let detail = if line.len() <= MAX_DETAIL_BYTES {
            line.to_owned()
        } else {
            let end = (0..=MAX_DETAIL_BYTES)
                .rev()
                .find(|&at| line.is_char_boundary(at))
                .unwrap_or_default();
            format!("{}…", line.get(..end).unwrap_or_default())
        };
        Self::CommandFailed { status, detail }
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
    source: &SourceName,
    bytes: &[u8],
    extract: impl FnOnce(&str) -> &str,
) -> Result<SecretValue, SecretError> {
    let fail = |problem| SecretError {
        source: source.clone(),
        problem,
    };
    if bytes.len() > MAX_SECRET_BYTES {
        return Err(fail(Problem::TooLarge {
            limit: MAX_SECRET_BYTES,
        }));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| fail(Problem::NotUnicode))?;
    Ok(SecretValue::new(extract(text)))
}

/// The secret that a command gave as its `stdout`: the first line of it. A
/// command that printed nothing, or only white space, gave no secret: that
/// is `Missing`, never an empty secret, which a repository would fail to
/// open with, far from its cause. (A variable, a file or a keyring entry
/// that is set to nothing was set so on purpose, and stays the empty
/// secret.)
pub fn command_secret(source: &SourceName, stdout: &[u8]) -> Result<SecretValue, SecretError> {
    let secret = decode(source, stdout, first_line)?;
    if secret.expose().trim().is_empty() {
        return Err(SecretError::missing(source.clone()));
    }
    Ok(secret)
}

/// The error of a command that ran and failed: its exit `status` and what it
/// wrote to `stderr`, with what it printed on `stdout` masked in it.
/// `stderr_cut`: `stderr` is only the start of what it wrote, so it may end
/// in the start of what it printed, which is masked too.
pub fn command_failure(
    source: &SourceName,
    status: Option<i32>,
    stdout: Option<&[u8]>,
    stderr: &[u8],
    stderr_cut: bool,
) -> SecretError {
    let Some(stdout) = stdout else {
        todo!("a failed command whose stdout was cut")
    };
    // A helper that rejects a value may quote it on stderr: what it printed
    // (less the line ending its last line ends with, which isn't what it
    // quotes), and its first line, which is what it would have given as its
    // secret, are masked there.
    let output = String::from_utf8_lossy(stdout);
    let printed = SecretValue::new(output.trim_end_matches(['\r', '\n']));
    let first = SecretValue::new(first_line(&output));
    let redactor = Redactor::new([&printed, &first]);
    let said = if stderr_cut {
        redactor.redact_cut(stderr)
    } else {
        redactor.redact(&String::from_utf8_lossy(stderr))
    };
    SecretError {
        source: source.clone(),
        problem: Problem::command_failed(status, said.as_bytes()),
    }
}

/// Where container runtimes and systemd control permissions, so a file there
/// isn't warned about (§10.2); `$CREDENTIALS_DIRECTORY` is the third.
const RUNTIME_SECRET_DIRECTORIES: [&str; 2] = ["/run/secrets", "/var/run/secrets"];

/// The group and world read bits of a Unix file mode.
const READABLE_BY_OTHERS: u32 = 0o044;

/// The warning for a secret file with Unix permission bits `mode`, if it
/// deserves one: it is group- or world-readable and isn't under one of the
/// directories where a container runtime or systemd sets permissions.
/// `credentials_directory` is the value of `$CREDENTIALS_DIRECTORY`, which
/// only counts when it is a real absolute directory.
///
/// Where a file is written isn't where it is: a link in `/run/secrets` may
/// lead anywhere. `resolve` follows every link in a path (`None` when it
/// can't), and the file is exempt only if it really is in one of the
/// directories, which really are directories below the root. A path that
/// can't be followed is not exempt. The warning names the path as written.
pub fn file_warning(
    path: &str,
    mode: u32,
    credentials_directory: Option<&str>,
    resolve: impl Fn(&str) -> Option<String>,
) -> Option<SecretWarning> {
    let exposed = mode & READABLE_BY_OTHERS != 0;
    (exposed && !is_runtime_secret(path, credentials_directory, resolve)).then(|| {
        SecretWarning::FileReadableByOthers {
            path: path.to_owned(),
        }
    })
}

fn is_runtime_secret(
    path: &str,
    credentials_directory: Option<&str>,
    resolve: impl Fn(&str) -> Option<String>,
) -> bool {
    let written = Path::new(path);
    // `/run/secrets/../../home/demo/key` isn't in `/run/secrets`.
    if written
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    let credentials = credentials_directory.filter(|directory| names_a_directory(directory));
    // Where the file really is; looked up once, and only if some directory
    // names it.
    let mut file = None;
    RUNTIME_SECRET_DIRECTORIES
        .into_iter()
        .chain(credentials)
        .filter(|directory| written.starts_with(directory))
        .any(|directory| {
            let Some(directory) = resolve(directory) else {
                return false;
            };
            let file = file.get_or_insert_with(|| resolve(path));
            names_a_directory(&directory)
                && file
                    .as_deref()
                    .is_some_and(|file| Path::new(file).starts_with(&directory))
        })
}

/// An absolute path below the root. An empty or relative value, or the root
/// itself, would count every file as being in the directory.
fn names_a_directory(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        && path
            .components()
            .any(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::first_line;

    /// Where a path leads when nothing is a link.
    fn same(path: &str) -> Option<String> {
        Some(path.to_owned())
    }

    fn warns(path: &str, mode: u32, credentials: Option<&str>) -> bool {
        file_warning(path, mode, credentials, same).is_some()
    }

    #[test]
    fn a_file_others_can_read_is_warned_about() {
        let path = "/home/demo/secrets/nas";
        assert_eq!(
            file_warning(path, 0o644, None, same),
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
        let directory = Some("/run/credentials/ladderback.service");
        assert!(!warns(
            "/run/credentials/ladderback.service/nas",
            0o644,
            directory
        ));
        assert!(!warns(
            "/run/credentials/ladderback.service/sub/nas",
            0o644,
            directory
        ));
        // Not under it, or no directory set.
        assert!(warns(
            "/run/credentials/other.service/nas",
            0o644,
            directory
        ));
        assert!(warns(
            "/run/credentials/ladderback.service/nas",
            0o644,
            None
        ));
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

    /// A file system where each `(path, target)` is a link, and every other
    /// path leads to itself.
    fn linked<'a>(links: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |path| {
            let target = links
                .iter()
                .find(|(link, _)| *link == path)
                .map_or(path, |(_, target)| *target);
            Some(target.to_owned())
        }
    }

    #[test]
    fn a_link_out_of_an_exempt_directory_is_warned_about() {
        let path = "/run/secrets/token";
        let links = [(path, "/home/demo/token")];
        assert_eq!(
            file_warning(path, 0o644, None, linked(&links)),
            Some(SecretWarning::FileReadableByOthers {
                path: path.to_owned()
            })
        );
        // The credentials directory is no different.
        let credentials = Some("/run/credentials/demo.service");
        let path = "/run/credentials/demo.service/token";
        let links = [(path, "/home/demo/token")];
        assert!(file_warning(path, 0o644, credentials, linked(&links)).is_some());
    }

    #[test]
    fn a_path_that_cannot_be_followed_is_not_exempt() {
        for path in ["/run/secrets/token", "/var/run/secrets/token"] {
            assert!(
                file_warning(path, 0o644, None, |_| None).is_some(),
                "{path}"
            );
        }
        let credentials = Some("/run/credentials/demo.service");
        let path = "/run/credentials/demo.service/token";
        assert!(file_warning(path, 0o644, credentials, |_| None).is_some());
    }

    #[test]
    fn a_credentials_directory_that_leads_to_the_root_exempts_nothing() {
        let directory = "/run/credentials/demo.service";
        let path = "/run/credentials/demo.service/token";
        let links = [(directory, "/"), (path, "/token")];
        assert!(file_warning(path, 0o644, Some(directory), linked(&links)).is_some());
    }

    fn warns_through(
        path: &str,
        mode: u32,
        credentials: Option<&str>,
        links: &[(&str, &str)],
    ) -> bool {
        file_warning(path, mode, credentials, linked(links)).is_some()
    }

    #[test]
    fn a_link_that_stays_in_the_exempt_directory_is_not_warned_about() {
        let path = "/run/secrets/token";
        let links = [(path, "/run/secrets/generation-2/token")];
        assert!(!warns_through(path, 0o644, None, &links));
        let credentials = Some("/run/credentials/demo.service");
        let path = "/run/credentials/demo.service/token";
        let links = [(path, "/run/credentials/demo.service/real")];
        assert!(!warns_through(path, 0o644, credentials, &links));
    }

    /// `/var/run` is a link to `/run`: the directory and the file in it lead
    /// to the same place, so the file is where the directory is.
    #[test]
    fn a_directory_that_is_itself_a_link_exempts_what_really_is_in_it() {
        let directory = "/var/run/credentials/demo.service";
        let path = "/var/run/credentials/demo.service/token";
        let links = [
            (directory, "/run/credentials/demo.service"),
            (path, "/run/credentials/demo.service/token"),
        ];
        assert!(!warns_through(path, 0o644, Some(directory), &links));
        let links = [
            (directory, "/run/credentials/demo.service"),
            (path, "/tmp/x"),
        ];
        assert!(warns_through(path, 0o644, Some(directory), &links));
    }

    #[test]
    fn a_directory_that_cannot_be_followed_exempts_nothing() {
        let path = "/run/secrets/token";
        let resolve = |at: &str| (at == path).then(|| path.to_owned());
        assert!(file_warning(path, 0o644, None, resolve).is_some());
    }

    fn command(program: &str) -> SourceName {
        SourceName::Command {
            program: program.to_owned(),
        }
    }

    #[test]
    fn a_command_that_printed_nothing_gave_no_secret() {
        let source = command("pass");
        for stdout in ["", "\n", "\r\n", "   ", " \t \n", "\u{a0}\n"] {
            assert_eq!(
                command_secret(&source, stdout.as_bytes()),
                Err(SecretError::missing(source.clone())),
                "{stdout:?}"
            );
        }
    }

    /// The secret is the first line, so a blank first line is no secret
    /// even when a later line holds text.
    #[test]
    fn a_command_whose_first_line_is_blank_gave_no_secret() {
        let source = command("pass");
        assert_eq!(
            command_secret(&source, b"\n\nhunter2\n"),
            Err(SecretError::missing(source))
        );
    }

    #[test]
    fn what_a_failed_command_printed_is_masked_where_it_says_so_on_stderr() {
        let source = command("pass");
        let error = command_failure(
            &source,
            Some(1),
            Some(b"hunter2-canary\n".as_slice()),
            b"rejected hunter2-canary\nsecond line\n",
            false,
        );
        assert_eq!(
            error,
            SecretError {
                source: source.clone(),
                problem: Problem::CommandFailed {
                    status: Some(1),
                    detail: "rejected ***".to_owned(),
                },
            }
        );
        // The first line alone is what the command would have given.
        let error = command_failure(
            &source,
            Some(2),
            Some(b"hunter2\nsecond line\n".as_slice()),
            b"bad value hunter2\n",
            false,
        );
        assert_eq!(
            error.problem,
            Problem::CommandFailed {
                status: Some(2),
                detail: "bad value ***".to_owned(),
            }
        );
    }

    /// The kept stderr can end inside the secret a helper quotes, or hold
    /// less than the whole of a long one: no part of it reaches the detail
    /// (I4).
    #[test]
    fn a_secret_cut_off_with_the_kept_stderr_is_masked_in_the_detail() {
        let source = command("pass");
        let secret: String = ('a'..='z').cycle().take(10_000).collect();
        let stdout = format!("{secret}\n");
        // Longer than the kept stderr.
        let kept = secret.as_bytes().get(..8192).unwrap();
        let error = command_failure(&source, Some(1), Some(stdout.as_bytes()), kept, true);
        assert_eq!(
            error.problem,
            Problem::CommandFailed {
                status: Some(1),
                detail: "***".to_owned(),
            }
        );
        // Quoted, and cut inside it.
        let stderr = format!("rejected {}", secret.get(..600).unwrap());
        let error = command_failure(
            &source,
            Some(1),
            Some(stdout.as_bytes()),
            stderr.as_bytes(),
            true,
        );
        assert_eq!(
            error.problem,
            Problem::CommandFailed {
                status: Some(1),
                detail: "rejected ***".to_owned(),
            }
        );
    }

    /// A helper can cut its stderr short itself, partway through the secret
    /// it quotes, and nothing in what was read says so: what it said is
    /// masked as a cut text whoever stopped it, so nothing of the secret
    /// reaches the detail (I4, #232). What ends nowhere near the secret is
    /// kept as it was said.
    #[test]
    fn what_a_failed_command_said_is_masked_as_cut_whoever_cut_it() {
        let source = command("pass");
        let stdout = Some(b"hunter2-canary\n".as_slice());
        for (said, detail) in [
            ("rejected hello", "rejected hello"),
            ("rejected hunter2-", "rejected ***"),
            ("rejected h", "rejected ***"),
        ] {
            let error = command_failure(&source, Some(1), stdout, said.as_bytes(), false);
            assert_eq!(
                error.problem,
                Problem::CommandFailed {
                    status: Some(1),
                    detail: detail.to_owned(),
                },
                "{said:?}"
            );
        }
    }

    /// A stdout that was cut holds only part of the secret, and masking
    /// what the helper said against that part would show the rest of it
    /// (`***nary`): nothing it said is quoted then, and the error keeps its
    /// status (I4, #233).
    #[test]
    fn a_failed_command_whose_stdout_was_cut_quotes_nothing_it_said() {
        let source = command("pass");
        for stderr_cut in [false, true] {
            let error = command_failure(
                &source,
                Some(1),
                None,
                b"rejected hunter2-canary\n",
                stderr_cut,
            );
            assert_eq!(
                error,
                SecretError {
                    source: source.clone(),
                    problem: Problem::CommandFailed {
                        status: Some(1),
                        detail: String::new(),
                    },
                }
            );
        }
    }

    #[test]
    fn a_command_gives_the_first_line_of_what_it_printed_as_it_is() {
        let source = command("pass");
        for (stdout, secret) in [
            ("hunter2\nsecond line\n", "hunter2"),
            ("hunter2\r\n", "hunter2"),
            ("hunter2", "hunter2"),
            ("  hunter2 \n", "  hunter2 "),
            ("\u{a0}x\n", "\u{a0}x"),
        ] {
            let given = command_secret(&source, stdout.as_bytes()).unwrap();
            assert_eq!(given.expose(), secret, "{stdout:?}");
        }
    }

    /// What a command printed is held to the limits of every source before
    /// it is asked whether there is a secret in it.
    #[test]
    fn what_a_command_printed_is_too_large_or_not_text_before_it_is_missing() {
        let source = command("pass");
        let blank = " ".repeat(MAX_SECRET_BYTES + 1);
        assert_eq!(
            command_secret(&source, blank.as_bytes()),
            Err(SecretError {
                source: source.clone(),
                problem: Problem::TooLarge {
                    limit: MAX_SECRET_BYTES
                },
            })
        );
        assert_eq!(
            command_secret(&source, &[0xff, b'\n']),
            Err(SecretError {
                source,
                problem: Problem::NotUnicode,
            })
        );
    }
}
