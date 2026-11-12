//! `ladderback install-service` and `ladderback remove-service` (operations
//! §12): wires this platform's `UserService` to the program's own path and
//! says what came of it, in the catalog's words.

use std::path::Path;
use std::process::ExitCode;

/// Installs this program as the user's service and starts it.
pub(crate) fn install_service() -> ExitCode {
    todo!()
}

/// Stops the user's service and removes its file.
pub(crate) fn remove_service() -> ExitCode {
    todo!()
}

/// `path` as a message shows it: a control character as its escape, never
/// sent to the terminal as it is.
fn shown(path: &Path) -> String {
    let _ = path;
    todo!()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::shown;

    #[test]
    fn a_path_is_shown_as_it_is() {
        let path = "/home/demo/My Apps/o'brien \"x\" \\ é/ladderback.service";
        assert_eq!(shown(Path::new(path)), path);
    }

    #[test]
    fn a_control_character_of_a_path_is_shown_as_its_escape() {
        assert_eq!(
            shown(Path::new("/srv/a\nb\u{1b}[31m\u{85}/ladderback.service")),
            r"/srv/a\nb\u{1b}[31m\u{85}/ladderback.service"
        );
    }
}
