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
        todo!()
    }

    fn log(&self, record: &Record) {
        todo!()
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
}
