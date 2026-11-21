use api::ErrorCode;

use super::*;
use crate::arguments::invalid;

/// A line numbered `number`, written `number` seconds into a minute.
fn line(number: u64, level: LogLevelDto, message: &str) -> LogLineDto {
    LogLineDto {
        at: format!("2026-11-20T09:00:{number:02}.000Z"),
        level,
        source: "ladderback".to_owned(),
        message: message.to_owned(),
        number,
    }
}

/// The recent log of `lines`, given oldest first, as the API answers it:
/// newest first.
fn log(mut lines: Vec<LogLineDto>) -> RecentLogDto {
    lines.reverse();
    RecentLogDto { lines }
}

fn arguments() -> LogArguments {
    LogArguments {
        level: None,
        since: None,
        until: None,
        operation: None,
        plan: None,
        limit: None,
        before: None,
    }
}

fn numbers(page: &LogPage) -> Vec<u64> {
    page.lines.iter().map(|line| line.number).collect()
}

#[test]
fn a_page_keeps_the_lines_at_the_level_or_worse_newest_first_as_they_were_answered() {
    let lines = vec![
        line(1, LogLevelDto::Error, "e"),
        line(2, LogLevelDto::Info, "i"),
        line(3, LogLevelDto::Warn, "w"),
        line(4, LogLevelDto::Trace, "t"),
    ];
    let page = read_page(
        log(lines.clone()),
        LogArguments {
            level: Some("warn".to_owned()),
            ..arguments()
        },
    )
    .unwrap();
    assert_eq!(
        page,
        LogPage {
            lines: vec![lines[2].clone(), lines[0].clone()],
            next: None,
            stopped: None,
        }
    );
}

#[test]
fn a_page_cut_by_its_limit_says_so_and_the_next_starts_below_its_last_line() {
    let lines = (1..=5)
        .map(|number| line(number, LogLevelDto::Info, "i"))
        .collect();
    let first = read_page(
        log(lines),
        LogArguments {
            limit: Some(2),
            ..arguments()
        },
    )
    .unwrap();
    assert_eq!(numbers(&first), [5, 4]);
    assert_eq!((first.next, first.stopped), (Some(4), Some(LogCap::Lines)));
    let lines = (1..=5)
        .map(|number| line(number, LogLevelDto::Info, "i"))
        .collect();
    let last = read_page(
        log(lines),
        LogArguments {
            limit: Some(0),
            before: Some(2),
            ..arguments()
        },
    )
    .unwrap();
    assert_eq!(numbers(&last), [1]);
    assert_eq!((last.next, last.stopped), (None, None));
}

#[test]
fn a_page_cut_by_its_bytes_says_so() {
    let long = "m".repeat(4096);
    let lines = (1..=9)
        .map(|number| line(number, LogLevelDto::Info, &long))
        .collect();
    let page = read_page(
        log(lines),
        LogArguments {
            limit: Some(500),
            ..arguments()
        },
    )
    .unwrap();
    assert_eq!(numbers(&page), [9, 8, 7, 6, 5, 4, 3]);
    assert_eq!((page.next, page.stopped), (Some(3), Some(LogCap::Bytes)));
}

#[test]
fn a_page_keeps_the_lines_of_a_time_range_an_operation_and_a_plan() {
    let id = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    let lines = vec![
        line(
            1,
            LogLevelDto::Info,
            &format!("operation {id} ended: Failed"),
        ),
        line(
            2,
            LogLevelDto::Warn,
            "the manual run of plan home didn't start",
        ),
        line(
            3,
            LogLevelDto::Info,
            &format!("operation {id} ended: Succeeded"),
        ),
        line(
            4,
            LogLevelDto::Info,
            "the manual run of plan homework didn't start",
        ),
    ];
    let of_operation = read_page(
        log(lines.clone()),
        LogArguments {
            operation: Some(id.to_uppercase()),
            since: Some("2026-11-20T09:00:02Z".to_owned()),
            ..arguments()
        },
    )
    .unwrap();
    assert_eq!(numbers(&of_operation), [3]);
    let of_plan = read_page(
        log(lines),
        LogArguments {
            plan: Some("home".to_owned()),
            until: Some("2026-11-20T09:00:03Z".to_owned()),
            ..arguments()
        },
    )
    .unwrap();
    assert_eq!(numbers(&of_plan), [2]);
}

#[test]
fn an_argument_no_filter_can_take_is_refused_by_its_name() {
    for (wrong, field) in [
        (
            LogArguments {
                level: Some("warning".to_owned()),
                ..arguments()
            },
            "level",
        ),
        (
            LogArguments {
                since: Some("yesterday".to_owned()),
                ..arguments()
            },
            "since",
        ),
        (
            LogArguments {
                until: Some("2026-13-01T00:00:00Z".to_owned()),
                ..arguments()
            },
            "until",
        ),
        (
            LogArguments {
                operation: Some("not-an-operation".to_owned()),
                ..arguments()
            },
            "operation",
        ),
        (
            LogArguments {
                plan: Some("Not A Plan".to_owned()),
                ..arguments()
            },
            "plan",
        ),
    ] {
        assert_eq!(
            read_page(log(Vec::new()), wrong),
            Err(invalid(field)),
            "{field}"
        );
    }
}

#[test]
fn a_line_whose_time_isnt_one_is_an_answer_the_api_never_gives() {
    let mut broken = line(1, LogLevelDto::Info, "i");
    broken.at = "a while ago".to_owned();
    let refused = read_page(log(vec![broken]), arguments()).unwrap_err();
    assert_eq!(refused.code, ErrorCode::DaemonUnreachable);
}
