mod serialization;

use domain::execution::{
    FailureReason, LifecycleError, MAX_STORED_WARNINGS, Operation, OperationKind, OperationStatus,
    Target, Transition, Trigger, Warning, WarningKind,
};
use domain::{OperationId, PlanId, RepositoryId, SnapshotId, Timestamp, Uuid};

use crate::ports::{
    BackupSummary, CheckSummary, EngineResult, HistoryPage, HistoryQuery, OperationDetail,
    OperationRecord, OperationStore, PageSize, PruneSummary,
};

/// Run against an empty store: the first check is that it lists nothing. The
/// suite saves a few hundred operations, so a store behind a network or a
/// disk should be a scratch one.
pub async fn run(store: &dyn OperationStore) {
    let fixtures = Fixtures::new();
    assert!(
        fixtures.is_some(),
        "the suite's own fixture IDs are valid IDs"
    );
    let Some(fx) = fixtures else { return };

    an_empty_log_lists_nothing(store).await;
    an_operation_that_was_never_saved_is_not_found(store).await;
    a_saved_operation_reads_back_as_it_was_saved(store, &fx).await;
    a_later_save_replaces_the_earlier_one(store, &fx).await;
    every_status_and_failure_reason_reads_back(store).await;
    every_kind_and_trigger_reads_back(store, &fx).await;
    every_result_reads_back(store, &fx).await;
    warnings_read_back_whole_and_in_order(store).await;
    a_later_save_replaces_the_warnings_too(store).await;
    one_operations_warnings_are_not_anothers(store).await;
    warnings_past_the_cap_are_counted_not_stored(store).await;
    times_are_kept_to_the_millisecond(store).await;
    history_lists_the_newest_operation_first(store, &fx).await;
    a_page_ends_at_its_limit_and_says_whether_more_remain(store, &fx).await;
    a_page_continues_after_any_cursor(store, &fx).await;
    history_can_be_limited_to_one_plan(store, &fx).await;
    serialization::run(store, &fx).await;
}

/// IDs the suite can't build without a fallible parse.
struct Fixtures {
    home: PlanId,
    docs: PlanId,
    photos: PlanId,
    music: PlanId,
    videos: PlanId,
    empty: PlanId,
    repository: RepositoryId,
    snapshot: SnapshotId,
    parent: SnapshotId,
}

impl Fixtures {
    fn new() -> Option<Self> {
        Some(Self {
            home: PlanId::parse("demo-home").ok()?,
            docs: PlanId::parse("demo-docs").ok()?,
            photos: PlanId::parse("demo-photos").ok()?,
            music: PlanId::parse("demo-music").ok()?,
            videos: PlanId::parse("demo-videos").ok()?,
            empty: PlanId::parse("demo-empty").ok()?,
            repository: RepositoryId::parse(&"ab".repeat(32)).ok()?,
            snapshot: SnapshotId::parse(&"cd".repeat(32)).ok()?,
            parent: SnapshotId::parse(&"ef".repeat(32)).ok()?,
        })
    }
}

fn id(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

/// `seconds` after a fixed instant, on a whole second.
fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

/// A queued backup of `plan`, or of no plan.
fn queued(n: u128, plan: Option<&PlanId>) -> Operation {
    let target = Target {
        plan: plan.cloned(),
        repository: None,
    };
    Operation::queue(
        id(n),
        OperationKind::Backup,
        Trigger::Schedule,
        target,
        at(0),
    )
}

/// A lifecycle step of the suite's own setup must be allowed.
fn applied(step: Result<(), LifecycleError>) {
    assert!(
        step.is_ok(),
        "the suite broke the operation's lifecycle: {step:?}"
    );
}

fn started(mut op: Operation) -> Operation {
    applied(op.start(at(10)));
    op
}

fn succeeded(op: Operation) -> Operation {
    let mut op = started(op);
    applied(op.succeed(at(70)));
    op
}

fn failed(op: Operation, reason: FailureReason) -> Operation {
    let mut op = started(op);
    applied(op.fail(reason, at(70)));
    op
}

fn unreadable(item: &str) -> Warning {
    Warning {
        kind: WarningKind::SourceUnreadable,
        item: Some(item.to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    }
}

async fn save(store: &dyn OperationStore, op: &Operation, result: Option<&EngineResult>) {
    let saved = store.save(op, result).await;
    assert!(
        saved.is_ok(),
        "saving operation {} failed: {saved:?}",
        op.id().as_uuid()
    );
}

/// The operation as stored; the check that it exists is made here.
async fn read(store: &dyn OperationStore, id: OperationId) -> Option<OperationDetail> {
    let found = store.get(id).await;
    assert!(
        found.is_ok(),
        "reading operation {} failed: {found:?}",
        id.as_uuid()
    );
    found.unwrap_or(None)
}

async fn page(store: &dyn OperationStore, query: &HistoryQuery) -> HistoryPage {
    let listed = store.list(query).await;
    assert!(listed.is_ok(), "listing {query:?} failed: {listed:?}");
    listed.unwrap_or(HistoryPage {
        operations: Vec::new(),
        next: None,
    })
}

fn ids(records: &[OperationRecord]) -> Vec<OperationId> {
    records.iter().map(|record| record.id).collect()
}

fn query(plan: Option<&PlanId>, after: Option<OperationId>, limit: usize) -> HistoryQuery {
    HistoryQuery {
        plan: plan.cloned(),
        after,
        limit: PageSize::new(limit),
    }
}

/// Every page of one listing, in order, checking each page's shape on the way:
/// no longer than `size`, newest first with no ID twice, entirely below the
/// cursor it continues from, and ending at its `next`.
async fn listing(
    store: &dyn OperationStore,
    plan: Option<&PlanId>,
    size: usize,
) -> Vec<OperationRecord> {
    const MOST_PAGES: usize = 10_000;
    let mut records = Vec::new();
    let mut after = None;
    let mut ended = false;
    for _ in 0..MOST_PAGES {
        let this = page(store, &query(plan, after, size)).await;
        let listed = ids(&this.operations);
        assert!(
            listed.len() <= size,
            "a page holds at most {size}: {listed:?}"
        );
        assert!(
            listed.windows(2).all(|pair| pair[0] > pair[1]),
            "a page lists the newest first, each operation once: {listed:?}"
        );
        if let Some(cursor) = after {
            assert!(
                listed.iter().all(|each| *each < cursor),
                "a page after {} holds only older operations: {listed:?}",
                cursor.as_uuid()
            );
        }
        records.extend(this.operations);
        let Some(next) = this.next else {
            ended = true;
            break;
        };
        assert_eq!(
            listed.last(),
            Some(&next),
            "`next` is the last operation of its page"
        );
        after = Some(next);
    }
    assert!(ended, "a listing ends within {MOST_PAGES} pages");
    records
}

async fn an_empty_log_lists_nothing(store: &dyn OperationStore) {
    assert_eq!(
        page(store, &HistoryQuery::default()).await,
        HistoryPage {
            operations: Vec::new(),
            next: None,
        }
    );
}

async fn an_operation_that_was_never_saved_is_not_found(store: &dyn OperationStore) {
    assert_eq!(read(store, id(0x9999)).await, None);
}

async fn a_saved_operation_reads_back_as_it_was_saved(store: &dyn OperationStore, fx: &Fixtures) {
    let target = Target {
        plan: Some(fx.home.clone()),
        repository: Some(fx.repository.clone()),
    };
    let op = Operation::queue(
        id(0x10),
        OperationKind::Backup,
        Trigger::Schedule,
        target.clone(),
        at(0),
    );
    save(store, &op, None).await;

    let found = read(store, id(0x10)).await;
    assert!(found.is_some(), "a saved operation is found");
    let Some(found) = found else { return };
    assert_eq!(found.record, OperationRecord::of(&op, None));
    assert_eq!(found.warnings, Vec::new());
    // The record field by field, so a wrong `OperationRecord::of` can't hide
    // behind a store that agrees with it.
    let record = found.record;
    assert_eq!(record.id, id(0x10));
    assert_eq!(record.kind, OperationKind::Backup);
    assert_eq!(record.trigger, Trigger::Schedule);
    assert_eq!(record.target, target);
    assert_eq!(record.status, OperationStatus::Queued);
    assert_eq!(record.queued_at, at(0));
    assert_eq!(record.started_at, None);
    assert_eq!(record.finished_at, None);
    assert_eq!(record.warning_count, 0);
    assert_eq!(record.result, None);
}

async fn a_later_save_replaces_the_earlier_one(store: &dyn OperationStore, fx: &Fixtures) {
    let mut op = queued(0x20, Some(&fx.home));
    save(store, &op, None).await;

    applied(op.start(at(10)));
    save(store, &op, None).await;
    let running = read(store, id(0x20)).await.map(|found| found.record);
    assert_eq!(running, Some(OperationRecord::of(&op, None)));
    assert_eq!(
        running.as_ref().map(|record| &record.status),
        Some(&OperationStatus::Running)
    );
    assert_eq!(
        running.and_then(|record| record.started_at),
        Some(at(10)),
        "the start time is kept"
    );

    applied(op.record_warning(unreadable("/home/demo/locked.txt")));
    applied(op.succeed(at(70)));
    let result = EngineResult::Fake { steps_completed: 3 };
    save(store, &op, Some(&result)).await;
    let found = read(store, id(0x20)).await;
    assert!(found.is_some(), "the operation is still there");
    let Some(found) = found else { return };
    assert_eq!(found.record, OperationRecord::of(&op, Some(&result)));
    assert_eq!(found.record.status, OperationStatus::SucceededWithWarnings);
    assert_eq!(found.record.queued_at, at(0));
    assert_eq!(found.record.started_at, Some(at(10)));
    assert_eq!(found.record.finished_at, Some(at(70)));
    assert_eq!(found.record.warning_count, 1);
    assert_eq!(found.record.result, Some(result));
    assert_eq!(found.warnings, vec![unreadable("/home/demo/locked.txt")]);

    let listed = listing(store, Some(&fx.home), 10).await;
    let once = ids(&listed)
        .iter()
        .filter(|listed| **listed == id(0x20))
        .count();
    assert_eq!(once, 1, "an operation saved three times is listed once");
}

async fn every_status_and_failure_reason_reads_back(store: &dyn OperationStore) {
    let awkward = "unknown field `foo` at line 1 column 9\nsecond \"line\", 'single', \\ and ☃ ✓ 日本語\ttab \u{0} nul";
    let reasons = [
        FailureReason::EngineUnavailable {
            detail: "/usr/local/bin/steading: No such file or directory (os error 2)".to_owned(),
        },
        FailureReason::EngineUnavailable {
            detail: String::new(),
        },
        FailureReason::JobRejected {
            detail: awkward.to_owned(),
        },
        FailureReason::EngineExited { exit_code: Some(0) },
        FailureReason::EngineExited { exit_code: Some(3) },
        FailureReason::EngineExited {
            exit_code: Some(-1),
        },
        FailureReason::EngineExited { exit_code: None },
        FailureReason::EngineFailed {
            detail: "repository not found".to_owned(),
        },
        FailureReason::EngineFailed {
            detail: "x".repeat(64 * 1024),
        },
        FailureReason::ProtocolViolated {
            transition: Transition::Start,
        },
        FailureReason::ProtocolViolated {
            transition: Transition::ReportProgress,
        },
        FailureReason::ProtocolViolated {
            transition: Transition::RecordWarning,
        },
        FailureReason::ProtocolViolated {
            transition: Transition::Succeed,
        },
        FailureReason::ProtocolViolated {
            transition: Transition::Fail,
        },
        FailureReason::ProtocolViolated {
            transition: Transition::Cancel,
        },
        FailureReason::DeadlinePassed,
        FailureReason::EngineStartTimedOut,
        FailureReason::EngineExitTimedOut,
    ];

    let mut ops = vec![
        queued(0x30, None),
        started(queued(0x31, None)),
        succeeded(queued(0x32, None)),
    ];
    let mut with_warning = started(queued(0x33, None));
    applied(with_warning.record_warning(unreadable("/home/demo/a")));
    applied(with_warning.succeed(at(70)));
    ops.push(with_warning);
    let mut cancelled_in_the_queue = queued(0x34, None);
    applied(cancelled_in_the_queue.cancel(at(5)));
    ops.push(cancelled_in_the_queue);
    let mut cancelled_while_running = started(queued(0x35, None));
    applied(cancelled_while_running.cancel(at(40)));
    ops.push(cancelled_while_running);
    // Failures: one that happened before the operation started, and one for every reason.
    let mut never_started = queued(0x36, None);
    applied(never_started.fail(reasons[0].clone(), at(1)));
    ops.push(never_started);
    for (n, reason) in (0x40..).zip(reasons) {
        ops.push(failed(queued(n, None), reason));
    }

    for op in &ops {
        save(store, op, None).await;
    }
    for op in &ops {
        let found = read(store, op.id()).await;
        assert!(found.is_some(), "operation {} is found", op.id().as_uuid());
        let Some(found) = found else { return };
        assert_eq!(
            &found.record.status,
            op.status(),
            "operation {} keeps its status",
            op.id().as_uuid()
        );
        assert_eq!(found.record, OperationRecord::of(op, None));
    }
}

async fn every_kind_and_trigger_reads_back(store: &dyn OperationStore, fx: &Fixtures) {
    let kinds = [
        OperationKind::Fake,
        OperationKind::Identify,
        OperationKind::Backup,
        OperationKind::Prune,
        OperationKind::Check,
    ];
    let triggers = [
        Trigger::Schedule,
        Trigger::Manual,
        Trigger::CatchUp,
        Trigger::Retry,
        Trigger::Hub,
        Trigger::Mcp,
    ];
    let targets = [
        Target::default(),
        Target {
            plan: Some(fx.home.clone()),
            repository: None,
        },
        Target {
            plan: None,
            repository: Some(fx.repository.clone()),
        },
    ];
    let mut n = 0x90;
    for trigger in triggers {
        for kind in kinds {
            let target = targets[(n % 3) as usize].clone();
            let op = Operation::queue(id(n), kind, trigger, target, at(0));
            save(store, &op, None).await;
            let found = read(store, id(n)).await.map(|found| found.record);
            assert_eq!(
                found,
                Some(OperationRecord::of(&op, None)),
                "{kind:?} triggered by {trigger:?} reads back"
            );
            assert_eq!(found.as_ref().map(|record| record.kind), Some(kind));
            assert_eq!(found.as_ref().map(|record| record.trigger), Some(trigger));
            assert_eq!(found.map(|record| record.target), Some(op.target().clone()));
            n += 1;
        }
    }
}

async fn every_result_reads_back(store: &dyn OperationStore, fx: &Fixtures) {
    let backup = BackupSummary {
        repository: fx.repository.clone(),
        snapshot: fx.snapshot.clone(),
        parent: Some(fx.parent.clone()),
        files_new: 1,
        files_changed: 2,
        files_unmodified: 3,
        // Past what a signed 64-bit column holds: a count is never narrowed.
        data_added: u64::MAX,
        data_added_packed: 5,
        error_count: 6,
    };
    let results = [
        EngineResult::Fake {
            steps_completed: u32::MAX,
        },
        EngineResult::Repository {
            id: fx.repository.clone(),
        },
        EngineResult::Backup(BackupSummary {
            parent: None,
            ..backup.clone()
        }),
        EngineResult::Backup(backup),
        EngineResult::Prune(PruneSummary {
            repository: fx.repository.clone(),
            packs_marked: 1,
            packs_deleted: 2,
            packs_recovered: 3,
        }),
        EngineResult::Check(CheckSummary {
            repository: fx.repository.clone(),
            errors: 4,
            warnings: 5,
        }),
    ];
    for (n, result) in (0xb0..).zip(results) {
        let op = succeeded(queued(n, None));
        save(store, &op, Some(&result)).await;
        let found = read(store, id(n)).await.map(|found| found.record);
        assert_eq!(
            found.as_ref().map(|record| &record.result),
            Some(&Some(result.clone())),
            "{result:?} reads back"
        );
        assert_eq!(found, Some(OperationRecord::of(&op, Some(&result))));
    }
}

async fn warnings_read_back_whole_and_in_order(store: &dyn OperationStore) {
    let mut op = started(queued(0xc0, None));
    let odd = [
        Warning {
            kind: WarningKind::Other,
            item: None,
            during: "scan".to_owned(),
            detail: "something new".to_owned(),
        },
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some("/home/demo/données ☃/日本語.txt".to_owned()),
            during: String::new(),
            detail: String::new(),
        },
        // An empty item is not a missing one.
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some(String::new()),
            during: "backup".to_owned(),
            detail: "line one\nline \"two\"".to_owned(),
        },
    ];
    for warning in odd {
        applied(op.record_warning(warning));
    }
    for n in 0..40 {
        applied(op.record_warning(unreadable(&format!("/home/demo/file-{n}"))));
    }
    applied(op.succeed(at(70)));
    save(store, &op, None).await;

    let found = read(store, id(0xc0)).await;
    assert!(found.is_some(), "the operation is found");
    let Some(found) = found else { return };
    assert_eq!(found.warnings, op.warnings(), "in the order they came");
    assert_eq!(found.record.warning_count, 43);
}

async fn a_later_save_replaces_the_warnings_too(store: &dyn OperationStore) {
    let mut op = started(queued(0xc1, None));
    for n in 0..3 {
        applied(op.record_warning(unreadable(&format!("/home/demo/first-{n}"))));
    }
    save(store, &op, None).await;
    for n in 0..2 {
        applied(op.record_warning(unreadable(&format!("/home/demo/later-{n}"))));
    }
    save(store, &op, None).await;

    let found = read(store, id(0xc1)).await;
    assert!(found.is_some(), "the operation is found");
    let Some(found) = found else { return };
    assert_eq!(found.warnings, op.warnings(), "five warnings, not eight");
    assert_eq!(found.record.warning_count, 5);
}

async fn one_operations_warnings_are_not_anothers(store: &dyn OperationStore) {
    let mut two = started(queued(0xc2, None));
    let mut three = started(queued(0xc3, None));
    let none = started(queued(0xc4, None));
    for n in 0..2 {
        applied(two.record_warning(unreadable(&format!("/home/demo/two-{n}"))));
    }
    for n in 0..3 {
        applied(three.record_warning(unreadable(&format!("/home/demo/three-{n}"))));
    }
    for op in [&two, &three, &none] {
        save(store, op, None).await;
    }
    for op in [&two, &three, &none] {
        let found = read(store, op.id()).await;
        assert_eq!(
            found.map(|found| found.warnings),
            Some(op.warnings().to_vec()),
            "operation {} has its own warnings",
            op.id().as_uuid()
        );
    }
}

async fn warnings_past_the_cap_are_counted_not_stored(store: &dyn OperationStore) {
    let mut op = started(queued(0xc5, None));
    let total = MAX_STORED_WARNINGS + 7;
    for n in 0..total {
        applied(op.record_warning(unreadable(&format!("/home/demo/file-{n}"))));
    }
    applied(op.succeed(at(70)));
    save(store, &op, None).await;

    let found = read(store, id(0xc5)).await;
    assert!(found.is_some(), "the operation is found");
    let Some(found) = found else { return };
    assert_eq!(
        found.record.warning_count, total as u64,
        "the total is kept"
    );
    assert_eq!(found.warnings.len(), MAX_STORED_WARNINGS);
    assert_eq!(found.warnings, op.warnings(), "the first ones are kept");
}

async fn times_are_kept_to_the_millisecond(store: &dyn OperationStore) {
    let mut op = Operation::queue(
        id(0xc6),
        OperationKind::Fake,
        Trigger::Manual,
        Target::default(),
        Timestamp::constant(1_800_000_000, 123_456_789),
    );
    applied(op.start(Timestamp::constant(1_800_000_010, 999_999_999)));
    applied(op.succeed(Timestamp::constant(1_800_000_020, 1)));
    save(store, &op, None).await;

    let found = read(store, id(0xc6)).await.map(|found| found.record);
    assert_eq!(
        found.as_ref().map(|record| record.queued_at),
        Some(Timestamp::constant(1_800_000_000, 123_000_000))
    );
    assert_eq!(
        found.as_ref().and_then(|record| record.started_at),
        Some(Timestamp::constant(1_800_000_010, 999_000_000)),
        "milliseconds are cut, never rounded up"
    );
    assert_eq!(
        found.as_ref().and_then(|record| record.finished_at),
        Some(Timestamp::constant(1_800_000_020, 0))
    );
    assert_eq!(found, Some(OperationRecord::of(&op, None)));
}

async fn history_lists_the_newest_operation_first(store: &dyn OperationStore, fx: &Fixtures) {
    // Saved out of order: the order is the IDs', not the order of saving.
    let saved = [0xe3_u128, 0xe6, 0xe1, 0xe5, 0xe2, 0xe4];
    let mut ops = Vec::new();
    for n in saved {
        let op = succeeded(queued(n, Some(&fx.videos)));
        save(store, &op, None).await;
        ops.push(op);
    }

    let everything = listing(store, None, 4).await;
    let ours: Vec<OperationId> = ids(&everything)
        .into_iter()
        .filter(|listed| saved.iter().any(|n| *listed == id(*n)))
        .collect();
    let newest_first: Vec<OperationId> = (0xe1..=0xe6_u128).rev().map(id).collect();
    assert_eq!(ours, newest_first);
    for op in &ops {
        let listed = everything.iter().find(|record| record.id == op.id());
        assert_eq!(listed, Some(&OperationRecord::of(op, None)));
    }
}

async fn a_page_ends_at_its_limit_and_says_whether_more_remain(
    store: &dyn OperationStore,
    fx: &Fixtures,
) {
    for n in 0xf1..=0xf6 {
        save(store, &queued(n, Some(&fx.docs)), None).await;
    }
    let docs = Some(&fx.docs);
    let newest_first: Vec<OperationId> = (0xf1..=0xf6_u128).rev().map(id).collect();

    for (limit, first_page, next) in [
        (1, 1, Some(0xf6)),
        (3, 3, Some(0xf4)),
        (5, 5, Some(0xf2)),
        (6, 6, None),
        (7, 6, None),
    ] {
        let this = page(store, &query(docs, None, limit)).await;
        assert_eq!(
            ids(&this.operations),
            newest_first[..first_page],
            "limit {limit}"
        );
        assert_eq!(this.next, next.map(id), "limit {limit}");
    }

    let second = page(store, &query(docs, Some(id(0xf4)), 3)).await;
    assert_eq!(ids(&second.operations), [id(0xf3), id(0xf2), id(0xf1)]);
    assert_eq!(second.next, None, "the last page, filled exactly, ends");
    let rest = page(store, &query(docs, Some(id(0xf2)), 5)).await;
    assert_eq!(ids(&rest.operations), [id(0xf1)]);
    assert_eq!(rest.next, None);

    assert_eq!(ids(&listing(store, docs, 2).await), newest_first);
}

async fn a_page_continues_after_any_cursor(store: &dyn OperationStore, fx: &Fixtures) {
    let docs = Some(&fx.docs);
    // No operation has this ID, and it is above every one the plan has.
    let above = page(store, &query(docs, Some(id(0xff)), 10)).await;
    assert_eq!(above.operations.len(), 6);
    assert_eq!(above.next, None);
    // The cursor itself is excluded, and nothing is older than the oldest.
    let oldest = page(store, &query(docs, Some(id(0xf1)), 10)).await;
    assert_eq!(
        oldest,
        HistoryPage {
            operations: Vec::new(),
            next: None
        }
    );
    let below = page(store, &query(docs, Some(id(0)), 10)).await;
    assert_eq!(
        below,
        HistoryPage {
            operations: Vec::new(),
            next: None
        }
    );
}

async fn history_can_be_limited_to_one_plan(store: &dyn OperationStore, fx: &Fixtures) {
    save(store, &queued(0x101, Some(&fx.photos)), None).await;
    save(store, &queued(0x102, Some(&fx.music)), None).await;
    save(store, &queued(0x103, Some(&fx.photos)), None).await;
    save(store, &queued(0x104, None), None).await;

    let photos = listing(store, Some(&fx.photos), 10).await;
    assert_eq!(ids(&photos), [id(0x103), id(0x101)]);
    assert!(
        photos
            .iter()
            .all(|record| record.target.plan.as_ref() == Some(&fx.photos)),
        "{photos:?}"
    );
    assert_eq!(ids(&listing(store, Some(&fx.music), 10).await), [id(0x102)]);
    let one_at_a_time = page(store, &query(Some(&fx.photos), Some(id(0x103)), 1)).await;
    assert_eq!(ids(&one_at_a_time.operations), [id(0x101)]);
    assert_eq!(one_at_a_time.next, None);
    // A plan nothing ran for lists nothing, and no plan lists another's operations.
    assert_eq!(listing(store, Some(&fx.empty), 10).await, Vec::new());
    assert_eq!(listing(store, Some(&fx.videos), 10).await.len(), 6);
}
