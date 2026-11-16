//! The Log page (technical design §4.4, #618), decided on the host: the
//! route it reads, the choice of levels and which lines each choice lists,
//! the words of a line's level, and the text "Copy for a bug report" puts on
//! the clipboard. `daemon_log_page.rs` is its glue.

use api::{BuildDto, LogLevelDto, LogLineDto};
use l10n::{MessageId, msg};

/// The route that answers the recent log, newest line first.
pub(crate) const LOG_PATH: &str = "/api/v1/log";

/// The choice of which lines the table lists: every line, or only those at
/// a level or worse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Levels {
    All,
    WarningsAndErrors,
    ErrorsOnly,
}

impl Levels {
    /// The choices in the order the page offers them, every line first.
    pub(crate) const ALL: [Self; 3] = [Self::All, Self::WarningsAndErrors, Self::ErrorsOnly];

    /// The choice's value in the radio group, which also makes its input's
    /// id.
    pub(crate) const fn value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::WarningsAndErrors => "warnings",
            Self::ErrorsOnly => "errors",
        }
    }

    /// The choice whose [`value`](Self::value) is `value`; every line for a
    /// value the group never sets.
    pub(crate) fn of(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|levels| levels.value() == value)
            .unwrap_or(Self::All)
    }

    /// The choice's label.
    pub(crate) const fn label(self) -> MessageId {
        match self {
            Self::All => msg::UI_LOG_LEVELS_ALL,
            Self::WarningsAndErrors => msg::UI_LOG_LEVELS_WARNINGS,
            Self::ErrorsOnly => msg::UI_LOG_LEVELS_ERRORS,
        }
    }

    /// Whether a line at `level` is listed under this choice.
    const fn lists(self, level: LogLevelDto) -> bool {
        match self {
            Self::All => true,
            Self::WarningsAndErrors => matches!(level, LogLevelDto::Error | LogLevelDto::Warn),
            Self::ErrorsOnly => matches!(level, LogLevelDto::Error),
        }
    }
}

/// The lines the table lists under `levels`, in the order the daemon
/// answered them: newest first.
pub(crate) fn shown(lines: &[LogLineDto], levels: Levels) -> Vec<LogLineDto> {
    lines
        .iter()
        .filter(|line| levels.lists(line.level))
        .cloned()
        .collect()
}

/// The words of a line's level in the table.
pub(crate) const fn level_name(level: LogLevelDto) -> MessageId {
    match level {
        LogLevelDto::Error => msg::UI_LOG_LEVEL_ERROR,
        LogLevelDto::Warn => msg::UI_LOG_LEVEL_WARN,
        LogLevelDto::Info => msg::UI_LOG_LEVEL_INFO,
        LogLevelDto::Debug => msg::UI_LOG_LEVEL_DEBUG,
        LogLevelDto::Trace => msg::UI_LOG_LEVEL_TRACE,
    }
}

/// A level as the log writes it, which a bug report quotes as it is, in any
/// language.
const fn log_word(level: LogLevelDto) -> &'static str {
    match level {
        LogLevelDto::Error => "ERROR",
        LogLevelDto::Warn => "WARN",
        LogLevelDto::Info => "INFO",
        LogLevelDto::Debug => "DEBUG",
        LogLevelDto::Trace => "TRACE",
    }
}

/// What "Copy for a bug report" puts on the clipboard: the program and the
/// build the daemon runs as `ladderback --version` prints them, then `lines`
/// in the order given, each as the log writes it (`[{at} {LEVEL} {source}]
/// {message}`, the level padded to five letters), every line ending in a line
/// break. A page the daemon hasn't yet told its build (`None`) leaves the
/// version out rather than guess it.
pub(crate) fn bug_report(build: Option<&BuildDto>, lines: &[LogLineDto]) -> String {
    let version = build.map(|build| format!("ladderback {}\n", build.display()));
    let lines = lines.iter().map(|line| {
        format!(
            "[{} {:<5} {}] {}\n",
            line.at,
            log_word(line.level),
            line.source,
            line.message
        )
    });
    version.into_iter().chain(lines).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: LogLevelDto, message: &str) -> LogLineDto {
        LogLineDto {
            at: "2026-11-15T09:00:01.250Z".to_owned(),
            level,
            source: "ladderback::config".to_owned(),
            message: message.to_owned(),
        }
    }

    fn every_level() -> Vec<LogLineDto> {
        [
            LogLevelDto::Trace,
            LogLevelDto::Debug,
            LogLevelDto::Info,
            LogLevelDto::Warn,
            LogLevelDto::Error,
        ]
        .into_iter()
        .map(|level| line(level, log_word(level)))
        .collect()
    }

    fn messages(lines: &[LogLineDto]) -> Vec<&str> {
        lines.iter().map(|line| line.message.as_str()).collect()
    }

    #[test]
    fn all_levels_lists_every_line_in_the_order_answered() {
        assert_eq!(
            messages(&shown(&every_level(), Levels::All)),
            ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]
        );
    }

    #[test]
    fn warnings_and_errors_lists_only_those_in_the_order_answered() {
        assert_eq!(
            messages(&shown(&every_level(), Levels::WarningsAndErrors)),
            ["WARN", "ERROR"]
        );
    }

    #[test]
    fn errors_only_lists_only_the_errors() {
        assert_eq!(
            messages(&shown(&every_level(), Levels::ErrorsOnly)),
            ["ERROR"]
        );
    }

    #[test]
    fn each_choice_is_found_again_by_its_value_and_an_unknown_value_lists_every_line() {
        for levels in Levels::ALL {
            assert_eq!(Levels::of(levels.value()), levels);
        }
        assert_eq!(Levels::of("debug"), Levels::All);
    }

    #[test]
    fn each_choice_and_each_level_has_words_of_its_own() {
        let labels: Vec<_> = Levels::ALL.into_iter().map(Levels::label).collect();
        assert_eq!(
            labels,
            [
                msg::UI_LOG_LEVELS_ALL,
                msg::UI_LOG_LEVELS_WARNINGS,
                msg::UI_LOG_LEVELS_ERRORS
            ]
        );
        let names: Vec<_> = every_level()
            .iter()
            .map(|line| level_name(line.level))
            .collect();
        assert_eq!(
            names,
            [
                msg::UI_LOG_LEVEL_TRACE,
                msg::UI_LOG_LEVEL_DEBUG,
                msg::UI_LOG_LEVEL_INFO,
                msg::UI_LOG_LEVEL_WARN,
                msg::UI_LOG_LEVEL_ERROR
            ]
        );
    }

    #[test]
    fn a_bug_report_starts_with_the_running_version_then_each_line_as_the_log_writes_it() {
        let build = BuildDto {
            version: "0.3.0".to_owned(),
            commit: Some("3f937cf0123456789".to_owned()),
            dirty: false,
            release: false,
        };
        let lines = [
            LogLineDto {
                at: "2026-11-15T09:00:02.500Z".to_owned(),
                level: LogLevelDto::Error,
                source: "ladderback::notify".to_owned(),
                message: "the notification to demo-channel failed".to_owned(),
            },
            line(LogLevelDto::Warn, "repository nas: a demo warning"),
            line(LogLevelDto::Info, "i"),
            line(LogLevelDto::Debug, "d"),
            line(LogLevelDto::Trace, "t"),
        ];
        assert_eq!(
            bug_report(Some(&build), &lines),
            "ladderback 0.3.0+3f937cf\n\
             [2026-11-15T09:00:02.500Z ERROR ladderback::notify] the notification to demo-channel failed\n\
             [2026-11-15T09:00:01.250Z WARN  ladderback::config] repository nas: a demo warning\n\
             [2026-11-15T09:00:01.250Z INFO  ladderback::config] i\n\
             [2026-11-15T09:00:01.250Z DEBUG ladderback::config] d\n\
             [2026-11-15T09:00:01.250Z TRACE ladderback::config] t\n"
        );
    }

    #[test]
    fn a_bug_report_leaves_out_a_version_the_page_has_not_been_told() {
        assert_eq!(
            bug_report(None, &[line(LogLevelDto::Warn, "w")]),
            "[2026-11-15T09:00:01.250Z WARN  ladderback::config] w\n"
        );
        assert_eq!(bug_report(None, &[]), "");
    }
}
