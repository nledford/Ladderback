//! What the daemon, the MCP server and a worker say when they couldn't turn
//! off their core dumps or debugger attach as they started (decision 98): a
//! warning, by its code, and the process goes on.

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

/// The log line of the daemon and of the MCP server: the code, then its
/// message in the reader's language, with the operating system's words.
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

    /// `set-password` has turned off its core dumps, and on Linux its
    /// dumpable flag, while it still waits for the password (#1256): a
    /// Ctrl-\ at its prompt leaves no dump of what was typed.
    #[cfg(unix)]
    #[test]
    fn set_password_refuses_core_dumps_before_it_reads_the_password() {
        at_prompt::refused_while_waiting(
            "dumps::tests::set_password_refuses_core_dumps_before_it_reads_the_password",
            || crate::set_password::set_password(false),
        );
    }

    /// `pair` has turned off its core dumps, and on Linux its dumpable
    /// flag, while it still waits for the pairing token (#1256).
    #[cfg(all(unix, feature = "hub"))]
    #[test]
    fn pair_refuses_core_dumps_before_it_reads_the_token() {
        at_prompt::refused_while_waiting(
            "dumps::tests::pair_refuses_core_dumps_before_it_reads_the_token",
            || {
                crate::hub::pair(
                    crate::hub::PairAsk {
                        address: None,
                        hostname: None,
                        yes: false,
                    },
                    false,
                )
            },
        );
    }

    /// A command run in a copy of this test binary, with standard input
    /// held open and empty, so that it waits at its prompt for good: the
    /// copy watches its own core-dump limit, and on Linux its dumpable flag,
    /// for the refusal (decision 98), which a refusal after the prompt never
    /// reaches.
    #[cfg(unix)]
    mod at_prompt {
        use std::process::{Command, ExitCode, Stdio};
        use std::time::{Duration, Instant};

        use nix::sys::resource::{Resource, getrlimit};

        /// Set in the copy that runs the command.
        const COPY: &str = "LADDERBACK_TEST_DUMPS_AT_PROMPT";

        /// How the copy ends: what it saw while the command waited.
        const REFUSED: i32 = 0;
        const NOT_REFUSED: i32 = 3;
        const DID_NOT_WAIT: i32 = 4;
        const REFUSED_ALREADY: i32 = 5;

        /// How long the copy watches for the refusal.
        const WATCH: Duration = Duration::from_secs(5);

        /// How long the command must go on waiting once the refusal shows,
        /// so that a command that refused and then ended without its prompt
        /// ends the copy as [`DID_NOT_WAIT`] first.
        const STILL_WAITING: Duration = Duration::from_millis(300);

        pub(super) fn refused_while_waiting(test: &str, command: fn() -> ExitCode) {
            if std::env::var_os(COPY).is_some() {
                in_the_copy(command);
            }
            let data = tempfile::tempdir().unwrap();
            let mut copy = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test])
                .env(COPY, "1")
                .env("LADDERBACK_DATA_DIR", data.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            // Open and empty until the copy has ended.
            let input = copy.stdin.take();
            let output = copy.wait_with_output().unwrap();
            drop(input);
            let said = String::from_utf8_lossy(&output.stderr);
            match output.status.code() {
                Some(REFUSED) => {}
                Some(REFUSED_ALREADY) => {
                    testkit::skip!("the core-dump limit is 0 already, so no refusal can show")
                }
                Some(NOT_REFUSED) => {
                    panic!("core dumps were still on while the command waited: {said}")
                }
                Some(DID_NOT_WAIT) => panic!("the command ended without waiting: {said}"),
                _ => panic!("the copy ended {:?}: {said}", output.status),
            }
        }

        fn in_the_copy(command: fn() -> ExitCode) -> ! {
            // On Linux the dumpable flag can still show a refusal where the
            // limit is 0 already.
            if refused() {
                std::process::exit(REFUSED_ALREADY);
            }
            std::thread::spawn(|| {
                let until = Instant::now() + WATCH;
                while Instant::now() < until {
                    if refused() {
                        std::thread::sleep(STILL_WAITING);
                        std::process::exit(REFUSED);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                std::process::exit(NOT_REFUSED);
            });
            let _ = command();
            std::process::exit(DID_NOT_WAIT);
        }

        /// Both limits 0, and on Linux the dumpable flag cleared.
        fn refused() -> bool {
            let limit = getrlimit(Resource::RLIMIT_CORE).ok() == Some((0, 0));
            #[cfg(target_os = "linux")]
            let limit = limit && nix::sys::prctl::get_dumpable().ok() == Some(false);
            limit
        }
    }
}
