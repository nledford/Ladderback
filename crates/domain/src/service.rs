//! The service (operations §12): an agent the user's service manager runs in
//! the background, from login on, as its service file says.

use std::path::Path;

/// A path a service file may name: the program the service runs, or a file
/// beside it such as its log. It is absolute, so it means the same in the
/// manager as in the shell that installed the service; it never steps up a
/// folder with `..`, so it names the file it reads as; it is text with no
/// control character, so a line break can't end the line it is written on
/// and start a setting of its own; and it holds no quote or backslash, which
/// systemd refuses in the path of the program it runs. It is kept with `.`
/// and doubled separators dropped, as systemd wants a program's path. How
/// each service file quotes the rest is the adapter's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServicePath(String);

/// Why a path can't be named by a service file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServicePathProblem {
    /// It is not text (Unicode).
    NotText,
    /// It is relative.
    Relative,
    /// It steps up a folder (`..`).
    StepsUp,
    /// It holds a control character, such as a line break.
    ControlCharacter,
    /// It holds a quote (`"`, `'`) or a backslash.
    Quote,
}

impl ServicePath {
    /// The path, if a service file may name it.
    ///
    /// # Errors
    ///
    /// The first [`ServicePathProblem`] it has, in the order the variants are
    /// listed.
    pub fn parse(path: &Path) -> Result<Self, ServicePathProblem> {
        let _ = path;
        todo!()
    }

    /// The path as text.
    pub fn as_str(&self) -> &str {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ServicePath, ServicePathProblem};

    fn parse(path: &str) -> Result<ServicePath, ServicePathProblem> {
        ServicePath::parse(Path::new(path))
    }

    #[test]
    fn an_absolute_path_is_kept_as_it_is_written() {
        let path = parse("/home/demo/.local/bin/ladderback").unwrap();
        assert_eq!(path.as_str(), "/home/demo/.local/bin/ladderback");
    }

    #[test]
    fn spaces_and_the_characters_a_service_file_reads_are_kept() {
        // Quoting them is the service file's business: the path is the user's.
        let written = "/home/demo/My Apps/50% $HOME&<x>;/ladderback";
        assert_eq!(parse(written).unwrap().as_str(), written);
    }

    #[test]
    fn doubled_separators_and_dots_are_dropped() {
        let path = parse("/home//demo/./.local/bin/ladderback/").unwrap();
        assert_eq!(path.as_str(), "/home/demo/.local/bin/ladderback");
    }

    #[test]
    fn a_quote_or_a_backslash_is_refused() {
        for written in [
            r#"/home/demo/My "Apps"/ladderback"#,
            "/home/demo/o'brien/ladderback",
            r"/home/demo/a\b/ladderback",
        ] {
            assert_eq!(parse(written), Err(ServicePathProblem::Quote), "{written:?}");
        }
    }

    #[test]
    fn a_control_character_is_found_before_a_quote() {
        assert_eq!(
            parse("/home/demo/\"\n/ladderback"),
            Err(ServicePathProblem::ControlCharacter)
        );
    }

    #[test]
    fn a_relative_path_is_refused() {
        assert_eq!(parse("bin/ladderback"), Err(ServicePathProblem::Relative));
        assert_eq!(parse(""), Err(ServicePathProblem::Relative));
    }

    #[test]
    fn a_path_that_steps_up_a_folder_is_refused() {
        assert_eq!(
            parse("/home/demo/bin/../../../usr/bin/ladderback"),
            Err(ServicePathProblem::StepsUp)
        );
    }

    #[test]
    fn a_name_that_merely_holds_dots_is_kept() {
        assert!(parse("/home/demo/..ladderback/ladderback..").is_ok());
    }

    #[test]
    fn a_line_break_or_another_control_character_is_refused() {
        for written in [
            "/home/demo/bin\n[Service]\nExecStart=/bin/sh/ladderback",
            "/home/demo/bin\r/ladderback",
            "/home/demo/bin\t/ladderback",
            "/home/demo/bin\u{0}/ladderback",
            "/home/demo/bin\u{7f}/ladderback",
            "/home/demo/bin\u{85}/ladderback",
        ] {
            assert_eq!(
                parse(written),
                Err(ServicePathProblem::ControlCharacter),
                "{written:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_text_is_refused() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"/home/demo/\xff/ladderback"));
        assert_eq!(ServicePath::parse(path), Err(ServicePathProblem::NotText));
    }
}
