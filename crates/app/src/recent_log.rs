//! The daemon's recent log: the real [`RecentLogStore`] behind the port
//! (technical design §4.2; #618), the lines in memory, within the caps the
//! domain's [`RecentLog`] keeps, and the answer the Log page reads.

use std::sync::{Mutex, PoisonError};

use api::{LogLevelDto, LogLineDto, RecentLogDto};
use domain::daemon_log::{LogLevel, LogLine, RecentLog};

use crate::ports::RecentLogStore;

/// The recent log as the API answers it: every line kept, newest first,
/// each instant in UTC to the millisecond, as the log writes it.
pub fn answer(log: &dyn RecentLogStore) -> RecentLogDto {
    RecentLogDto {
        lines: log.newest_first().iter().map(line_dto).collect(),
    }
}

fn line_dto(line: &LogLine) -> LogLineDto {
    LogLineDto {
        at: format!("{:.3}", line.at()),
        level: match line.level() {
            LogLevel::Error => LogLevelDto::Error,
            LogLevel::Warn => LogLevelDto::Warn,
            LogLevel::Info => LogLevelDto::Info,
            LogLevel::Debug => LogLevelDto::Debug,
            LogLevel::Trace => LogLevelDto::Trace,
        },
        source: line.source().to_owned(),
        message: line.message().to_owned(),
    }
}

/// The recent log in memory, shared by the logger that records and the
/// routes that read.
#[derive(Debug, Default)]
pub struct RecentLogRing {
    log: Mutex<RecentLog>,
}

impl RecentLogRing {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RecentLogStore for RecentLogRing {
    fn record(&self, line: LogLine) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    fn newest_first(&self) -> Vec<LogLine> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .newest_first()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use domain::Timestamp;

    use super::*;

    fn line(utc_ms: i64, level: LogLevel, message: &str) -> LogLine {
        let at = Timestamp::from_millisecond(utc_ms).unwrap();
        LogLine::new(at, level, "ladderback::demo", format_args!("{message}"))
    }

    #[test]
    fn the_answer_lists_each_line_newest_first_its_instant_in_utc_to_the_millisecond() {
        let ring = RecentLogRing::new();
        let levels = [
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ];
        for (offset, level) in (0..).zip(levels) {
            ring.record(line(1_800_000_000_000 + offset * 250, level, "said"));
        }
        let answer = answer(&ring);
        let listed: Vec<(&str, LogLevelDto)> = answer
            .lines
            .iter()
            .map(|line| (line.at.as_str(), line.level))
            .collect();
        assert_eq!(
            listed,
            [
                ("2027-01-15T08:00:01.000Z", LogLevelDto::Trace),
                ("2027-01-15T08:00:00.750Z", LogLevelDto::Debug),
                ("2027-01-15T08:00:00.500Z", LogLevelDto::Info),
                ("2027-01-15T08:00:00.250Z", LogLevelDto::Warn),
                ("2027-01-15T08:00:00.000Z", LogLevelDto::Error),
            ]
        );
        let first = answer.lines.first().unwrap();
        assert_eq!(first.source, "ladderback::demo");
        assert_eq!(first.message, "said");
    }
}
