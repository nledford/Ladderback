//! How the command line's output is chosen (technical design §4.5): whether
//! plain output was asked for, what `clap` may colour, and how a command that
//! reports state says it: in words, or as a document with `--json`.

use std::ffi::OsString;
use std::io::{self, Write};

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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::{self, Write};

    use clap::ColorChoice;
    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};

    use super::{Report, asked_for_plain, clap_colour, write_json};

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
}
