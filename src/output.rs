//! How the command line's output is chosen (technical design §4.5): whether
//! plain output was asked for, what `clap` may colour, how a command that
//! reports state says it (in words, or as a document with `--json`), and what
//! a command does when nobody reads its output any more.
//!
//! Nothing here, and nothing a command prints, goes through `print!` or
//! `eprint!`: they panic when the text cannot be written, and the reader of a
//! pipe can go at any time (`ladderback import-profile nas.toml | head -1`).
//! A command writes through [`to_stdout`] and [`to_stderr`] and takes the
//! failure, and [`status_after`] is the status it ends with.

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

use clap::ColorChoice;
use serde::Serialize;

/// Whether `args`, a command line with the program's name first, ask for
/// plain output with `--plain`. It is looked for in the raw arguments
/// because `clap` formats its help and its usage errors while it parses,
/// before the flag has a value; what follows a bare `--` is a file name or a
/// value, never a flag.
pub(crate) fn asked_for_plain(args: &[OsString]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--plain")
}

/// The colour `clap` may use in its help and its usage errors: none when
/// plain output was asked for; otherwise whatever the environment and the
/// terminal say.
pub(crate) fn clap_colour(plain: bool) -> ColorChoice {
    if plain {
        ColorChoice::Never
    } else {
        ColorChoice::Auto
    }
}

/// How a command that reports state says what it did: in the catalog's
/// sentences, for a person, or as the one JSON document a script parses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Report {
    Text,
    Json,
}

impl Report {
    /// The report a command line asked for, with `--json` or without.
    pub(crate) fn asked(json: bool) -> Self {
        if json { Self::Json } else { Self::Text }
    }
}

/// Writes `document` to `out` as one line of compact JSON, and flushes it:
/// what a command prints with `--json`, and all it prints on standard output.
///
/// The line is made whole before any of it is written, so a document that
/// cannot be made leaves nothing on `out`.
pub(crate) fn write_json(out: &mut impl Write, document: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(document)?;
    line.push(b'\n');
    out.write_all(&line)?;
    out.flush()
}

/// The status of a command that stopped because nobody was reading its
/// output: 128 and the number of SIGPIPE, 13, which is what a shell shows for
/// a program that a closed pipe killed. A Rust program ignores SIGPIPE and is
/// told by a failed write instead, so it ends with this status itself.
pub(crate) const CLOSED_PIPE: u8 = 141;

/// Writes `text` to `out`, all of it, and flushes it. Where `print!` panics
/// when the text cannot be written, this says why.
pub(crate) fn write_text(out: &mut impl Write, text: &str) -> io::Result<()> {
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Writes `text` on standard output (see [`write_text`]).
pub(crate) fn to_stdout(text: &str) -> io::Result<()> {
    write_text(&mut io::stdout().lock(), text)
}

/// Writes `text` on standard error (see [`write_text`]).
pub(crate) fn to_stderr(text: &str) -> io::Result<()> {
    write_text(&mut io::stderr().lock(), text)
}

/// Whether a write failed because nobody reads what it wrote to: the reader
/// of a pipe has gone, as `head` goes once it has its lines.
fn nobody_reads(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

/// The status a command ends with, given how what it had to say was written.
/// It is `status`, the command's own, when all of it was; [`CLOSED_PIPE`],
/// and nothing said, when nobody was reading; and 1 when it could not be
/// written for another reason (a full disk), which `cannot_write` says, with
/// the reason, on the one stream that is left.
pub(crate) fn status_after(
    written: io::Result<ExitCode>,
    cannot_write: impl FnOnce(&io::Error),
) -> ExitCode {
    match written {
        Ok(status) => status,
        Err(error) if nobody_reads(&error) => ExitCode::from(CLOSED_PIPE),
        Err(error) => {
            cannot_write(&error);
            ExitCode::FAILURE
        }
    }
}

/// The status `clap` ends with after it has said its help, its version or a
/// usage error: its own (0, or 2 for a command line it did not understand),
/// unless nobody reads what it printed.
pub(crate) fn parser_status(error: &clap::Error) -> ExitCode {
    status_of_parser(error.exit_code(), error.print())
}

/// [`parser_status`] for the exit code `clap` has and what came of printing
/// its message. A failed write other than a closed pipe is not said, as
/// `clap` never said one: the status stands.
fn status_of_parser(code: i32, printed: io::Result<()>) -> ExitCode {
    match printed {
        Err(error) if nobody_reads(&error) => ExitCode::from(CLOSED_PIPE),
        Ok(()) | Err(_) => u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::process::ExitCode;

    use clap::ColorChoice;
    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};

    use super::{
        Report, asked_for_plain, clap_colour, status_after, status_of_parser, write_json,
        write_text,
    };

    fn command_line(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_flag_is_found_before_or_after_the_command() {
        for words in [
            &["ladderback", "--plain", "serve"][..],
            &["ladderback", "serve", "--plain"],
            &["ladderback", "import-profile", "nas.toml", "--plain"],
            &["ladderback", "--help", "--plain"],
        ] {
            assert!(asked_for_plain(&command_line(words)), "{words:?}");
        }
    }

    #[test]
    fn a_command_line_without_the_flag_does_not_ask() {
        for words in [
            &["ladderback"][..],
            &["ladderback", "serve", "--listen", "127.0.0.1:0"],
            &["ladderback", "serve", "--plainly"],
            &["ladderback", "serve", "--plain=yes"],
            &["ladderback", "serve", "-plain"],
            &["ladderback", "serve", "plain"],
        ] {
            assert!(!asked_for_plain(&command_line(words)), "{words:?}");
        }
    }

    #[test]
    fn what_follows_a_double_dash_is_not_a_flag() {
        assert!(!asked_for_plain(&command_line(&[
            "ladderback",
            "import-profile",
            "--",
            "--plain",
        ])));
        assert!(asked_for_plain(&command_line(&[
            "ladderback",
            "--plain",
            "import-profile",
            "--",
            "nas.toml",
        ])));
    }

    #[test]
    fn plain_output_gets_no_colour_and_the_rest_is_left_to_the_terminal() {
        assert_eq!(clap_colour(true), ColorChoice::Never);
        assert_eq!(clap_colour(false), ColorChoice::Auto);
    }

    #[test]
    fn a_report_is_a_document_when_json_was_asked_for_and_words_when_it_was_not() {
        assert_eq!(Report::asked(true), Report::Json);
        assert_eq!(Report::asked(false), Report::Text);
    }

    #[test]
    fn a_document_is_one_line_of_compact_json_and_its_newline() {
        let mut out = Vec::new();
        write_json(&mut out, &serde_json::json!({"a": [1, 2], "b": {"c": "d"}})).unwrap();
        assert_eq!(out, b"{\"a\":[1,2],\"b\":{\"c\":\"d\"}}\n");
    }

    #[test]
    fn a_line_break_inside_a_text_does_not_break_the_line() {
        let mut out = Vec::new();
        write_json(
            &mut out,
            &serde_json::json!({"detail": "one\ntwo\r\nthree"}),
        )
        .unwrap();
        let written = String::from_utf8(out).unwrap();
        assert_eq!(written.lines().count(), 1, "{written:?}");
        assert!(written.ends_with("}\n"), "{written:?}");
    }

    /// A writer that takes nothing, or takes everything but cannot flush.
    struct Broken {
        takes_nothing: bool,
    }

    impl Write for Broken {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.takes_nothing {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            } else {
                Ok(buf.len())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("cannot flush"))
        }
    }

    #[test]
    fn a_document_that_cannot_be_written_is_an_error() {
        let error = write_json(
            &mut Broken {
                takes_nothing: true,
            },
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn a_document_that_cannot_be_flushed_is_an_error_too() {
        let error = write_json(
            &mut Broken {
                takes_nothing: false,
            },
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "cannot flush");
    }

    #[test]
    fn a_document_that_cannot_be_made_leaves_nothing_written() {
        struct Unmakeable;

        impl Serialize for Unmakeable {
            fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(S::Error::custom("no such document"))
            }
        }

        let mut out = Vec::new();
        assert!(write_json(&mut out, &Unmakeable).is_err());
        assert!(out.is_empty(), "{out:?}");
    }

    /// A writer that keeps what it is given and counts its flushes.
    #[derive(Default)]
    struct Recording {
        written: Vec<u8>,
        flushes: usize,
    }

    impl Write for Recording {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[test]
    fn a_text_is_written_whole_and_flushed() {
        let mut out = Recording::default();
        write_text(&mut out, "one\ntwo").unwrap();
        assert_eq!(out.written, b"one\ntwo");
        assert_eq!(out.flushes, 1);
    }

    #[test]
    fn a_text_that_cannot_be_written_is_an_error_not_a_panic() {
        let error = write_text(
            &mut Broken {
                takes_nothing: true,
            },
            "text",
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        let error = write_text(
            &mut Broken {
                takes_nothing: false,
            },
            "text",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "cannot flush");
    }

    #[test]
    fn a_command_that_wrote_what_it_had_to_say_ends_with_its_own_status() {
        for status in [ExitCode::SUCCESS, ExitCode::FAILURE, ExitCode::from(2)] {
            let said = Cell::new(false);
            assert_eq!(status_after(Ok(status), |_| said.set(true)), status);
            assert!(!said.get());
        }
    }

    #[test]
    fn a_command_whose_output_nobody_reads_ends_quietly_with_141() {
        let said = Cell::new(false);
        let status = status_after(Err(io::Error::from(io::ErrorKind::BrokenPipe)), |_| {
            said.set(true);
        });
        assert_eq!(status, ExitCode::from(141));
        assert!(!said.get());
    }

    #[test]
    fn a_command_that_cannot_write_for_another_reason_says_why_and_ends_with_1() {
        let said = RefCell::new(Vec::new());
        let status = status_after(Err(io::Error::other("no space left")), |error| {
            said.borrow_mut().push(error.to_string());
        });
        assert_eq!(status, ExitCode::FAILURE);
        assert_eq!(*said.borrow(), ["no space left"]);
    }

    #[test]
    fn the_parser_ends_with_its_own_status() {
        assert_eq!(status_of_parser(0, Ok(())), ExitCode::SUCCESS);
        assert_eq!(status_of_parser(2, Ok(())), ExitCode::from(2));
        // A status that is no status of a process is a failure.
        assert_eq!(status_of_parser(-1, Ok(())), ExitCode::FAILURE);
    }

    #[test]
    fn the_parser_whose_message_nobody_reads_ends_with_141() {
        for code in [0, 2] {
            let status = status_of_parser(code, Err(io::Error::from(io::ErrorKind::BrokenPipe)));
            assert_eq!(status, ExitCode::from(141), "{code}");
        }
    }

    #[test]
    fn the_parser_leaves_a_message_it_cannot_write_for_another_reason_unsaid() {
        let status = status_of_parser(2, Err(io::Error::other("no space left")));
        assert_eq!(status, ExitCode::from(2));
    }
}
