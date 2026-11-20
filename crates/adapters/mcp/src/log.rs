//! The tool that reads the daemon's recent log (§4.6, #957): its lines
//! newest first, narrowed by level as the Log page narrows them, and further
//! by when they were written and by an operation or a plan they name, a page
//! at a time. Either backend answers the whole recent log, as
//! `GET /api/v1/log` does; the server narrows and pages it with the domain's
//! rules, so both modes read alike (§16, decision 134).

use api::{ErrorCode, ErrorDto, LogLevelDto, LogLineDto, RecentLogDto};
use domain::daemon_log::{self, LogFilter, LogLevel, LogLine, ReadCut};
use domain::{OperationId, PlanId, Timestamp};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::arguments::invalid;
use crate::pages::page_length;

/// The arguments of `read_log`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct LogArguments {
    /// Only the lines at this level or worse, most severe first: `error`,
    /// `warn`, `info`, `debug` or `trace`, as the Log page's "Warnings and
    /// errors" is `warn`. Every line when not given.
    pub level: Option<String>,
    /// Only the lines written at this instant or later, in RFC 3339, such as
    /// `2026-11-20T09:00:00Z` or a line's `at`.
    pub since: Option<String>,
    /// Only the lines written at this instant or earlier, in RFC 3339.
    pub until: Option<String>,
    /// Only the lines that name this operation, by its ID, as
    /// `list_operations` gives it.
    pub operation: Option<String>,
    /// Only the lines that name this plan, by its ID: those saying
    /// `plan <id>`.
    pub plan: Option<String>,
    /// How many lines to read, a whole number from 1 to 200; 50 when not
    /// given. A larger number reads 200, and 0 one. An answer holds at most
    /// 32 KiB of text however many lines it is asked for.
    pub limit: Option<u64>,
    /// The `next` of the page before, to read the lines older than it.
    pub before: Option<u64>,
}

/// One page of the recent log, newest first.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct LogPage {
    pub lines: Vec<LogLineDto>,
    /// The `before` of the page after this one; `null` on the last page.
    pub next: Option<u64>,
    /// The cap that cut the page short; `null` on the last page.
    pub stopped: Option<LogCap>,
}

/// The cap that cut a page of the recent log short.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub enum LogCap {
    /// The page holds as many lines as it was asked for.
    Lines,
    /// The next line would take it past 32 KiB of text.
    Bytes,
}

/// The page of `log` that `arguments` ask for.
///
/// # Errors
///
/// `invalid-arguments`: a `since` or `until` that isn't an instant, an
/// `operation` no operation's ID could be, or a `plan` no plan's.
/// `daemon-unreachable`: a line of `log` whose time isn't one, which the
/// API never answers.
pub fn read_page(log: RecentLogDto, arguments: LogArguments) -> Result<LogPage, ErrorDto> {
    let filter = LogFilter {
        level: arguments
            .level
            .map(|name| level_named(&name).ok_or_else(|| invalid("level")))
            .transpose()?
            .unwrap_or(LogLevel::Trace),
        since: instant(arguments.since, "since")?,
        until: instant(arguments.until, "until")?,
        operation: arguments
            .operation
            .map(|id| {
                Uuid::parse_str(&id)
                    .map(OperationId::from_uuid)
                    .map_err(|_| invalid("operation"))
            })
            .transpose()?,
        plan: arguments
            .plan
            .map(|id| PlanId::parse(&id).map_err(|_| invalid("plan")))
            .transpose()?,
    };
    let lines = log
        .lines
        .into_iter()
        .map(line)
        .collect::<Result<Vec<_>, _>>()?;
    // A page is at most 200 lines long, which any target's `usize` holds.
    let limit = usize::try_from(page_length(arguments.limit)).unwrap_or(usize::MAX);
    let read = daemon_log::read(&lines, &filter, arguments.before, limit);
    Ok(LogPage {
        next: read
            .cut
            .and_then(|_| read.lines.last().map(|line| line.number())),
        stopped: read.cut.map(|cut| match cut {
            ReadCut::Lines => LogCap::Lines,
            ReadCut::Bytes => LogCap::Bytes,
        }),
        lines: read
            .lines
            .into_iter()
            .map(app::recent_log::line_dto)
            .collect(),
    })
}

/// The level a line of the API is at.
fn level(level: LogLevelDto) -> LogLevel {
    match level {
        LogLevelDto::Error => LogLevel::Error,
        LogLevelDto::Warn => LogLevel::Warn,
        LogLevelDto::Info => LogLevel::Info,
        LogLevelDto::Debug => LogLevel::Debug,
        LogLevelDto::Trace => LogLevel::Trace,
    }
}

/// The level `name` names, as a line of the API spells it.
fn level_named(name: &str) -> Option<LogLevel> {
    match name {
        "error" => Some(LogLevel::Error),
        "warn" => Some(LogLevel::Warn),
        "info" => Some(LogLevel::Info),
        "debug" => Some(LogLevel::Debug),
        "trace" => Some(LogLevel::Trace),
        _ => None,
    }
}

/// The instant `given` names, when it was given, as the argument `field`.
fn instant(given: Option<String>, field: &str) -> Result<Option<Timestamp>, ErrorDto> {
    given
        .map(|text| text.parse::<Timestamp>().map_err(|_| invalid(field)))
        .transpose()
}

/// A line of the API's answer as the log kept it.
fn line(line: LogLineDto) -> Result<LogLine, ErrorDto> {
    let Ok(at) = line.at.parse::<Timestamp>() else {
        return Err(ErrorDto::new(ErrorCode::DaemonUnreachable, [], None));
    };
    Ok(LogLine::new(
        at,
        level(line.level),
        &line.source,
        format_args!("{}", line.message),
    )
    .numbered(line.number))
}

#[cfg(test)]
mod tests;
