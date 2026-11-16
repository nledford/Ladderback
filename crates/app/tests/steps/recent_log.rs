//! Steps for the untagged rule of `features/execution/daemon_log.feature`:
//! the recent log the daemon keeps, through its port, with the in-memory
//! recent log (the daemon's own) and the scenario's fake clock.

use std::sync::Arc;

use app::fakes::InMemoryRecentLog;
use app::ports::{Clock, RecentLogStore};
use cucumber::{given, then};
use domain::daemon_log::{LogLevel, LogLine, MAX_BYTES, MAX_MESSAGE_BYTES};

use super::Agent;

/// The source each line of these scenarios is written by.
const SOURCE: &str = "ladderback";

pub struct RecentLogWorld {
    log: Arc<dyn RecentLogStore>,
}

impl Default for RecentLogWorld {
    fn default() -> Self {
        Self {
            log: Arc::new(InMemoryRecentLog::new()),
        }
    }
}

impl std::fmt::Debug for RecentLogWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentLogWorld").finish_non_exhaustive()
    }
}

fn logs(agent: &Agent, message: &str) {
    let at = agent.clock.now();
    agent.recent_log.log.record(LogLine::new(
        at,
        LogLevel::Info,
        SOURCE,
        format_args!("{message}"),
    ));
}

fn messages(agent: &Agent) -> Vec<String> {
    agent
        .recent_log
        .log
        .newest_first()
        .iter()
        .map(|line| line.message().to_owned())
        .collect()
}

/// The number a line of a numbered run says, as its message writes it:
/// zero-padded to the line's length.
fn number(message: &str) -> u64 {
    message
        .trim_start_matches('0')
        .parse()
        .unwrap_or_else(|_| panic!("{message:?} is no numbered line"))
}

fn numbered(agent: &Agent, count: usize, bytes: usize) {
    for line in 1..=count {
        logs(agent, &format!("{line:0>bytes$}"));
    }
}

#[given(expr = "the daemon has logged {string}, {string} and {string}, in that order")]
fn logged_three(agent: &mut Agent, first: String, second: String, third: String) {
    for message in [first, second, third] {
        logs(agent, &message);
    }
}

#[given(expr = "the daemon has logged {int} lines of {int} bytes each, numbered from 1")]
fn logged_numbered(agent: &mut Agent, count: usize, bytes: usize) {
    numbered(agent, count, bytes);
}

#[given(expr = "the daemon has logged {int} lines of 4 KiB each, numbered from 1")]
fn logged_numbered_kib(agent: &mut Agent, count: usize) {
    numbered(agent, count, MAX_MESSAGE_BYTES);
}

#[given(expr = "the daemon has logged a message of {int} {string}")]
fn logged_repeated(agent: &mut Agent, count: usize, text: String) {
    logs(agent, &text.repeat(count));
}

#[then(expr = "the recent log lists {string}, {string} and {string}, in that order")]
fn lists_three(agent: &mut Agent, first: String, second: String, third: String) {
    assert_eq!(messages(agent), [first, second, third]);
}

#[then(expr = "the recent log holds {int} lines")]
fn holds_lines(agent: &mut Agent, count: usize) {
    assert_eq!(messages(agent).len(), count);
}

#[then(expr = "its newest line is line {int} and its oldest line {int}")]
fn newest_and_oldest(agent: &mut Agent, newest: u64, oldest: u64) {
    let kept = messages(agent);
    assert_eq!(kept.first().map(|line| number(line)), Some(newest));
    assert_eq!(kept.last().map(|line| number(line)), Some(oldest));
}

#[then("the recent log holds no more than 1 MiB of text")]
fn within_the_bytes(agent: &mut Agent) {
    let lines = agent.recent_log.log.newest_first();
    let bytes: usize = lines
        .iter()
        .map(|line| line.source().len() + line.message().len())
        .sum();
    assert!(bytes <= MAX_BYTES, "{bytes} bytes in {} lines", lines.len());
}

#[then(expr = "its newest line is line {int}")]
fn newest_line(agent: &mut Agent, line: u64) {
    assert_eq!(messages(agent).first().map(|kept| number(kept)), Some(line));
}

#[then(expr = "it doesn't hold line {int}")]
fn not_held(agent: &mut Agent, gone: u64) {
    let kept = messages(agent);
    assert!(!kept.is_empty(), "the recent log holds nothing");
    assert!(
        kept.iter().all(|line| number(line) != gone),
        "line {gone} is still held"
    );
}

#[then(
    expr = "the recent log's newest message is 4 KiB at most, ends in {string} and holds only {string} before it"
)]
fn cut(agent: &mut Agent, ending: String, text: String) {
    let kept = messages(agent);
    let message = kept.first().expect("a line");
    assert!(
        message.len() <= MAX_MESSAGE_BYTES,
        "{} bytes",
        message.len()
    );
    let before = message
        .strip_suffix(&ending)
        .unwrap_or_else(|| panic!("{message:?} doesn't end in {ending:?}"));
    assert!(!before.is_empty());
    assert_eq!(before.replace(&text, ""), "", "{before:?}");
}
