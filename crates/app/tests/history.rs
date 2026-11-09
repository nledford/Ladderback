//! The history use cases over an operation log, and the record history shows.
//! What every log must do (order, paging, the plan filter) is the port's
//! contract suite; this is what the use cases add: the page size a caller may
//! ask for, and what reaches the caller when the log can't answer.

use std::sync::Arc;

use app::fakes::InMemoryOperationStore;
use app::history::{GetOperation, ListOperations};
use app::ports::{
    EngineResult, HistoryPage, HistoryQuery, MAX_PAGE_SIZE, OperationDetail, OperationRecord,
    OperationStore, PageSize, Start, StoreError,
};
use async_trait::async_trait;
use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning, WarningKind,
};
use domain::{OperationId, PlanId, RepositoryId, Timestamp, Uuid};

fn id(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

fn plan(name: &str) -> PlanId {
    PlanId::parse(name).unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn queued(n: u128, plan_id: Option<&PlanId>) -> Operation {
    Operation::queue(
        id(n),
        OperationKind::Backup,
        Trigger::Schedule,
        Target {
            plan: plan_id.cloned(),
            repository: None,
        },
        at(0),
    )
}

/// A log holding the operations numbered `1..=count`, all of one plan unless
/// told otherwise.
async fn log_of(count: u128) -> Arc<InMemoryOperationStore> {
    let store = Arc::new(InMemoryOperationStore::new());
    let home = plan("demo-home");
    for n in 1..=count {
        store.save(&queued(n, Some(&home)), None).await.unwrap();
    }
    store
}

fn ids(page: &HistoryPage) -> Vec<OperationId> {
    page.operations.iter().map(|record| record.id).collect()
}

/// The ids `high` down to `low`, newest first.
fn newest_first(high: u128, low: u128) -> Vec<OperationId> {
    (low..=high).rev().map(id).collect()
}

#[tokio::test]
async fn a_caller_that_names_no_size_gets_the_default_page() {
    let history = ListOperations::new(log_of(60).await);

    let first = history.list(None, None, None).await.unwrap();
    assert_eq!(first.operations.len(), 50);
    assert_eq!(ids(&first), newest_first(60, 11));
    assert_eq!(first.next, Some(id(11)));

    let second = history.list(None, first.next, None).await.unwrap();
    assert_eq!(ids(&second), newest_first(10, 1));
    assert_eq!(second.next, None, "the last page says nothing follows");
}

#[tokio::test]
async fn a_caller_may_ask_for_a_page_of_any_size_up_to_the_cap() {
    let history = ListOperations::new(log_of(250).await);
    for size in [1, 7, 50, 199, 200] {
        let page = history.list(None, None, Some(size)).await.unwrap();
        assert_eq!(page.operations.len(), size, "a page of {size}");
    }
}

#[tokio::test]
async fn a_size_past_the_cap_is_cut_to_the_cap() {
    let history = ListOperations::new(log_of(250).await);
    for size in [201, 1_000, usize::MAX] {
        let page = history.list(None, None, Some(size)).await.unwrap();
        assert_eq!(page.operations.len(), 200, "asked for {size}");
        assert_eq!(page.next, Some(id(51)), "asked for {size}");
    }
}

#[tokio::test]
async fn a_size_of_zero_is_raised_to_one() {
    let history = ListOperations::new(log_of(3).await);
    let page = history.list(None, None, Some(0)).await.unwrap();
    assert_eq!(ids(&page), [id(3)]);
    assert_eq!(page.next, Some(id(3)));
}

#[tokio::test]
async fn paging_visits_every_operation_once_newest_first() {
    let history = ListOperations::new(log_of(23).await);
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = history.list(None, after, Some(5)).await.unwrap();
        seen.extend(ids(&page));
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, newest_first(23, 1));
}

#[tokio::test]
async fn a_plan_sees_only_its_own_operations_and_pages_through_them() {
    let store = Arc::new(InMemoryOperationStore::new());
    let (home, docs) = (plan("demo-home"), plan("demo-docs"));
    for n in 1..=9 {
        let owner = if n % 3 == 0 { &docs } else { &home };
        store.save(&queued(n, Some(owner)), None).await.unwrap();
    }
    store.save(&queued(10, None), None).await.unwrap();
    let history = ListOperations::new(store);

    let first = history
        .list(Some(docs.clone()), None, Some(2))
        .await
        .unwrap();
    assert_eq!(ids(&first), [id(9), id(6)]);
    assert_eq!(first.next, Some(id(6)));
    let second = history.list(Some(docs), first.next, Some(2)).await.unwrap();
    assert_eq!(ids(&second), [id(3)]);
    assert_eq!(second.next, None);

    let everything = history.list(None, None, None).await.unwrap();
    assert_eq!(everything.operations.len(), 10);
}

#[tokio::test]
async fn an_operation_is_found_by_its_id_with_its_warnings() {
    let store = Arc::new(InMemoryOperationStore::new());
    let mut op = queued(5, Some(&plan("demo-home")));
    op.start(at(1)).unwrap();
    let warning = Warning {
        kind: WarningKind::SourceUnreadable,
        item: Some("/home/demo/locked.txt".to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    };
    op.record_warning(warning.clone()).unwrap();
    op.succeed(at(9)).unwrap();
    let result = EngineResult::Fake { steps_completed: 2 };
    store.save(&op, Some(&result)).await.unwrap();
    store.save(&queued(6, None), None).await.unwrap();

    let found = GetOperation::new(store)
        .get(id(5))
        .await
        .unwrap()
        .expect("operation 5 is in the history");
    assert_eq!(found.record.id, id(5));
    assert_eq!(found.record.status, OperationStatus::SucceededWithWarnings);
    assert_eq!(found.record.warning_count, 1);
    assert_eq!(found.record.result, Some(result));
    assert_eq!(found.warnings, vec![warning]);
}

#[tokio::test]
async fn an_operation_that_was_never_logged_is_not_found() {
    let found = GetOperation::new(log_of(3).await).get(id(4)).await.unwrap();
    assert_eq!(found, None);
}

/// A log that can't answer.
struct Broken(StoreError);

#[async_trait]
impl OperationStore for Broken {
    async fn save(&self, _: &Operation, _: Option<&EngineResult>) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn try_start(&self, _: &Operation) -> Result<Start, StoreError> {
        Err(self.0.clone())
    }

    async fn get(&self, _: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        Err(self.0.clone())
    }

    async fn list(&self, _: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        Err(self.0.clone())
    }
}

#[tokio::test]
async fn what_the_log_could_not_do_reaches_the_caller() {
    for error in [
        StoreError::Unavailable {
            detail: "database is locked".to_owned(),
        },
        StoreError::Corrupt {
            detail: "operation 5: unknown status `bogus`".to_owned(),
        },
    ] {
        let store: Arc<dyn OperationStore> = Arc::new(Broken(error.clone()));
        assert_eq!(
            ListOperations::new(Arc::clone(&store))
                .list(None, None, None)
                .await,
            Err(error.clone())
        );
        assert_eq!(GetOperation::new(store).get(id(1)).await, Err(error));
    }
}

#[test]
fn a_page_holds_at_least_one_operation_and_at_most_the_cap() {
    assert_eq!(PageSize::new(0).get(), 1);
    assert_eq!(PageSize::new(1).get(), 1);
    assert_eq!(PageSize::new(2).get(), 2);
    assert_eq!(PageSize::new(MAX_PAGE_SIZE - 1).get(), MAX_PAGE_SIZE - 1);
    assert_eq!(PageSize::new(MAX_PAGE_SIZE).get(), MAX_PAGE_SIZE);
    assert_eq!(PageSize::new(MAX_PAGE_SIZE + 1).get(), MAX_PAGE_SIZE);
    assert_eq!(PageSize::new(usize::MAX).get(), MAX_PAGE_SIZE);
}

fn at_nanos(seconds: i64, nanos: i32) -> Timestamp {
    Timestamp::new(1_800_000_000 + seconds, nanos).unwrap()
}

#[test]
fn a_record_describes_the_operation_with_its_times_cut_to_the_millisecond() {
    let target = Target {
        plan: Some(plan("demo-home")),
        repository: Some(RepositoryId::parse(&"ab".repeat(32)).unwrap()),
    };
    let mut op = Operation::queue(
        id(7),
        OperationKind::Prune,
        Trigger::CatchUp,
        target.clone(),
        at_nanos(0, 123_999_999),
    );

    let waiting = OperationRecord::of(&op, None);
    assert_eq!(waiting.queued_at, at_nanos(0, 123_000_000));
    assert_eq!((waiting.started_at, waiting.finished_at), (None, None));
    assert_eq!(waiting.status, OperationStatus::Queued);

    op.start(at_nanos(1, 5_999_999)).unwrap();
    op.record_warning(Warning {
        kind: WarningKind::Other,
        item: None,
        during: "prune".to_owned(),
        detail: "odd".to_owned(),
    })
    .unwrap();
    op.succeed(at_nanos(2, 999_999)).unwrap();
    let result = EngineResult::Fake { steps_completed: 4 };

    let record = OperationRecord::of(&op, Some(&result));
    assert_eq!(record.id, id(7));
    assert_eq!(record.kind, OperationKind::Prune);
    assert_eq!(record.trigger, Trigger::CatchUp);
    assert_eq!(record.target, target);
    assert_eq!(record.status, OperationStatus::SucceededWithWarnings);
    assert_eq!(record.queued_at, at_nanos(0, 123_000_000));
    assert_eq!(record.started_at, Some(at_nanos(1, 5_000_000)));
    assert_eq!(record.finished_at, Some(at_nanos(2, 0)));
    assert_eq!(record.warning_count, 1);
    assert_eq!(record.result, Some(result));
    assert_eq!(OperationRecord::of(&op, None).result, None);
}

/// An operation of `kind` on `target`, once in each status it can have.
fn in_every_status(kind: OperationKind, target: &Target) -> Vec<Operation> {
    let queued = || Operation::queue(id(8), kind, Trigger::Manual, target.clone(), at(0));
    let running = || {
        let mut op = queued();
        op.start(at(1)).unwrap();
        op
    };
    let warning = Warning {
        kind: WarningKind::Other,
        item: None,
        during: "prune".to_owned(),
        detail: "odd".to_owned(),
    };
    let mut succeeded = running();
    succeeded.succeed(at(2)).unwrap();
    let mut with_warnings = running();
    with_warnings.record_warning(warning).unwrap();
    with_warnings.succeed(at(2)).unwrap();
    let mut failed = running();
    failed
        .fail(FailureReason::EngineExited { exit_code: None }, at(2))
        .unwrap();
    let mut cancelled = running();
    cancelled.cancel(at(2)).unwrap();
    vec![
        queued(),
        running(),
        succeeded,
        with_warnings,
        failed,
        cancelled,
    ]
}

#[test]
fn a_record_maintains_a_repository_only_while_a_maintenance_operation_is_running_on_it() {
    let repository = RepositoryId::parse(&"ab".repeat(32)).unwrap();
    let targets = [
        (
            Target {
                plan: Some(plan("demo-home")),
                repository: Some(repository.clone()),
            },
            true,
        ),
        (
            Target {
                plan: None,
                repository: Some(repository.clone()),
            },
            true,
        ),
        (
            Target {
                plan: Some(plan("demo-home")),
                repository: None,
            },
            false,
        ),
        (
            Target {
                plan: None,
                repository: None,
            },
            false,
        ),
    ];
    let kinds = [
        (OperationKind::Prune, true),
        (OperationKind::Check, true),
        (OperationKind::Backup, false),
        (OperationKind::Identify, false),
        (OperationKind::Fake, false),
    ];
    for (kind, is_maintenance) in kinds {
        for (target, names_a_repository) in &targets {
            for op in in_every_status(kind, target) {
                let record = OperationRecord::of(&op, None);
                let holds = is_maintenance
                    && *names_a_repository
                    && record.status == OperationStatus::Running;
                assert_eq!(
                    record.maintaining(),
                    holds.then_some(&repository),
                    "{kind:?} on {target:?} while {:?}",
                    record.status
                );
            }
        }
    }
}
