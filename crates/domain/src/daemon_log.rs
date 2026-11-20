//! The recent log (technical design §3.1, §4.2; #618): the most recent lines
//! of the daemon log, kept in memory from the daemon's start on, which the
//! web UI's Log page shows and MCP's `read_log` reads (#957).
//!
//! A line is what the daemon log writes, already masked (I4): when it was
//! written, its level, its source (the part of Ladderback, or the crate,
//! that wrote it) and its message. The recent log is bounded in lines and in
//! bytes, and a line's message and source are cut where they enter, so no
//! line, however long its record, makes it hold more (§13.5, rule 2).

use std::collections::VecDeque;
use std::fmt;

use crate::{OperationId, PlanId, Timestamp};

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

/// The most bytes of text one read of the recent log answers: the sum of
/// its lines' sources and messages, as [`MAX_BYTES`] counts them. An AI
/// agent reads the answer into its context, and 200 lines of up to 4 KiB
/// each would be most of a megabyte; 32 KiB of text is about 8,000 tokens,
/// more than a page of 200 lines of usual length (about 100 bytes each)
/// takes, so the cap binds only when the lines are long. The answer's JSON
/// adds each line's time, level and number (§16, decision 134).
pub const READ_MAX_BYTES: usize = 32 * 1024;

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

impl LogLevel {
    /// Whether a line at this level is at `level` or worse, as the Log
    /// page's level choice and `read_log`'s `level` keep it: every line is
    /// at [`Trace`](Self::Trace) or worse, and only an error at
    /// [`Error`](Self::Error).
    pub fn is_at_least(self, level: Self) -> bool {
        todo!("{level:?}")
    }
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
    pub fn new(at: Timestamp, level: LogLevel, source: &str, message: fmt::Arguments<'_>) -> Self {
        let mut capped_source = Capped::new(MAX_SOURCE_BYTES);
        // An error here is only the writer saying it is full.
        let _ = fmt::Write::write_str(&mut capped_source, source);
        let mut capped_message = Capped::new(MAX_MESSAGE_BYTES);
        let _ = fmt::write(&mut capped_message, message);
        Self {
            at,
            level,
            source: capped_source.text,
            message: capped_message.text,
        }
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

    /// The line's number: 1 for the first line the recent log kept since
    /// the daemon started, one more for each line after it. 0 for a line
    /// not kept yet. A read's `before` takes it, so a page after another
    /// starts where that one ended however many lines came since.
    pub fn number(&self) -> u64 {
        todo!()
    }

    /// The line as numbered `number`: one the recent log kept, read back
    /// from its answer.
    #[must_use]
    pub fn numbered(self, number: u64) -> Self {
        todo!("{number}")
    }

    /// The bytes the line counts for against [`MAX_BYTES`].
    pub fn weight(&self) -> usize {
        self.source.len().saturating_add(self.message.len())
    }
}

/// A text written up to `cap` bytes. What goes past the cap is dropped, and
/// the text is cut at a character's boundary to end in [`CUT`] within the
/// cap. Once full, it refuses every write, which stops a formatting early.
struct Capped {
    text: String,
    cap: usize,
    full: bool,
}

impl Capped {
    fn new(cap: usize) -> Self {
        Self {
            text: String::new(),
            cap,
            full: false,
        }
    }
}

impl fmt::Write for Capped {
    fn write_str(&mut self, piece: &str) -> fmt::Result {
        if self.full {
            return Err(fmt::Error);
        }
        if self.text.len().saturating_add(piece.len()) <= self.cap {
            self.text.push_str(piece);
            return Ok(());
        }
        let end = piece.floor_char_boundary(self.cap.saturating_sub(self.text.len()));
        self.text.push_str(piece.get(..end).unwrap_or_default());
        let keep = self
            .text
            .floor_char_boundary(self.cap.saturating_sub(CUT.len()));
        self.text.truncate(keep);
        self.text.push_str(CUT);
        self.full = true;
        Err(fmt::Error)
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
    pub fn push(&mut self, line: LogLine) {
        self.bytes = self.bytes.saturating_add(line.weight());
        self.lines.push_back(line);
        while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
            let Some(oldest) = self.lines.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(oldest.weight());
        }
    }

    /// The lines kept, newest first.
    pub fn newest_first(&self) -> std::iter::Rev<std::collections::vec_deque::Iter<'_, LogLine>> {
        self.lines.iter().rev()
    }

    /// How many lines are kept.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The bytes of text kept: the lines' weights, added up.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Which lines a read of the recent log keeps: those at `level` or worse,
/// written from `since` to `until` (both included), and naming `operation`
/// and `plan`, each when given. Every line by default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogFilter {
    pub level: LogLevel,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    /// A line names an operation by its ID, hyphenated, as the daemon's log
    /// writes it.
    pub operation: Option<OperationId>,
    /// A line names a plan as `plan <id>`, the ID ending where the message
    /// does or at a character no plan's ID holds.
    pub plan: Option<PlanId>,
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            level: LogLevel::Trace,
            since: None,
            until: None,
            operation: None,
            plan: None,
        }
    }
}

impl LogFilter {
    /// Whether `line` is one the filter keeps.
    pub fn keeps(&self, line: &LogLine) -> bool {
        todo!("{line:?}")
    }
}

/// The cap that cut a read of the recent log short, with more lines to
/// read after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadCut {
    /// It holds as many lines as it was asked for.
    Lines,
    /// The next line would take it past [`READ_MAX_BYTES`].
    Bytes,
}

/// One read of the recent log: its lines, newest first, and the cap that
/// cut it short when lines the filter keeps are left after them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRead<'a> {
    pub lines: Vec<&'a LogLine>,
    pub cut: Option<ReadCut>,
}

/// The lines of `newest_first` that `filter` keeps, numbered below `before`
/// when given, newest first: at most `limit` of them, and no more than
/// [`READ_MAX_BYTES`] of text, though always the first, which is never more
/// than a line's caps.
pub fn read<'a>(
    newest_first: impl IntoIterator<Item = &'a LogLine>,
    filter: &LogFilter,
    before: Option<u64>,
    limit: usize,
) -> LogRead<'a> {
    let _ = (newest_first.into_iter(), filter, before, limit);
    todo!()
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

    fn said(second: i64, level: LogLevel, text: &str) -> LogLine {
        LogLine::new(at(second), level, "ladderback", format_args!("{text}"))
    }

    /// `lines`, oldest first, kept in a recent log.
    fn kept(lines: impl IntoIterator<Item = LogLine>) -> RecentLog {
        let mut log = RecentLog::new();
        for line in lines {
            log.push(line);
        }
        log
    }

    #[test]
    fn a_level_is_at_itself_and_every_level_less_severe_than_it() {
        use LogLevel::{Debug, Error, Info, Trace, Warn};
        let levels = [Error, Warn, Info, Debug, Trace];
        for (rank, line) in levels.into_iter().enumerate() {
            for (threshold_rank, threshold) in levels.into_iter().enumerate() {
                assert_eq!(
                    line.is_at_least(threshold),
                    rank <= threshold_rank,
                    "{line:?} at {threshold:?}"
                );
            }
        }
    }

    #[test]
    fn the_recent_log_numbers_each_line_from_1_and_a_line_let_go_keeps_the_rest_numbered() {
        assert_eq!(line("not kept").number(), 0);
        assert_eq!(line("read back").numbered(42).number(), 42);
        let mut log = kept((0..MAX_LINES).map(|_| line("l")));
        log.push(line("newest"));
        let numbers: Vec<u64> = log.newest_first().map(LogLine::number).collect();
        let expected: Vec<u64> = (2..=1_001).rev().collect();
        assert_eq!(numbers, expected);
    }

    #[test]
    fn a_filter_keeps_the_lines_at_its_level_or_worse() {
        let filter = LogFilter {
            level: LogLevel::Warn,
            ..LogFilter::default()
        };
        assert!(filter.keeps(&said(0, LogLevel::Error, "e")));
        assert!(filter.keeps(&said(0, LogLevel::Warn, "w")));
        assert!(!filter.keeps(&said(0, LogLevel::Info, "i")));
        assert!(LogFilter::default().keeps(&said(0, LogLevel::Trace, "t")));
    }

    #[test]
    fn a_filter_keeps_the_lines_written_from_its_start_to_its_end_both_included() {
        let filter = LogFilter {
            since: Some(at(10)),
            until: Some(at(20)),
            ..LogFilter::default()
        };
        let kept: Vec<i64> = [9, 10, 15, 20, 21]
            .into_iter()
            .filter(|second| filter.keeps(&said(*second, LogLevel::Info, "x")))
            .collect();
        assert_eq!(kept, [10, 15, 20]);
    }

    #[test]
    fn a_filter_of_an_operation_keeps_the_lines_that_name_its_id() {
        let id = uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0042);
        let filter = LogFilter {
            operation: Some(OperationId::from_uuid(id)),
            ..LogFilter::default()
        };
        let ended = format!("operation {} ended: Succeeded", id.hyphenated());
        let other = uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0043);
        assert!(filter.keeps(&said(0, LogLevel::Info, &ended)));
        assert!(!filter.keeps(&said(
            0,
            LogLevel::Info,
            &format!("operation {} ended: Succeeded", other.hyphenated())
        )));
        assert!(!filter.keeps(&said(0, LogLevel::Info, "listening")));
    }

    #[test]
    fn a_filter_of_a_plan_keeps_the_lines_that_name_it_and_not_a_plan_whose_id_it_begins() {
        let filter = LogFilter {
            plan: Some(PlanId::parse("home").unwrap()),
            ..LogFilter::default()
        };
        let keeps = |text: &str| filter.keeps(&said(0, LogLevel::Warn, text));
        assert!(keeps("the manual run of plan home didn't start: sealed"));
        assert!(keeps("plan home: a demo warning"));
        assert!(keeps("the backup of plan home"));
        assert!(!keeps("the manual run of plan homework didn't start"));
        assert!(!keeps("the manual run of plan home-2 didn't start"));
        assert!(!keeps("the scheduled run of plan work was skipped: home"));
    }

    #[test]
    fn a_read_lists_the_lines_its_filter_keeps_newest_first_below_before() {
        let log = kept([
            said(1, LogLevel::Warn, "one"),
            said(2, LogLevel::Info, "two"),
            said(3, LogLevel::Warn, "three"),
            said(4, LogLevel::Warn, "four"),
        ]);
        let filter = LogFilter {
            level: LogLevel::Warn,
            ..LogFilter::default()
        };
        let read_all = read(log.newest_first(), &filter, None, 10);
        let messages: Vec<&str> = read_all.lines.iter().map(|line| line.message()).collect();
        assert_eq!(messages, ["four", "three", "one"]);
        assert_eq!(read_all.cut, None);
        let below = read(log.newest_first(), &filter, Some(4), 10);
        let numbers: Vec<u64> = below.lines.iter().map(|line| line.number()).collect();
        assert_eq!(numbers, [3, 1]);
    }

    #[test]
    fn a_read_of_as_many_lines_as_it_asks_for_is_cut_only_when_a_line_it_keeps_is_left() {
        let log = kept([
            said(1, LogLevel::Info, "one"),
            said(2, LogLevel::Warn, "two"),
            said(3, LogLevel::Warn, "three"),
        ]);
        let every = read(log.newest_first(), &LogFilter::default(), None, 2);
        assert_eq!(every.lines.len(), 2);
        assert_eq!(every.cut, Some(ReadCut::Lines));
        let warnings = LogFilter {
            level: LogLevel::Warn,
            ..LogFilter::default()
        };
        let both = read(log.newest_first(), &warnings, None, 2);
        assert_eq!(both.lines.len(), 2);
        assert_eq!(both.cut, None, "the line left is one the filter leaves out");
    }

    #[test]
    fn a_read_stops_before_the_line_that_would_pass_its_bytes_and_says_so() {
        // Lines of 1 KiB each, their source included.
        let text = "m".repeat(1024 - "ladderback".len());
        let log = kept((0..40).map(|_| line(&text)));
        let exactly = read(log.newest_first(), &LogFilter::default(), None, 200);
        assert_eq!(exactly.lines.len(), READ_MAX_BYTES / 1024);
        assert_eq!(exactly.cut, Some(ReadCut::Bytes));
        let few = read(log.newest_first(), &LogFilter::default(), Some(33), 200);
        assert_eq!(few.lines.len(), 32);
        assert_eq!(few.cut, None, "the 32 lines left fit exactly");
        // The longest lines: 7 of them fit, never 8.
        let longest = line(&"m".repeat(MAX_MESSAGE_BYTES));
        let some = read(
            std::iter::repeat_n(&longest, 9),
            &LogFilter::default(),
            None,
            200,
        );
        assert_eq!(some.lines.len(), 7);
        assert_eq!(some.cut, Some(ReadCut::Bytes));
    }
}
