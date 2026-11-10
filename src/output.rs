//! How the command line's output is chosen (technical design §4.5): whether
//! plain output was asked for, and what `clap` may colour.

use std::ffi::OsString;

use clap::ColorChoice;

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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::ColorChoice;

    use super::{asked_for_plain, clap_colour};

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
}
