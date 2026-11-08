//! The daemon's own log.
//!
//! Every crate reports through the `log` facade, and nothing is written until
//! a logger is installed: the daemon's records, and the lines the worker's
//! stderr is forwarded as (target `steading::worker`), would be dropped.

use env_logger::{Logger, Target};

/// Makes the daemon log to stderr, once.
pub(crate) fn install() {}

/// The daemon's logger, writing to `target`.
pub(crate) fn logger(_rust_log: Option<&str>, _target: Target) -> Logger {
    todo!()
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, PoisonError};

    use env_logger::{Logger, Target};
    use log::{Level, Log, Record};

    use super::logger;

    const LEVELS: [Level; 5] = [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ];

    /// What a logger wrote to its pipe.
    #[derive(Clone, Default)]
    struct Pipe(Arc<Mutex<Vec<u8>>>);

    impl Pipe {
        fn text(&self) -> String {
            let written = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            String::from_utf8_lossy(&written).into_owned()
        }
    }

    impl Write for Pipe {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A logger set up like the daemon's for `rust_log`, and the pipe it
    /// writes to.
    fn piped(rust_log: Option<&str>) -> (Logger, Pipe) {
        let pipe = Pipe::default();
        (logger(rust_log, Target::Pipe(Box::new(pipe.clone()))), pipe)
    }

    fn log_to(logger: &Logger, level: Level, target: &str, message: &str) {
        logger.log(
            &Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    /// The levels, among a record each for `target`, that a logger set up for
    /// `rust_log` writes.
    fn written(rust_log: Option<&str>, target: &str) -> Vec<Level> {
        let (logger, pipe) = piped(rust_log);
        for level in LEVELS {
            log_to(&logger, level, target, &format!("a {level} record"));
        }
        let text = pipe.text();
        LEVELS
            .into_iter()
            .filter(|level| text.contains(&format!("a {level} record")))
            .collect()
    }

    const NORMAL: [Level; 3] = [Level::Error, Level::Warn, Level::Info];

    #[test]
    fn by_default_errors_warnings_and_infos_are_logged_and_debug_and_trace_are_not() {
        for target in ["steading", "steading::worker"] {
            assert_eq!(written(None, target), NORMAL, "{target}");
        }
    }

    #[test]
    fn rust_log_debug_lets_debug_through_but_not_trace() {
        for target in ["steading", "steading::worker"] {
            assert_eq!(
                written(Some("debug"), target),
                [Level::Error, Level::Warn, Level::Info, Level::Debug],
                "{target}"
            );
        }
    }

    #[test]
    fn rust_log_error_drops_warnings() {
        for target in ["steading", "steading::worker"] {
            assert_eq!(written(Some("error"), target), [Level::Error], "{target}");
        }
    }

    #[test]
    fn a_directive_for_one_target_silences_just_that_target() {
        let worker = written(Some("steading::worker=off"), "steading::worker");
        assert!(worker.is_empty(), "the worker still logged: {worker:?}");
        assert_eq!(written(Some("steading::worker=off"), "steading"), NORMAL);
    }

    #[test]
    fn a_directive_for_one_target_leaves_the_others_at_the_default_level() {
        assert_eq!(
            written(Some("steading::worker=debug"), "steading::worker"),
            [Level::Error, Level::Warn, Level::Info, Level::Debug]
        );
        assert_eq!(written(Some("steading::worker=debug"), "steading"), NORMAL);
    }

    #[test]
    fn an_empty_or_blank_rust_log_is_the_default() {
        for rust_log in ["", " ", "  \t\n"] {
            assert_eq!(written(Some(rust_log), "steading"), NORMAL, "{rust_log:?}");
        }
    }

    #[test]
    fn each_record_is_one_plain_line_with_its_level_target_and_message() {
        let (logger, pipe) = piped(None);
        log_to(&logger, Level::Warn, "steading::worker", "disk is slow");
        log_to(&logger, Level::Info, "steading", "steading 0.0.0 starting");
        let text = pipe.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        assert!(
            lines[0].contains("WARN")
                && lines[0].contains("steading::worker")
                && lines[0].ends_with("disk is slow"),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].contains("INFO")
                && lines[1].contains("steading")
                && lines[1].ends_with("steading 0.0.0 starting"),
            "{}",
            lines[1]
        );
        assert!(text.ends_with('\n'), "{text:?}");
        assert!(!text.contains('\u{1b}'), "a pipe got colors: {text:?}");
    }
}
