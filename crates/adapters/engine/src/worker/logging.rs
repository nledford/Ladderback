//! rustic's log records, as worker events.
//!
//! rustic and its backends report through the `log` crate. The worker's
//! stdout is the daemon's protocol channel, so nothing may print to it
//! directly; this logger turns each record into a [`WorkerEvent::Log`]
//! instead. Messages are never localized (§8.4), and the event stream
//! redacts secrets in them like in everything else it writes.

use log::{Level as LogLevel, LevelFilter, Log, Metadata, Record, SetLoggerError};

use super::events::Events;
use crate::job::{Level, WorkerEvent};

/// The most detailed level forwarded. Debug and trace records from rustic's
/// HTTP and storage layers are one line per request, far too many to ship.
pub(crate) const MAX_LEVEL: LevelFilter = LevelFilter::Info;

pub(crate) struct EventLogger {
    events: Events,
}

impl EventLogger {
    pub(crate) fn new(events: Events) -> Self {
        Self { events }
    }
}

impl Log for EventLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= MAX_LEVEL
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            LogLevel::Error => Level::Error,
            LogLevel::Warn => Level::Warn,
            LogLevel::Info => Level::Info,
            LogLevel::Debug | LogLevel::Trace => Level::Debug,
        };
        // A log record has nobody to tell about a broken stream: see
        // `Events::report`.
        self.events.report(&WorkerEvent::Log {
            level,
            message: format!("{}: {}", record.target(), record.args()),
        });
    }

    fn flush(&self) {}
}

/// Makes the process log through `events`. A process has one logger, so this
/// works once.
pub(crate) fn install(events: Events) -> Result<(), SetLoggerError> {
    log::set_boxed_logger(Box::new(EventLogger::new(events)))?;
    log::set_max_level(MAX_LEVEL);
    Ok(())
}

#[cfg(test)]
mod tests {
    use log::Level;

    use super::*;
    use crate::job::{Level as WireLevel, WorkerEvent};
    use crate::worker::events::testing::capture;

    fn record_of(logger: &EventLogger, level: Level, target: &str, message: &str) {
        logger.log(
            &Record::builder()
                .args(format_args!("{message}"))
                .level(level)
                .target(target)
                .build(),
        );
    }

    #[test]
    fn a_record_becomes_a_log_event_that_names_its_source() {
        let (events, capture) = capture();
        let logger = EventLogger::new(events);
        record_of(
            &logger,
            Level::Info,
            "rustic_core::repository",
            "password is correct",
        );
        assert_eq!(
            capture.parsed(),
            [WorkerEvent::Log {
                level: WireLevel::Info,
                message: "rustic_core::repository: password is correct".to_owned()
            }]
        );
    }

    #[test]
    fn warnings_and_errors_keep_their_level() {
        let (events, capture) = capture();
        let logger = EventLogger::new(events);
        record_of(&logger, Level::Warn, "t", "careful");
        record_of(&logger, Level::Error, "t", "broken");
        let levels: Vec<_> = capture
            .parsed()
            .into_iter()
            .map(|event| match event {
                WorkerEvent::Log { level, .. } => level,
                other => panic!("not a log event: {other:?}"),
            })
            .collect();
        assert_eq!(levels, [WireLevel::Warn, WireLevel::Error]);
    }

    #[test]
    fn records_more_detailed_than_the_limit_are_not_forwarded() {
        let (events, capture) = capture();
        let logger = EventLogger::new(events);
        for level in [Level::Debug, Level::Trace] {
            let metadata = Metadata::builder().level(level).target("t").build();
            assert!(!logger.enabled(&metadata), "{level} is forwarded");
            record_of(&logger, level, "t", "chatter");
        }
        assert_eq!(capture.text(), "");
        let info = Metadata::builder().level(Level::Info).target("t").build();
        assert!(logger.enabled(&info));
    }

    #[test]
    fn a_record_that_cannot_be_written_gives_the_worker_up() {
        let (events, gave_up) = crate::worker::events::testing::breaking_after(0);
        let logger = EventLogger::new(events);
        record_of(&logger, Level::Info, "rustic_core", "hello");
        assert_eq!(gave_up.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// The message the bridge sends for one warning of `target`.
    fn bridged(target: &str, message: &str) -> String {
        let (events, capture) = capture();
        let logger = EventLogger::new(events);
        record_of(&logger, Level::Warn, target, message);
        match capture.parsed().as_slice() {
            [WorkerEvent::Log { message, .. }] => message.clone(),
            other => panic!("not one log event: {other:?}"),
        }
    }

    /// rustic's local backend warns, quoting the whole command line, when a
    /// `post-create-command` or `post-delete-command` can't start, can't be
    /// split into words, or exits with an error. The bridge passes the
    /// warning on with the option's name and the reason, without the line,
    /// which can carry a credential the redactor doesn't know (I4, #882).
    /// A line that holds rustic's own markers keeps its text out too. The
    /// warnings are rustic's own at the pinned revision, so a version that
    /// changes them fails here: one that stops quoting the line fails the
    /// first assertion, and the rewrite can go.
    #[test]
    fn rustic_s_post_command_warnings_lose_the_command_line_and_keep_the_reason() {
        use rustic_backend::LocalBackend;
        use rustic_core::{FileType, Id, WriteBackend};

        crate::client::test_support::capture_the_worker_log();
        let scratch = tempfile::tempdir().unwrap();
        let missing = scratch.path().join("no-such-hook");
        for (option, line, canary, reason) in [
            (
                "post-create-command",
                format!("{} --token=CANARY-start", missing.display()),
                "CANARY-start",
                "os error",
            ),
            (
                "post-create-command",
                "/bin/sh -c 'exit 3' hook --token=CANARY-status".to_owned(),
                "CANARY-status",
                "exit status: 3",
            ),
            (
                "post-create-command",
                "/bin/sh --token=CANARY-split 'unclosed".to_owned(),
                "CANARY-split",
                "missing closing quote",
            ),
            (
                "post-delete-command",
                "/bin/sh -c 'exit 4' hook --token=CANARY-delete".to_owned(),
                "CANARY-delete",
                "exit status: 4",
            ),
            (
                "post-create-command",
                format!("{} ': caused by: CANARY-marked-start'", missing.display()),
                "CANARY-marked-start",
                "os error",
            ),
            (
                "post-create-command",
                "/bin/sh -c 'exit 5' hook '` failed with status `CANARY-marked-status'".to_owned(),
                "CANARY-marked-status",
                "exit status: 5",
            ),
            (
                "post-create-command",
                "/bin/sh ' : (source: CANARY-marked-split)' 'unclosed".to_owned(),
                "CANARY-marked-split",
                "missing closing quote",
            ),
        ] {
            let folder = scratch.path().join(canary);
            let backend = LocalBackend::new(
                folder.to_str().unwrap(),
                [(option.to_owned(), line.clone())],
            )
            .unwrap();
            let id = Id::random();
            backend
                .write_bytes(FileType::Snapshot, &id, false, b"demo".to_vec().into())
                .unwrap();
            backend.remove(FileType::Snapshot, &id, false).unwrap();
            let Some(warning) = crate::client::test_support::local_backend_warning(canary) else {
                panic!("rustic no longer quotes {line:?} in a warning");
            };
            let message = bridged("rustic_backend::local", &warning);
            assert!(!message.contains(canary), "the line stays: {message}");
            assert!(message.contains(option), "{message}");
            assert!(message.contains(reason), "the reason is lost: {message}");
        }
    }

    /// A warning of a post command in a shape the bridge doesn't know may
    /// quote the line anywhere: only the option's name goes on.
    #[test]
    fn a_post_command_warning_of_an_unknown_shape_loses_its_text() {
        let message = bridged(
            "rustic_backend::local",
            "post-delete: Error: something new about `notify --token=CANARY-new`",
        );
        assert!(!message.contains("CANARY-new"), "{message}");
        assert!(message.contains("post-delete-command"), "{message}");
    }
}
