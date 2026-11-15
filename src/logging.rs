//! The daemon's own log.
//!
//! Every crate reports through the `log` facade, and nothing is written until
//! a logger is installed: the daemon's records, and the lines the worker's
//! stderr is forwarded as (target `ladderback::worker`), would be dropped.
//!
//! The daemon logs to stderr, at `info` unless `RUST_LOG` says otherwise.
//! While the daemon opens a repository itself (to evaluate retention), the
//! records of rustic and the crates under it reach this log too, and every
//! record is masked with that repository's secrets (`engine::Masked`, I4).

use env_logger::{Builder, Logger, Target, WriteStyle};
use log::{Level, LevelFilter, Log, Metadata, Record};

/// What the daemon logs when `RUST_LOG` names no level. env_logger's own
/// default is `error`, which would hide the warnings of a worker.
const DEFAULT_LEVEL: LevelFilter = LevelFilter::Info;

/// Makes the daemon log to stderr, once, in the style [`write_style`] gives
/// for `plain`. Where a logger is set already, this one or another, that one
/// stays, and nothing is changed or reported.
pub(crate) fn install(plain: bool) {
    let rust_log = std::env::var("RUST_LOG").ok();
    set(logger(
        rust_log.as_deref(),
        Target::Stderr,
        write_style(plain),
    ));
}

/// Makes `logger` the process's logger, once, at the level it filters at,
/// its records masked while the daemon has a repository open itself. Where
/// a logger is set already, that one stays.
fn set(logger: DaemonLog) {
    let level = logger.filter();
    if log::set_boxed_logger(Box::new(engine::Masked::new(logger))).is_ok() {
        log::set_max_level(level);
    }
}

/// The MCP SDK's target. At debug level it logs every request and result
/// whole, a `preview_import`'s profile text and its password included (I4).
const MCP_SDK: &str = "rmcp";

/// The most the MCP SDK ([`MCP_SDK`]) logs, whatever `RUST_LOG` says.
const MCP_SDK_MOST: Level = Level::Warn;

/// The daemon's logger: env_logger's, which refuses every record of the MCP
/// SDK below [`MCP_SDK_MOST`]. The cap checks each record's target rather
/// than adding a directive: env_logger lets the directive for the longest
/// matching name decide, so one for a module of the SDK
/// (`rmcp::service=debug`) would outrank one for the whole SDK.
pub(crate) struct DaemonLog(Logger);

impl DaemonLog {
    /// The most verbose level any record can pass at.
    fn filter(&self) -> LevelFilter {
        self.0.filter()
    }
}

/// Whether `metadata` is a record of the MCP SDK, or of a module of it,
/// below [`MCP_SDK_MOST`].
fn sdk_verbose(metadata: &Metadata<'_>) -> bool {
    let of_sdk = metadata
        .target()
        .strip_prefix(MCP_SDK)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"));
    of_sdk && metadata.level() > MCP_SDK_MOST
}

impl Log for DaemonLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        !sdk_verbose(metadata) && self.0.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if !sdk_verbose(record.metadata()) {
            self.0.log(record);
        }
    }

    fn flush(&self) {
        self.0.flush();
    }
}

/// The daemon's logger, writing to `target` in `style`, for the value of
/// `RUST_LOG`, if that is set.
///
/// It logs at [`DEFAULT_LEVEL`] unless `rust_log` names another level, and
/// env_logger's directives (`debug`, `ladderback::worker=off`) go on top of
/// that, so one for a target leaves the others at the default. An empty or
/// blank `rust_log` says nothing, as none does. Whatever it says, the MCP
/// SDK logs at [`MCP_SDK_MOST`] at most.
pub(crate) fn logger(rust_log: Option<&str>, target: Target, style: WriteStyle) -> DaemonLog {
    let mut builder = Builder::new();
    builder.filter_level(DEFAULT_LEVEL);
    if let Some(rust_log) = rust_log {
        builder.parse_filters(rust_log);
    }
    DaemonLog(
        builder
            .target(target)
            .write_style(style)
            .format_timestamp_millis()
            .build(),
    )
}

/// How the daemon's log is styled: with no colour at all when plain output
/// was asked for; otherwise with colour where the stream is a terminal and
/// the environment (`NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`) allows it.
pub(crate) fn write_style(plain: bool) -> WriteStyle {
    if plain {
        WriteStyle::Never
    } else {
        WriteStyle::Auto
    }
}

#[cfg(test)]
mod tests {
    mod in_process_open;

    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, PoisonError};

    use env_logger::{Target, WriteStyle};
    use log::{Level, Log, Record};

    use super::{DaemonLog, logger, write_style};

    const LEVELS: [Level; 5] = [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ];

    /// What a logger wrote to its pipe.
    #[derive(Clone, Default)]
    pub(super) struct Pipe(Arc<Mutex<Vec<u8>>>);

    impl Pipe {
        pub(super) fn text(&self) -> String {
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
    fn piped(rust_log: Option<&str>) -> (DaemonLog, Pipe) {
        let pipe = Pipe::default();
        (
            logger(
                rust_log,
                Target::Pipe(Box::new(pipe.clone())),
                WriteStyle::Auto,
            ),
            pipe,
        )
    }

    fn log_to(logger: &DaemonLog, level: Level, target: &str, message: &str) {
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
        for target in ["ladderback", "ladderback::worker"] {
            assert_eq!(written(None, target), NORMAL, "{target}");
        }
    }

    #[test]
    fn rust_log_debug_lets_debug_through_but_not_trace() {
        for target in ["ladderback", "ladderback::worker"] {
            assert_eq!(
                written(Some("debug"), target),
                [Level::Error, Level::Warn, Level::Info, Level::Debug],
                "{target}"
            );
        }
    }

    #[test]
    fn rust_log_error_drops_warnings() {
        for target in ["ladderback", "ladderback::worker"] {
            assert_eq!(written(Some("error"), target), [Level::Error], "{target}");
        }
    }

    #[test]
    fn a_directive_for_one_target_silences_just_that_target() {
        let worker = written(Some("ladderback::worker=off"), "ladderback::worker");
        assert!(worker.is_empty(), "the worker still logged: {worker:?}");
        assert_eq!(
            written(Some("ladderback::worker=off"), "ladderback"),
            NORMAL
        );
    }

    #[test]
    fn a_directive_for_one_target_leaves_the_others_at_the_default_level() {
        assert_eq!(
            written(Some("ladderback::worker=debug"), "ladderback::worker"),
            [Level::Error, Level::Warn, Level::Info, Level::Debug]
        );
        assert_eq!(
            written(Some("ladderback::worker=debug"), "ladderback"),
            NORMAL
        );
    }

    /// The MCP SDK logs each request and result whole at debug level, a
    /// profile's password included (I4), so no `RUST_LOG` lets it under
    /// `warn`, not even a directive for one of its modules; one that silences
    /// it more is kept.
    #[test]
    fn the_mcp_sdk_never_logs_under_warn_whatever_rust_log_says() {
        let warnings = [Level::Error, Level::Warn];
        for rust_log in [
            None,
            Some("debug"),
            Some("trace"),
            Some("rmcp=trace"),
            Some("rmcp::service=debug"),
            Some("rmcp::service"),
        ] {
            for target in ["rmcp", "rmcp::service"] {
                assert_eq!(
                    written(rust_log, target),
                    warnings,
                    "{rust_log:?}, {target}"
                );
            }
        }
        assert_eq!(written(Some("error"), "rmcp::service"), [Level::Error]);
        assert!(written(Some("rmcp=off"), "rmcp::service").is_empty());
        assert_eq!(
            written(Some("trace"), "ladderback"),
            LEVELS,
            "other targets keep their level"
        );
    }

    #[test]
    fn an_empty_or_blank_rust_log_is_the_default() {
        for rust_log in ["", " ", "  \t\n"] {
            assert_eq!(
                written(Some(rust_log), "ladderback"),
                NORMAL,
                "{rust_log:?}"
            );
        }
    }

    #[test]
    fn each_record_is_one_plain_line_with_its_level_target_and_message() {
        let (logger, pipe) = piped(None);
        log_to(&logger, Level::Warn, "ladderback::worker", "disk is slow");
        log_to(
            &logger,
            Level::Info,
            "ladderback",
            "ladderback 0.0.0 starting",
        );
        let text = pipe.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        assert!(
            lines[0].contains("WARN")
                && lines[0].contains("ladderback::worker")
                && lines[0].ends_with("disk is slow"),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].contains("INFO")
                && lines[1].contains("ladderback")
                && lines[1].ends_with("ladderback 0.0.0 starting"),
            "{}",
            lines[1]
        );
        assert!(text.ends_with('\n'), "{text:?}");
        assert!(!text.contains('\u{1b}'), "a pipe got colors: {text:?}");
    }

    #[test]
    fn plain_output_turns_colour_off_and_the_rest_is_left_to_the_terminal() {
        assert_eq!(write_style(true), WriteStyle::Never);
        assert_eq!(write_style(false), WriteStyle::Auto);
    }

    /// What a logger of `style` writes for one warning, through a pipe.
    fn written_in(style: WriteStyle) -> String {
        let pipe = Pipe::default();
        let logger = logger(None, Target::Pipe(Box::new(pipe.clone())), style);
        log_to(&logger, Level::Warn, "ladderback", "disk is slow");
        pipe.text()
    }

    #[test]
    fn the_style_a_logger_is_given_decides_whether_its_records_are_coloured() {
        let coloured = written_in(WriteStyle::Always);
        assert!(coloured.contains('\u{1b}'), "{coloured:?}");
        let plain = written_in(WriteStyle::Never);
        assert!(!plain.contains('\u{1b}'), "{plain:?}");
        assert!(plain.ends_with("disk is slow\n"), "{plain:?}");
    }
}
