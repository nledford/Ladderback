use std::sync::Arc;

use domain::secrets::{
    CommandLine, FileRead, KeyringEntry, MAX_SECRET_BYTES, Problem, SecretError, SecretRef,
    SecretValue, SecretWarning, SourceName,
};

use crate::ports::{Resolved, SecretResolver};

/// Sets up the places secrets are read from, in memory for a fake and for
/// real in a scratch directory for the real resolver, so the same checks run
/// against both. Methods that make a value return it, because the suite
/// can't build the fallible ones itself.
pub trait SecretsHarness: Send + Sync {
    /// A resolver over everything set up so far. A command may run for a
    /// fraction of a second at most: the suite hangs one on purpose.
    fn resolver(&self) -> Arc<dyn SecretResolver>;

    /// Sets a variable of the environment the resolver reads.
    fn set_env(&self, name: &str, value: &str);

    /// Writes a file with Unix permission bits `mode`, replacing any file of
    /// that name, and returns its absolute path. `name` may include a
    /// directory (`credentials/token`), which is created.
    fn write_file(&self, name: &str, contents: &[u8], mode: u32) -> String;

    /// A symbolic link named `name`, where `write_file` would put a file of
    /// that name, that leads to `target`, the path of a file `write_file`
    /// wrote. Returns the link's absolute path.
    fn link(&self, name: &str, target: &str) -> String;

    /// An absolute path with nothing at it, in a directory that exists.
    fn missing_file(&self) -> String;

    /// An absolute path of a directory.
    fn directory(&self) -> String;

    /// Keeps `secret` in the keyring under a name of the harness's choosing
    /// that includes `name`, and returns the entry.
    fn set_keyring_entry(&self, name: &str, secret: &str) -> KeyringEntry;

    /// A keyring entry that holds nothing.
    fn empty_keyring_entry(&self) -> KeyringEntry;

    /// A command that prints exactly `stdout` and succeeds.
    fn printing(&self, stdout: &[u8]) -> CommandLine;

    /// A command that prints `stdout` and `stderr`, then exits with `status`.
    fn failing(&self, status: i32, stdout: &str, stderr: &str) -> CommandLine;

    /// A command that doesn't end on its own.
    fn hanging(&self) -> CommandLine;

    /// A command that can't be started: there is no such program.
    fn unstartable(&self) -> CommandLine;
}

pub async fn run(harness: &dyn SecretsHarness) {
    an_inline_secret_is_its_own_value(harness).await;
    a_variable_gives_its_value_as_it_is(harness).await;
    a_variable_that_is_not_set_is_missing_and_never_empty(harness).await;
    a_file_gives_its_first_line_or_all_of_it(harness).await;
    a_file_that_is_not_there_is_missing(harness).await;
    a_directory_is_not_a_secret_file(harness).await;
    a_secret_has_to_be_text(harness).await;
    a_secret_may_be_as_large_as_the_limit_and_no_larger(harness).await;
    a_file_others_can_read_comes_with_a_warning(harness).await;
    the_credentials_directory_is_trusted(harness).await;
    a_link_out_of_the_credentials_directory_is_not_trusted(harness).await;
    a_command_gives_the_first_line_of_its_output(harness).await;
    a_command_that_prints_nothing_is_missing(harness).await;
    a_command_that_fails_is_told_by_its_status_and_its_stderr(harness).await;
    a_failed_command_that_quotes_what_it_printed_has_it_masked(harness).await;
    a_failed_command_whose_output_ends_partway_through_a_line_quotes_nothing(harness).await;
    a_command_that_cannot_be_started_is_unreachable(harness).await;
    a_command_that_takes_too_long_is_timed_out(harness).await;
    a_keyring_gives_the_secret_of_an_entry(harness).await;
    an_entry_that_is_not_there_is_missing(harness).await;
    a_secret_is_read_again_every_time(harness).await;
    an_error_never_holds_a_secret(harness).await;
}

async fn resolve(
    harness: &dyn SecretsHarness,
    secret: &SecretRef,
) -> Result<Resolved, SecretError> {
    harness.resolver().resolve(secret).await
}

fn gives(value: &str) -> Result<Resolved, SecretError> {
    Ok(Resolved {
        value: SecretValue::new(value),
        warnings: Vec::new(),
    })
}

fn file(path: &str, read: FileRead) -> SecretRef {
    SecretRef::File {
        path: path.to_owned(),
        read,
    }
}

fn env(name: &str) -> SecretRef {
    SecretRef::Env(name.to_owned())
}

fn file_source(path: &str) -> SourceName {
    SourceName::File {
        path: path.to_owned(),
    }
}

fn command_source(command: &CommandLine) -> SourceName {
    SourceName::Command {
        program: command.program().to_owned(),
    }
}

/// `Err` with this problem, naming this source: the part of the error the
/// suite pins; the words of a detail are the platform's.
fn fails_with(
    result: &Result<Resolved, SecretError>,
    source: &SourceName,
    problem: impl Fn(&Problem) -> bool,
) -> bool {
    matches!(result, Err(error) if error.source == *source && problem(&error.problem))
}

async fn an_inline_secret_is_its_own_value(harness: &dyn SecretsHarness) {
    let secret = SecretRef::Inline(SecretValue::new("hunter2"));
    assert_eq!(resolve(harness, &secret).await, gives("hunter2"));
}

async fn a_variable_gives_its_value_as_it_is(harness: &dyn SecretsHarness) {
    harness.set_env("DEMO_PASSWORD", " hunter2 \n");
    assert_eq!(
        resolve(harness, &env("DEMO_PASSWORD")).await,
        gives(" hunter2 \n")
    );
    // Set to nothing is a secret of no characters, and not a missing one.
    harness.set_env("DEMO_EMPTY", "");
    assert_eq!(resolve(harness, &env("DEMO_EMPTY")).await, gives(""));
}

async fn a_variable_that_is_not_set_is_missing_and_never_empty(harness: &dyn SecretsHarness) {
    let result = resolve(harness, &env("DEMO_NOT_SET")).await;
    assert_eq!(
        result,
        Err(SecretError::missing(SourceName::Env {
            name: "DEMO_NOT_SET".to_owned()
        }))
    );
}

async fn a_file_gives_its_first_line_or_all_of_it(harness: &dyn SecretsHarness) {
    let path = harness.write_file("two-lines", b"hunter2\nsecond line\n", 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::FirstLine)).await,
        gives("hunter2")
    );
    // All of it, minus one newline at the end.
    assert_eq!(
        resolve(harness, &file(&path, FileRead::Whole)).await,
        gives("hunter2\nsecond line")
    );
    let path = harness.write_file("no-newline", b"hunter2", 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::FirstLine)).await,
        gives("hunter2")
    );
    assert_eq!(
        resolve(harness, &file(&path, FileRead::Whole)).await,
        gives("hunter2")
    );
    let path = harness.write_file("empty", b"", 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::Whole)).await,
        gives("")
    );
}

async fn a_file_that_is_not_there_is_missing(harness: &dyn SecretsHarness) {
    let path = harness.missing_file();
    for read in [FileRead::FirstLine, FileRead::Whole] {
        assert_eq!(
            resolve(harness, &file(&path, read)).await,
            Err(SecretError::missing(file_source(&path)))
        );
    }
}

async fn a_directory_is_not_a_secret_file(harness: &dyn SecretsHarness) {
    let path = harness.directory();
    let result = resolve(harness, &file(&path, FileRead::Whole)).await;
    assert!(
        fails_with(&result, &file_source(&path), |problem| matches!(
            problem,
            Problem::Unreachable { .. }
        )),
        "{result:?}"
    );
}

async fn a_secret_has_to_be_text(harness: &dyn SecretsHarness) {
    let path = harness.write_file("binary", &[b'a', 0xff, b'b'], 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::Whole)).await,
        Err(SecretError {
            source: file_source(&path),
            problem: Problem::NotUnicode,
        })
    );
    let command = harness.printing(&[b'a', 0xff, b'b']);
    assert_eq!(
        resolve(harness, &SecretRef::Command(command.clone())).await,
        Err(SecretError {
            source: command_source(&command),
            problem: Problem::NotUnicode,
        })
    );
}

/// The limit holds the whole of what a source gives, not only the secret in
/// it: a first line of 7 characters in a file of 64 KiB and a byte is too much.
async fn a_secret_may_be_as_large_as_the_limit_and_no_larger(harness: &dyn SecretsHarness) {
    let at_the_limit = "x".repeat(MAX_SECRET_BYTES);
    let over = "x".repeat(MAX_SECRET_BYTES + 1);
    let too_large = |source: SourceName| {
        Err(SecretError {
            source,
            problem: Problem::TooLarge {
                limit: MAX_SECRET_BYTES,
            },
        })
    };

    harness.set_env("DEMO_AT_THE_LIMIT", &at_the_limit);
    assert_eq!(
        resolve(harness, &env("DEMO_AT_THE_LIMIT")).await,
        gives(&at_the_limit)
    );
    harness.set_env("DEMO_OVER", &over);
    assert_eq!(
        resolve(harness, &env("DEMO_OVER")).await,
        too_large(SourceName::Env {
            name: "DEMO_OVER".to_owned()
        })
    );

    let path = harness.write_file("at-the-limit", at_the_limit.as_bytes(), 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::Whole)).await,
        gives(&at_the_limit)
    );
    let path = harness.write_file("over", over.as_bytes(), 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::Whole)).await,
        too_large(file_source(&path))
    );
    let long = format!("hunter2\n{over}");
    let path = harness.write_file("short-first-line", long.as_bytes(), 0o600);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::FirstLine)).await,
        too_large(file_source(&path))
    );

    let command = harness.printing(at_the_limit.as_bytes());
    assert_eq!(
        resolve(harness, &SecretRef::Command(command)).await,
        gives(&at_the_limit)
    );
    // One byte over is too large, as is a short first line in too much.
    for printed in [&over, &long] {
        let command = harness.printing(printed.as_bytes());
        assert_eq!(
            resolve(harness, &SecretRef::Command(command.clone())).await,
            too_large(command_source(&command))
        );
    }
}

async fn a_file_others_can_read_comes_with_a_warning(harness: &dyn SecretsHarness) {
    for mode in [0o644, 0o640, 0o604] {
        let path = harness.write_file(&format!("exposed-{mode:o}"), b"hunter2\n", mode);
        assert_eq!(
            resolve(harness, &file(&path, FileRead::FirstLine)).await,
            Ok(Resolved {
                value: SecretValue::new("hunter2"),
                warnings: vec![SecretWarning::FileReadableByOthers { path }],
            }),
            "mode {mode:o}"
        );
    }
    for mode in [0o600, 0o400, 0o700] {
        let path = harness.write_file(&format!("private-{mode:o}"), b"hunter2\n", mode);
        assert_eq!(
            resolve(harness, &file(&path, FileRead::FirstLine)).await,
            gives("hunter2"),
            "mode {mode:o}"
        );
    }
}

/// systemd sets the permissions of what it puts in `$CREDENTIALS_DIRECTORY`.
async fn the_credentials_directory_is_trusted(harness: &dyn SecretsHarness) {
    let path = harness.write_file("credential", b"hunter2\n", 0o644);
    let directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
    assert!(!directory.is_empty(), "{path} is not an absolute path");
    harness.set_env("CREDENTIALS_DIRECTORY", directory);
    assert_eq!(
        resolve(harness, &file(&path, FileRead::FirstLine)).await,
        gives("hunter2")
    );
    // A different directory trusts nothing here.
    harness.set_env("CREDENTIALS_DIRECTORY", "/run/credentials/other.service");
    assert_eq!(
        resolve(harness, &file(&path, FileRead::FirstLine)).await,
        Ok(Resolved {
            value: SecretValue::new("hunter2"),
            warnings: vec![SecretWarning::FileReadableByOthers { path }],
        })
    );
    harness.set_env("CREDENTIALS_DIRECTORY", "");
}

/// What systemd and the container runtimes put in a directory is theirs, but
/// a link there to a file they didn't make is not: where a file really is
/// decides whose permissions it has (§10.2).
async fn a_link_out_of_the_credentials_directory_is_not_trusted(harness: &dyn SecretsHarness) {
    let outside = harness.write_file("elsewhere/secret", b"hunter2\n", 0o644);
    let inside = harness.write_file("credentials/real", b"hunter2\n", 0o644);
    let leaving = harness.link("credentials/leaving", &outside);
    let staying = harness.link("credentials/staying", &inside);
    let directory = inside
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    assert!(!directory.is_empty(), "{inside} is not an absolute path");
    harness.set_env("CREDENTIALS_DIRECTORY", directory);
    assert_eq!(
        resolve(harness, &file(&staying, FileRead::FirstLine)).await,
        gives("hunter2"),
        "a link to a file in the directory"
    );
    assert_eq!(
        resolve(harness, &file(&leaving, FileRead::FirstLine)).await,
        Ok(Resolved {
            value: SecretValue::new("hunter2"),
            warnings: vec![SecretWarning::FileReadableByOthers { path: leaving }],
        }),
        "a link to a file outside it"
    );
    harness.set_env("CREDENTIALS_DIRECTORY", "");
}

async fn a_command_gives_the_first_line_of_its_output(harness: &dyn SecretsHarness) {
    for (stdout, expected) in [
        ("hunter2\nsecond line\n", "hunter2"),
        ("hunter2\r\n", "hunter2"),
        ("hunter2", "hunter2"),
        ("  hunter2 \n", "  hunter2 "),
    ] {
        let command = harness.printing(stdout.as_bytes());
        assert_eq!(
            resolve(harness, &SecretRef::Command(command)).await,
            gives(expected),
            "{stdout:?}"
        );
    }
}

/// A command that printed nothing gave no secret: that is not an empty one,
/// which a repository would then fail to open with, far from its cause.
async fn a_command_that_prints_nothing_is_missing(harness: &dyn SecretsHarness) {
    for stdout in ["", "\n", "\r\n", "   ", " \t \n"] {
        let command = harness.printing(stdout.as_bytes());
        assert_eq!(
            resolve(harness, &SecretRef::Command(command.clone())).await,
            Err(SecretError::missing(command_source(&command))),
            "{stdout:?}"
        );
    }
}

async fn a_command_that_fails_is_told_by_its_status_and_its_stderr(harness: &dyn SecretsHarness) {
    let command = harness.failing(3, "hunter2\n", "no such entry\nsecond line");
    assert_eq!(
        resolve(harness, &SecretRef::Command(command.clone())).await,
        Err(SecretError {
            source: command_source(&command),
            problem: Problem::CommandFailed {
                status: Some(3),
                detail: "no such entry".to_owned(),
            },
        })
    );
}

/// A helper that rejects a value may quote it on stderr, which an error shows.
async fn a_failed_command_that_quotes_what_it_printed_has_it_masked(harness: &dyn SecretsHarness) {
    let command = harness.failing(
        1,
        "hunter2-canary\n",
        "rejected hunter2-canary\nsecond line",
    );
    assert_eq!(
        resolve(harness, &SecretRef::Command(command.clone())).await,
        Err(SecretError {
            source: command_source(&command),
            problem: Problem::CommandFailed {
                status: Some(1),
                detail: "rejected ***".to_owned(),
            },
        })
    );
}

/// A failed helper's output that ends partway through a line may have been
/// cut by its own end, as when it is killed partway through printing its
/// secret: masked against that part, a quote of the whole secret would show
/// the rest. Nothing it said is quoted then (I4, #237).
async fn a_failed_command_whose_output_ends_partway_through_a_line_quotes_nothing(
    harness: &dyn SecretsHarness,
) {
    let command = harness.failing(1, "hunter2-ca", "rejected hunter2-canary");
    assert_eq!(
        resolve(harness, &SecretRef::Command(command.clone())).await,
        Err(SecretError {
            source: command_source(&command),
            problem: Problem::CommandFailed {
                status: Some(1),
                detail: String::new(),
            },
        })
    );
}

async fn a_command_that_cannot_be_started_is_unreachable(harness: &dyn SecretsHarness) {
    let command = harness.unstartable();
    let result = resolve(harness, &SecretRef::Command(command.clone())).await;
    assert!(
        fails_with(&result, &command_source(&command), |problem| matches!(
            problem,
            Problem::Unreachable { .. }
        )),
        "{result:?}"
    );
}

async fn a_command_that_takes_too_long_is_timed_out(harness: &dyn SecretsHarness) {
    let command = harness.hanging();
    let result = resolve(harness, &SecretRef::Command(command.clone())).await;
    assert!(
        fails_with(&result, &command_source(&command), |problem| matches!(
            problem,
            Problem::TimedOut { .. }
        )),
        "{result:?}"
    );
}

async fn a_keyring_gives_the_secret_of_an_entry(harness: &dyn SecretsHarness) {
    let entry = harness.set_keyring_entry("nas", "hunter2");
    assert_eq!(
        resolve(harness, &SecretRef::Keyring(entry)).await,
        gives("hunter2")
    );
}

async fn an_entry_that_is_not_there_is_missing(harness: &dyn SecretsHarness) {
    let entry = harness.empty_keyring_entry();
    assert_eq!(
        resolve(harness, &SecretRef::Keyring(entry.clone())).await,
        Err(SecretError::missing(SourceName::Keyring {
            entry: entry.to_string()
        }))
    );
}

/// A resolver keeps nothing: a secret that was rotated since is read as it
/// is now. (A job reads each secret once, which the use case sees to.)
async fn a_secret_is_read_again_every_time(harness: &dyn SecretsHarness) {
    harness.set_env("DEMO_ROTATED", "old");
    assert_eq!(resolve(harness, &env("DEMO_ROTATED")).await, gives("old"));
    harness.set_env("DEMO_ROTATED", "new");
    assert_eq!(resolve(harness, &env("DEMO_ROTATED")).await, gives("new"));

    let path = harness.write_file("rotated", b"old\n", 0o600);
    let secret = file(&path, FileRead::FirstLine);
    assert_eq!(resolve(harness, &secret).await, gives("old"));
    harness.write_file("rotated", b"new\n", 0o600);
    assert_eq!(resolve(harness, &secret).await, gives("new"));
}

/// An error is shown to the user and logged. It names where a secret was
/// looked for and never what was found there, even when that was refused.
async fn an_error_never_holds_a_secret(harness: &dyn SecretsHarness) {
    let canary = "canary-secret-value";
    let big = format!("{canary}\n{}", "x".repeat(MAX_SECRET_BYTES));
    let mut not_text = canary.as_bytes().to_vec();
    not_text.push(0xff);

    let big_file = harness.write_file("canary-large", big.as_bytes(), 0o600);
    let text_file = harness.write_file("canary-binary", &not_text, 0o600);
    let secrets = [
        file(&big_file, FileRead::FirstLine),
        file(&text_file, FileRead::Whole),
        SecretRef::Command(harness.printing(big.as_bytes())),
        SecretRef::Command(harness.printing(&not_text)),
        // What a failing command printed on stdout is not an error detail.
        SecretRef::Command(harness.failing(1, canary, "refused")),
    ];
    for secret in &secrets {
        let result = resolve(harness, secret).await;
        assert!(result.is_err(), "{secret:?} should have failed");
        if let Some(error) = result.err() {
            for text in [format!("{error:?}"), error.to_string()] {
                assert!(!text.contains(canary), "{text}");
            }
        }
    }
}
