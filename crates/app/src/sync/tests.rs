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

/// A schedule store the agent's database can't read.
struct UnreadableSchedules;

fn locked() -> StoreError {
    StoreError::Unavailable {
        detail: "the database is locked".to_owned(),
    }
}

#[async_trait::async_trait]
impl crate::ports::ScheduleStore for UnreadableSchedules {
    async fn planned_runs(&self) -> Result<Vec<crate::ports::PlannedRun>, StoreError> {
        Err(locked())
    }

    async fn plan_run(&self, _: &crate::ports::PlannedRun) -> Result<(), StoreError> {
        Err(locked())
    }

    async fn unplan(&self, _: &domain::PlanId) -> Result<(), StoreError> {
        Err(locked())
    }

    async fn pending_retries(&self) -> Result<Vec<crate::ports::PendingRetry>, StoreError> {
        Err(locked())
    }

    async fn keep_retry(&self, _: &crate::ports::PendingRetry) -> Result<(), StoreError> {
        Err(locked())
    }

    async fn drop_retry(&self, _: &domain::PlanId) -> Result<(), StoreError> {
        Err(locked())
    }

    async fn release_check(&self) -> Result<Option<crate::ports::ReleaseCheck>, StoreError> {
        Err(locked())
    }

    async fn keep_release_check(&self, _: &crate::ports::ReleaseCheck) -> Result<(), StoreError> {
        Err(locked())
    }
}

/// A sync on `clock` that never reaches a hub, reading its plans from
/// `schedules` when given them.
fn sync_reading(
    clock: Arc<crate::fakes::FakeClock>,
    schedules: Option<Arc<dyn crate::ports::ScheduleStore>>,
) -> SyncWithHub {
    use crate::fakes::{
        FakeSigning, InMemoryConfigStore, InMemoryIdentity, InMemoryOperationStore,
        LoopbackHubConnector, SeededRandom,
    };

    let connect = ConnectToHub::new(
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        clock.clone(),
        Arc::new(LoopbackHubConnector::unanswered()),
        "0.5.0-agent".to_owned(),
        None,
    );
    let sync = SyncWithHub::new(
        connect,
        Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(3),
        ))),
        clock.clone(),
        AgentId::from_uuid(Uuid::from_u128(1)),
        Hostname::parse("laptop-1").unwrap(),
        None,
    );
    match schedules {
        Some(schedules) => sync.with_plans(ReadPlans::new(
            crate::status::ReadStatus::new(
                Arc::new(InMemoryConfigStore::new()),
                Arc::new(InMemoryOperationStore::new()),
                clock,
                domain::TimeZone::UTC,
            ),
            schedules,
        )),
        None => sync,
    }
}

fn a_clock() -> Arc<crate::fakes::FakeClock> {
    Arc::new(crate::fakes::FakeClock::new(
        Timestamp::from_second(1_800_000_030).unwrap(),
    ))
}

/// A sync given no plans never reads them, and so never tells of them.
#[tokio::test]
async fn a_sync_given_no_plans_has_none_due() {
    let sync = sync_reading(a_clock(), None);
    assert!(!sync.plans_due());
    assert!(!sync.live_due().await);
}

/// Plans are due at once, then again only once [`PLANS_CHECK`] passed
/// since they were read: not a millisecond before.
#[tokio::test]
async fn plans_are_due_at_first_and_then_every_plans_check() {
    let clock = a_clock();
    let sync = sync_reading(
        clock.clone(),
        Some(Arc::new(crate::fakes::InMemoryScheduleStore::new())),
    );
    assert!(sync.plans_due(), "never read yet");
    assert!(sync.plans_to_tell().await.is_some(), "the hub holds none");
    assert!(!sync.plans_due(), "just read");
    clock.advance(PLANS_CHECK - SignedDuration::from_millis(1));
    assert!(!sync.plans_due(), "a millisecond short");
    clock.advance(SignedDuration::from_millis(1));
    assert!(sync.plans_due());
}

/// Plans the hub doesn't hold yet make no live session before
/// [`PLANS_CHECK`] passed since they were read.
#[tokio::test]
async fn changed_plans_wait_for_the_plans_check_before_a_live_session() {
    let clock = a_clock();
    let sync = sync_reading(
        clock.clone(),
        Some(Arc::new(crate::fakes::InMemoryScheduleStore::new())),
    );
    assert!(sync.live_due().await, "plans never told");
    assert!(!sync.live_due().await, "read just now");
    clock.advance(PLANS_CHECK);
    assert!(sync.live_due().await);
}

/// A streak of plans that don't read is logged once, at its first.
#[tokio::test]
async fn plans_that_dont_read_are_logged_once_a_streak() {
    let sync = sync_reading(a_clock(), Some(Arc::new(UnreadableSchedules)));
    crate::test_log::capture_the_log();
    for _ in 0..3 {
        assert!(sync.plans_to_tell().await.is_none());
    }
    let logged = crate::test_log::logged_by("app::sync");
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        logged
            .first()
            .is_some_and(|(_, line)| line.starts_with("the plans for the hub couldn't be read")),
        "{logged:?}"
    );
}
