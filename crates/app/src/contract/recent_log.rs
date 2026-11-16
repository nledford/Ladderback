//! The contract every [`RecentLogStore`] meets: it lists what it was given
//! newest first, and keeps no more than the domain's caps allow.

use std::sync::Arc;

use domain::Timestamp;
use domain::daemon_log::{LogLevel, LogLine, MAX_BYTES, MAX_LINES};

use crate::ports::RecentLogStore;

/// Makes an empty recent log for each check.
pub trait RecentLogHarness {
    fn log(&self) -> Arc<dyn RecentLogStore>;
}

pub fn run(harness: &dyn RecentLogHarness) {
    a_new_log_lists_nothing(harness);
    the_lines_are_listed_newest_first_as_recorded(harness);
    past_the_most_lines_the_oldest_are_let_go(harness);
    past_the_most_bytes_the_oldest_are_let_go(harness);
}

fn line(second: i64, level: LogLevel, message: &str) -> LogLine {
    let at = Timestamp::from_second(1_800_000_000 + second).unwrap_or(Timestamp::UNIX_EPOCH);
    LogLine::new(at, level, "ladderback", format_args!("{message}"))
}

fn messages(log: &dyn RecentLogStore) -> Vec<String> {
    log.newest_first()
        .iter()
        .map(|line| line.message().to_owned())
        .collect()
}

fn a_new_log_lists_nothing(harness: &dyn RecentLogHarness) {
    assert!(harness.log().newest_first().is_empty());
}

fn the_lines_are_listed_newest_first_as_recorded(harness: &dyn RecentLogHarness) {
    let log = harness.log();
    let recorded = [
        line(0, LogLevel::Info, "starting"),
        line(1, LogLevel::Warn, "a warning"),
        line(2, LogLevel::Error, "an error"),
    ];
    for line in recorded.clone() {
        log.record(line);
    }
    let mut newest_first = recorded.to_vec();
    newest_first.reverse();
    assert_eq!(log.newest_first(), newest_first);
}

fn past_the_most_lines_the_oldest_are_let_go(harness: &dyn RecentLogHarness) {
    let log = harness.log();
    for number in 0..MAX_LINES + 3 {
        log.record(line(0, LogLevel::Info, &number.to_string()));
    }
    let kept = messages(log.as_ref());
    assert_eq!(kept.len(), MAX_LINES);
    assert_eq!(kept.first().map(String::as_str), Some("1002"));
    assert_eq!(kept.last().map(String::as_str), Some("3"));
}

fn past_the_most_bytes_the_oldest_are_let_go(harness: &dyn RecentLogHarness) {
    let log = harness.log();
    let text = "m".repeat(4000);
    for _ in 0..300 {
        log.record(line(0, LogLevel::Info, &text));
    }
    let kept = log.newest_first();
    let bytes: usize = kept.iter().map(LogLine::weight).sum();
    assert!(bytes <= MAX_BYTES, "{bytes} bytes kept");
    assert!(
        kept.len() < 300 && bytes > MAX_BYTES - 4096,
        "{} lines kept",
        kept.len()
    );
}
