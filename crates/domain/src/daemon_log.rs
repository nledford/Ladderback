//! The recent log (technical design §3.1, §4.2; #618): the most recent lines
//! of the daemon log, kept in memory from the daemon's start on, which the
//! web UI's Log page shows.
//!
//! A line is what the daemon log writes, already masked (I4): when it was
//! written, its level, its source (the part of Ladderback, or the crate,
//! that wrote it) and its message. The recent log is bounded in lines and in
//! bytes, and a line's message and source are cut where they enter, so no
//! line, however long its record, makes it hold more (§13.5, rule 2).

use std::collections::VecDeque;
use std::fmt;

use crate::Timestamp;

/// The most lines the recent log keeps.
pub const MAX_LINES: usize = 1_000;

/// The most bytes of text the recent log keeps: the sum of each line's
/// source and message.
pub const MAX_BYTES: usize = 1 << 20;

/// The most bytes of a line's message, its ellipsis included when it was
/// cut.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024;

/// The most bytes of a line's source, its ellipsis included when it was cut.
/// A source is a module's path, far shorter in practice.
pub const MAX_SOURCE_BYTES: usize = 256;

/// What ends a text that was cut.
pub const CUT: &str = "…";

/// How much a line of the daemon log matters, most severe first: the levels
/// of the `log` facade the daemon writes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// One line of the daemon log, as the log wrote it: its message masked, cut
/// to [`MAX_MESSAGE_BYTES`], and its source cut to [`MAX_SOURCE_BYTES`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    at: Timestamp,
    level: LogLevel,
    source: String,
    message: String,
}

impl LogLine {
    /// The line written at `at`, at `level`, by `source`, saying `message`.
    /// The message is written out only up to its cap, so a record of any
    /// length costs no more than the cap to keep.
    pub fn new(
        _at: Timestamp,
        _level: LogLevel,
        _source: &str,
        _message: fmt::Arguments<'_>,
    ) -> Self {
        todo!()
    }

    pub fn at(&self) -> Timestamp {
        self.at
    }

    pub fn level(&self) -> LogLevel {
        self.level
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// The bytes the line counts for against [`MAX_BYTES`].
    pub fn weight(&self) -> usize {
        todo!()
    }
}

/// The recent log: the newest lines of the daemon log, at most
/// [`MAX_LINES`] of them and [`MAX_BYTES`] of text, the oldest let go first.
#[derive(Clone, Debug, Default)]
pub struct RecentLog {
    /// Oldest first.
    lines: VecDeque<LogLine>,
    /// The sum of the lines' weights.
    bytes: usize,
}

impl RecentLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps `line` as the newest, then lets go of the oldest lines until
    /// the rest are within both caps. A line is never over [`MAX_BYTES`] on
    /// its own, so the newest is always kept.
    pub fn push(&mut self, _line: LogLine) {
        todo!()
    }

    /// The lines kept, newest first.
    pub fn newest_first(&self) -> std::iter::Rev<std::collections::vec_deque::Iter<'_, LogLine>> {
        todo!()
    }

    /// How many lines are kept.
    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The bytes of text kept: the lines' weights, added up.
    pub fn bytes(&self) -> usize {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + second).unwrap()
    }

    fn line(text: &str) -> LogLine {
        LogLine::new(at(0), LogLevel::Info, "ladderback", format_args!("{text}"))
    }

    #[test]
    fn a_line_keeps_what_it_was_given() {
        let line = LogLine::new(
            at(7),
            LogLevel::Warn,
            "ladderback::scheduler",
            format_args!("the plan {} runs next", "home"),
        );
        assert_eq!(line.at(), at(7));
        assert_eq!(line.level(), LogLevel::Warn);
        assert_eq!(line.source(), "ladderback::scheduler");
        assert_eq!(line.message(), "the plan home runs next");
        assert_eq!(
            line.weight(),
            "ladderback::scheduler".len() + "the plan home runs next".len()
        );
    }

    #[test]
    fn a_message_of_exactly_the_cap_is_kept_whole() {
        let text = "a".repeat(MAX_MESSAGE_BYTES);
        assert_eq!(line(&text).message(), text);
    }

    #[test]
    fn a_message_one_byte_over_the_cap_is_cut_with_an_ellipsis_within_the_cap() {
        let text = "a".repeat(MAX_MESSAGE_BYTES + 1);
        let message = line(&text).message().to_owned();
        assert!(message.len() <= MAX_MESSAGE_BYTES, "{}", message.len());
        assert_eq!(
            message,
            format!("{}{CUT}", "a".repeat(MAX_MESSAGE_BYTES - CUT.len()))
        );
    }

    #[test]
    fn a_message_written_in_pieces_is_cut_across_them_at_a_character_boundary() {
        let piece = "é".repeat(1000);
        let message = LogLine::new(
            at(0),
            LogLevel::Info,
            "ladderback",
            format_args!("{piece}{piece}{piece}"),
        )
        .message()
        .to_owned();
        let kept = message.strip_suffix(CUT).expect("an ellipsis at the end");
        assert!(kept.chars().all(|c| c == 'é'), "{kept}");
        assert_eq!(kept.len(), (MAX_MESSAGE_BYTES - CUT.len()) / 2 * 2);
    }

    #[test]
    fn a_source_over_its_cap_is_cut_with_an_ellipsis_within_the_cap() {
        let source = "s".repeat(MAX_SOURCE_BYTES + 10);
        let line = LogLine::new(at(0), LogLevel::Info, &source, format_args!("x"));
        assert_eq!(
            line.source(),
            format!("{}{CUT}", "s".repeat(MAX_SOURCE_BYTES - CUT.len()))
        );
        let whole = "s".repeat(MAX_SOURCE_BYTES);
        let line = LogLine::new(at(0), LogLevel::Info, &whole, format_args!("x"));
        assert_eq!(line.source(), whole);
    }

    #[test]
    fn the_lines_are_listed_newest_first_and_counted() {
        let mut log = RecentLog::new();
        assert!(log.is_empty());
        for text in ["one", "two", "three"] {
            log.push(line(text));
        }
        let listed: Vec<&str> = log.newest_first().map(LogLine::message).collect();
        assert_eq!(listed, ["three", "two", "one"]);
        assert_eq!(log.len(), 3);
        assert!(!log.is_empty());
        assert_eq!(log.bytes(), 3 * "ladderback".len() + "onetwothree".len());
    }

    #[test]
    fn exactly_the_most_lines_are_all_kept_and_one_more_lets_the_oldest_go() {
        let mut log = RecentLog::new();
        for number in 0..MAX_LINES {
            log.push(line(&number.to_string()));
        }
        assert_eq!(log.len(), MAX_LINES);
        log.push(line("newest"));
        assert_eq!(log.len(), MAX_LINES);
        assert_eq!(
            log.newest_first().next_back().map(LogLine::message),
            Some("1")
        );
        let weights: usize = log.newest_first().map(LogLine::weight).sum();
        assert_eq!(log.bytes(), weights, "a line let go is taken off the bytes");
    }

    #[test]
    fn exactly_the_most_bytes_are_all_kept_and_one_more_byte_lets_the_oldest_go() {
        // Lines of 4,096 bytes each, its source included: 256 of them are
        // the cap exactly.
        let text = "m".repeat(MAX_MESSAGE_BYTES - "ladderback".len());
        let mut log = RecentLog::new();
        for _ in 0..MAX_BYTES / MAX_MESSAGE_BYTES {
            log.push(line(&text));
        }
        assert_eq!(log.bytes(), MAX_BYTES);
        assert_eq!(log.len(), MAX_BYTES / MAX_MESSAGE_BYTES);
        log.push(line("m"));
        assert_eq!(log.len(), MAX_BYTES / MAX_MESSAGE_BYTES);
        assert_eq!(
            log.bytes(),
            MAX_BYTES - MAX_MESSAGE_BYTES + "ladderback".len() + 1
        );
        assert_eq!(log.newest_first().next().map(LogLine::message), Some("m"));
    }
}
