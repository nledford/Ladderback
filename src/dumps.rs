//! What the daemon and a worker say when they couldn't turn off their core
//! dumps or debugger attach as they started (decision 98): a warning, by its
//! code, and the process goes on.

use api::WarningCode;
use l10n::Localizer;
use platform::dumps::Unprotected;

/// The code a protection that couldn't be put in place is logged under, and
/// the operating system's words.
fn said(unprotected: &Unprotected) -> (WarningCode, String) {
    match unprotected {
        Unprotected::CoreDumps(error) => (WarningCode::CoreDumpsOn, error.to_string()),
        Unprotected::DebuggerAttach(error) => {
            (WarningCode::DebuggerAttachAllowed, error.to_string())
        }
    }
}

/// The daemon's log line: the code, then its message in the reader's
/// language, with the operating system's words.
pub(crate) fn daemon_line(localizer: &Localizer, unprotected: &Unprotected) -> String {
    let (code, detail) = said(unprotected);
    let message = localizer.warning(code, &[], Some(&detail));
    format!("{code:?}: {message}")
}

/// The worker's line on stderr, which the daemon logs as a warning, masked:
/// the code and the operating system's words. A worker reads no catalog.
pub(crate) fn worker_line(unprotected: &Unprotected) -> String {
    let (code, detail) = said(unprotected);
    format!("ladderback worker: {code:?}: {detail}")
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    fn refused() -> io::Error {
        io::Error::from_raw_os_error(1)
    }

    #[test]
    fn a_core_dump_limit_left_on_is_logged_by_its_code_with_the_systems_words() {
        let line = daemon_line(&Localizer::new(&[]), &Unprotected::CoreDumps(refused()));

        assert!(line.starts_with("CoreDumpsOn: "), "{line}");
        assert!(line.contains("could not turn off core dumps"), "{line}");
        assert!(line.contains(&refused().to_string()), "{line}");
    }

    #[test]
    fn a_debugger_left_able_to_attach_is_logged_by_its_code_with_the_systems_words() {
        let line = daemon_line(
            &Localizer::new(&[]),
            &Unprotected::DebuggerAttach(refused()),
        );

        assert!(line.starts_with("DebuggerAttachAllowed: "), "{line}");
        assert!(line.contains("attaching a debugger"), "{line}");
        assert!(line.contains(&refused().to_string()), "{line}");
    }

    #[test]
    fn a_worker_says_on_stderr_by_its_code_what_it_could_not_turn_off() {
        assert_eq!(
            worker_line(&Unprotected::CoreDumps(refused())),
            format!("ladderback worker: CoreDumpsOn: {}", refused())
        );
        assert_eq!(
            worker_line(&Unprotected::DebuggerAttach(refused())),
            format!("ladderback worker: DebuggerAttachAllowed: {}", refused())
        );
    }
}
