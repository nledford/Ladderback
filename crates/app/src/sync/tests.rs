use std::sync::Arc;

use api::{ErrorCode, MsgArg, UiEvent};
use domain::fleet::{
    AgentReport, EpochSeconds, Happening, MAX_OUTBOX_REPORTS, OutboxEpoch, ReportSeq, WireName,
};
use domain::{OperationId, SignedDuration, Uuid};

use super::*;
use crate::fakes::InMemoryOutbox;
use crate::ports::Outbox;

const OP: &str = "0190f3a0-0000-7000-8000-000000000001";

fn op() -> OperationId {
    OperationId::from_uuid(Uuid::parse_str(OP).unwrap())
}

fn at(seconds: u64) -> EpochSeconds {
    EpochSeconds::from_seconds(seconds)
}

#[test]
fn the_wait_after_a_failure_doubles_from_a_second_up_to_five_minutes() {
    assert_eq!(backoff(None), SignedDuration::from_secs(1));
    assert_eq!(
        backoff(Some(SignedDuration::from_secs(1))),
        SignedDuration::from_secs(2)
    );
    assert_eq!(
        backoff(Some(SignedDuration::from_secs(64))),
        SignedDuration::from_secs(128)
    );
    assert_eq!(
        backoff(Some(SignedDuration::from_secs(256))),
        LONGEST_BACKOFF
    );
    assert_eq!(backoff(Some(LONGEST_BACKOFF)), LONGEST_BACKOFF);
    assert_eq!(backoff(Some(SignedDuration::MAX)), LONGEST_BACKOFF);
}

#[test]
fn an_operations_start_and_end_are_reports() {
    let started = UiEvent::OperationStarted {
        operation: OP.to_owned(),
        repository: Some("nas".to_owned()),
        started_at: "2027-01-15T08:00:00Z".to_owned(),
    };
    assert_eq!(
        RecordReports::report_of(&started),
        Some(AgentReport {
            op_id: op(),
            happening: Happening::Started,
            at: at(1_800_000_000),
        })
    );
    let succeeded = UiEvent::OperationSucceeded {
        operation: OP.to_owned(),
        finished_at: "2027-01-15T08:00:09.5Z".to_owned(),
    };
    assert_eq!(
        RecordReports::report_of(&succeeded),
        Some(AgentReport {
            op_id: op(),
            happening: Happening::Succeeded,
            at: at(1_800_000_009),
        })
    );
}

/// A cancel is an end too, which the outbox keeps however full it is.
#[test]
fn a_cancel_is_an_end_report() {
    let cancelled = UiEvent::OperationCancelled {
        operation: OP.to_owned(),
        finished_at: "2027-01-15T08:00:09Z".to_owned(),
    };
    let report = RecordReports::report_of(&cancelled);
    assert_eq!(
        report,
        Some(AgentReport {
            op_id: op(),
            happening: Happening::Cancelled,
            at: at(1_800_000_009),
        })
    );
    assert!(report.is_some_and(|report| report.kept()));
}

#[test]
fn a_failure_reports_its_code_and_none_of_its_arguments() {
    let failed = UiEvent::OperationFailed {
        operation: OP.to_owned(),
        code: ErrorCode::Unknown,
        args: vec![MsgArg::Text("/home/someone/secret-plans.txt".to_owned())],
        finished_at: "2027-01-15T08:01:00Z".to_owned(),
    };
    let report = RecordReports::report_of(&failed).expect("a report");
    assert_eq!(
        report.happening,
        Happening::Failed {
            code: WireName::parse(&api::sync::code_name(ErrorCode::Unknown)).unwrap()
        }
    );
    assert_eq!(report.at, at(1_800_000_060));
}

#[test]
fn progress_warnings_and_unreadable_events_are_no_reports() {
    let progress = UiEvent::OperationProgress {
        operation: OP.to_owned(),
        step: None,
        unit: api::ProgressUnit::default(),
        current: 1,
        total: None,
        at: None,
    };
    assert_eq!(RecordReports::report_of(&progress), None);
    let other_id = UiEvent::OperationSucceeded {
        operation: "not-an-id".to_owned(),
        finished_at: "2027-01-15T03:00:00Z".to_owned(),
    };
    assert_eq!(RecordReports::report_of(&other_id), None);
    let other_time = UiEvent::OperationSucceeded {
        operation: OP.to_owned(),
        finished_at: "yesterday".to_owned(),
    };
    assert_eq!(RecordReports::report_of(&other_time), None);
    let before_1970 = UiEvent::OperationSucceeded {
        operation: OP.to_owned(),
        finished_at: "1969-12-31T23:59:59Z".to_owned(),
    };
    assert_eq!(RecordReports::report_of(&before_1970), None);
}

#[tokio::test]
async fn a_recorded_event_is_kept_in_order_and_an_event_with_no_report_keeps_nothing() {
    let outbox = Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(3),
    )));
    let record = RecordReports::new(outbox.clone());
    let started = UiEvent::OperationStarted {
        operation: OP.to_owned(),
        repository: None,
        started_at: "2027-01-15T03:00:00Z".to_owned(),
    };
    let progress = UiEvent::OperationProgress {
        operation: OP.to_owned(),
        step: None,
        unit: api::ProgressUnit::default(),
        current: 1,
        total: None,
        at: None,
    };
    assert_eq!(
        record.record(&started).await,
        Ok(Some(Appended::Kept(ReportSeq::FIRST)))
    );
    assert_eq!(record.record(&progress).await, Ok(None));
    let held = outbox.unacknowledged(None, 10).await.unwrap();
    assert_eq!(held.len(), 1);
}

fn event(happening: &Happening) -> UiEvent {
    let at = "2027-01-15T03:00:00Z".to_owned();
    match happening {
        Happening::Started => UiEvent::OperationStarted {
            operation: OP.to_owned(),
            repository: None,
            started_at: at,
        },
        _ => UiEvent::OperationSucceeded {
            operation: OP.to_owned(),
            finished_at: at,
        },
    }
}

/// A full outbox gives up its oldest start for a new report, and says so
/// in the log, with the place it gave up and the operation it kept a
/// report of, never silently.
#[tokio::test]
async fn a_full_outbox_gives_up_its_oldest_start_and_says_so() {
    let outbox = Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(3),
    )));
    let record = RecordReports::new(outbox.clone());
    crate::test_log::capture_the_log();
    for _ in 0..MAX_OUTBOX_REPORTS {
        record.record(&event(&Happening::Started)).await.unwrap();
    }
    assert_eq!(crate::test_log::logged_by("app::sync"), []);
    let kept = record.record(&event(&Happening::Succeeded)).await.unwrap();
    assert_eq!(
        kept,
        Some(Appended::KeptGivingUp {
            seq: ReportSeq::new(MAX_OUTBOX_REPORTS + 1).unwrap(),
            gave_up: ReportSeq::FIRST,
        })
    );
    let first = outbox.unacknowledged(None, 1).await.unwrap();
    assert_eq!(first.first().map(|entry| entry.seq.get()), Some(2));
    assert_eq!(
        crate::test_log::logged_by("app::sync"),
        [(
            log::Level::Warn,
            format!(
                "the outbox holds 10000 reports the hub hasn't acknowledged: it gave up its \
                 oldest start, at place 1, to keep a report of operation {OP}"
            )
        )]
    );
}

/// An outbox full of ends refuses a new report, and says so in the log.
#[tokio::test]
async fn an_outbox_full_of_ends_refuses_a_report_and_says_so() {
    let outbox = Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(3),
    )));
    let record = RecordReports::new(outbox.clone());
    crate::test_log::capture_the_log();
    for _ in 0..MAX_OUTBOX_REPORTS {
        record.record(&event(&Happening::Succeeded)).await.unwrap();
    }
    let refused = record.record(&event(&Happening::Started)).await.unwrap();
    assert_eq!(refused, Some(Appended::Refused));
    assert_eq!(
        crate::test_log::logged_by("app::sync"),
        [(
            log::Level::Warn,
            format!(
                "the outbox holds 10000 ends the hub hasn't acknowledged, or ran out of \
                 places: a report of operation {OP} wasn't kept"
            )
        )]
    );
}
