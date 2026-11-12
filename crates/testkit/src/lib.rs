//! Test support for Ladderback's own tests. A dev-dependency only: no
//! production crate may depend on it (`scripts/check-architecture.sh`).
//!
//! [`skip!`] is how a test says it can't run here (running as root makes
//! `chmod 000` ineffective, a backend tool isn't installed). A silent early
//! return would read as a pass.
//!
//! [`daemon::isolate`] is how a test that starts the daemon gives it folders
//! of its own, never the platform's.
//!
//! [`scenario_watch::ScenarioWatch`] is how a cucumber runner stops at a
//! scenario that runs too long, naming it, instead of hanging until CI
//! cancels the job.
//!
//! [`inheritable::InheritablePipe`] is how a test shows that what starts a
//! child waits for the spawn gate.

pub mod daemon;
#[cfg(unix)]
pub mod inheritable;
pub mod scenario_watch;

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

/// The environment variable naming the file that collects skips. `just test`
/// sets it and lists the file's lines when the run ends.
pub const SKIP_LOG_ENV: &str = "LADDERBACK_SKIP_LOG";

/// Skips the current test, visibly: prints `SKIPPED <module path>: <reason>`
/// to stderr, appends the same line to the file named by `LADDERBACK_SKIP_LOG`
/// when that is set, then returns from the enclosing function. The reason
/// takes `format!` arguments.
///
/// ```text
/// if running_as_root() {
///     testkit::skip!("root ignores file permissions");
/// }
/// ```
#[macro_export]
macro_rules! skip {
    ($($reason:tt)+) => {{
        $crate::record_skip(::std::module_path!(), &::std::format!($($reason)+));
        return;
    }};
}

/// The one-line record of a skip. Line breaks in the reason become spaces,
/// so every skip stays one line of the log.
pub fn skip_line(module: &str, reason: &str) -> String {
    let reason = reason.replace(['\r', '\n'], " ");
    format!("SKIPPED {module}: {reason}")
}

/// Records a skip on stderr and in the file named by `LADDERBACK_SKIP_LOG`.
pub fn record_skip(module: &str, reason: &str) {
    // This crate's own tests skip on purpose; they must not add lines to
    // the list of the run that is testing it.
    let log = if cfg!(test) {
        None
    } else {
        // An empty variable is no log.
        std::env::var_os(SKIP_LOG_ENV).filter(|log| !log.is_empty())
    };
    record_skip_to(log.as_deref().map(Path::new), module, reason);
}

/// Records a skip on stderr and appends it to `log` when there is one. A log
/// that can't be written is reported on stderr, never a panic: the skip
/// itself still shows there.
pub fn record_skip_to(log: Option<&Path>, module: &str, reason: &str) {
    let line = skip_line(module, reason);
    eprintln!("{line}");
    if let Some(log) = log
        && let Err(error) = append_line(log, &line)
    {
        eprintln!(
            "testkit: cannot record the skip in {}: {error}",
            log.display()
        );
    }
}

/// One `write` per line, so skips from test processes running side by side
/// don't interleave.
fn append_line(log: &Path, line: &str) -> io::Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?
        .write_all(format!("{line}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use super::*;

    fn skipping(skipped: bool, ran_on: &Cell<bool>) {
        if skipped {
            skip!("no {} here", "tool");
        }
        ran_on.set(true);
    }

    #[test]
    fn a_skip_returns_from_the_test_before_the_rest_runs() {
        let ran = Cell::new(false);
        skipping(true, &ran);
        assert!(!ran.get());
        skipping(false, &ran);
        assert!(ran.get());
    }

    #[test]
    fn a_skip_line_names_the_module_and_the_reason() {
        assert_eq!(
            skip_line("engine::client::tests", "sh is missing"),
            "SKIPPED engine::client::tests: sh is missing"
        );
    }

    #[test]
    fn a_reason_stays_on_one_line() {
        assert_eq!(
            skip_line("m", "one\ntwo\r\nthree"),
            "SKIPPED m: one two  three"
        );
    }

    #[test]
    fn skips_are_appended_to_the_log() {
        let dir = std::env::temp_dir().join(format!("ladderback-testkit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("skips.log");
        fs::write(&log, "SKIPPED earlier::test: already there\n").unwrap();
        record_skip_to(Some(&log), "a::b", "first");
        record_skip_to(Some(&log), "c::d", "second");
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "SKIPPED earlier::test: already there\nSKIPPED a::b: first\nSKIPPED c::d: second\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_log_that_cannot_be_written_does_not_fail_the_skip() {
        let missing = std::env::temp_dir()
            .join(format!("ladderback-testkit-none-{}", std::process::id()))
            .join("no-such-dir")
            .join("skips.log");
        record_skip_to(Some(&missing), "a::b", "reason");
        assert!(!missing.exists());
    }
}
