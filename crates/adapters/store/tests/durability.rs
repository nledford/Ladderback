//! What the log keeps across closing, reopening and a second handle on the
//! same file: the history outlives the process that wrote it.

mod support;

use app::ports::{
    EngineResult, HistoryPage, HistoryQuery, OperationDetail, OperationRecord, OperationStore,
};
use domain::execution::{FailureReason, Operation, OperationKind, Target, Trigger};
use sqlx::Row;
use store::SqliteStore;
use support::*;

/// A queued, a running, a finished, a failed and a cancelled operation, with
/// warnings and a result among them, and what each was saved with.
fn a_mix() -> Vec<(Operation, Option<EngineResult>)> {
    let queued_one = queued_backup(0x10);
    let running = with_warnings(started(queued_backup(0x11)), 3);
    let mut finished = with_warnings(started(queued_backup(0x12)), 2);
    finished
        .succeed(at(70))
        .expect("a running operation succeeds");
    let failed_one = failed(
        queued(
            0x13,
            OperationKind::Prune,
            Trigger::Manual,
            Target::default(),
        ),
        FailureReason::EngineFailed {
            detail: "repository not found".to_owned(),
        },
    );
    let mut cancelled = queued(
        0x14,
        OperationKind::Check,
        Trigger::Retry,
        Target {
            plan: None,
            repository: Some(repository()),
        },
    );
    cancelled.cancel(at(5)).expect("a queued operation cancels");
    vec![
        (queued_one, None),
        (running, None),
        (finished, Some(backup_result())),
        (failed_one, None),
        (cancelled, None),
    ]
}

/// What the log must say about `saved`, worked out from the operations alone.
fn expected(saved: &[(Operation, Option<EngineResult>)]) -> (Vec<OperationDetail>, HistoryPage) {
    let mut details: Vec<OperationDetail> = saved
        .iter()
        .map(|(op, result)| OperationDetail {
            record: OperationRecord::of(op, result.as_ref()),
            warnings: op.warnings().to_vec(),
        })
        .collect();
    details.sort_by_key(|detail| std::cmp::Reverse(detail.record.id));
    let page = HistoryPage {
        operations: details.iter().map(|d| d.record.clone()).collect(),
        next: None,
    };
    (details, page)
}

async fn read_back(
    store: &SqliteStore,
    saved: &[(Operation, Option<EngineResult>)],
) -> (Vec<OperationDetail>, HistoryPage) {
    let (details, _) = expected(saved);
    let mut found = Vec::new();
    for detail in &details {
        let one = store.get(detail.record.id).await;
        assert!(one.is_ok(), "reading {:?}: {one:?}", detail.record.id);
        found.push(one.unwrap_or(None).expect("the operation is there"));
    }
    let page = store
        .list(&HistoryQuery::default())
        .await
        .expect("listing works");
    (found, page)
}

#[tokio::test]
async fn operations_survive_closing_and_reopening_the_store() {
    let scratch = Scratch::new();
    let saved = a_mix();
    let store = scratch.open().await;
    for (op, result) in &saved {
        save(&store, op, result.as_ref()).await;
    }
    store.close().await;

    let reopened = scratch.open().await;
    let (details, page) = within(30, "reading back", read_back(&reopened, &saved)).await;
    let (expected_details, expected_page) = expected(&saved);
    assert_eq!(details, expected_details);
    assert_eq!(page, expected_page);
}

#[tokio::test]
async fn a_store_dropped_without_closing_has_lost_nothing() {
    let scratch = Scratch::new();
    let saved = a_mix();
    let store = scratch.open().await;
    for (op, result) in &saved {
        save(&store, op, result.as_ref()).await;
    }
    // The way a killed process leaves it: no `close`, no checkpoint of its own.
    drop(store);

    let reopened = scratch.open().await;
    let (details, page) = within(30, "reading back", read_back(&reopened, &saved)).await;
    let (expected_details, expected_page) = expected(&saved);
    assert_eq!(details, expected_details);
    assert_eq!(page, expected_page);
}

/// The migration rows as the database holds them.
async fn migration_rows(scratch: &Scratch) -> Vec<(i64, String, bool, Vec<u8>, i64, String)> {
    let mut raw = scratch.raw().await;
    let rows = sqlx::query(
        "SELECT version, description, success, checksum, execution_time, CAST(installed_on AS TEXT) AS installed_on
         FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut raw)
    .await
    .expect("the migration table is readable");
    rows.iter()
        .map(|row| {
            (
                row.get("version"),
                row.get("description"),
                row.get("success"),
                row.get("checksum"),
                row.get("execution_time"),
                row.get("installed_on"),
            )
        })
        .collect()
}

#[tokio::test]
async fn reopening_does_not_apply_the_migrations_again() {
    let scratch = Scratch::new();
    scratch.open().await.close().await;
    let first = migration_rows(&scratch).await;
    let versions: Vec<i64> = first.iter().map(|row| row.0).collect();
    assert_eq!(
        versions,
        [1, 2, 3, 4, 5, 6, 7],
        "seven migrations ship: {first:?}"
    );
    assert!(
        first.iter().all(|row| row.2),
        "and each is recorded as applied"
    );

    let store = scratch.open().await;
    let op = queued_backup(0x10);
    save(&store, &op, None).await;
    store.close().await;
    scratch.open().await.close().await;

    assert_eq!(migration_rows(&scratch).await, first);
    let reopened = scratch.open().await;
    let found = reopened.get(id(0x10)).await.expect("reading works");
    assert_eq!(
        found.map(|detail| detail.record),
        Some(OperationRecord::of(&op, None)),
        "what was saved is still there after the second reopening"
    );
}

#[tokio::test]
async fn a_second_handle_on_the_same_file_sees_what_the_first_wrote_and_the_other_way_round() {
    let scratch = Scratch::new();
    let first = scratch.open().await;
    let second = scratch.open().await;

    let by_first = succeeded(queued_backup(0x20));
    let result = backup_result();
    save(&first, &by_first, Some(&result)).await;
    let by_second = with_warnings(started(queued_backup(0x21)), 4);
    save(&second, &by_second, None).await;

    for store in [&first, &second] {
        let one = store.get(id(0x20)).await.expect("reading works");
        assert_eq!(
            one,
            Some(OperationDetail {
                record: OperationRecord::of(&by_first, Some(&result)),
                warnings: Vec::new(),
            })
        );
        let two = store.get(id(0x21)).await.expect("reading works");
        assert_eq!(
            two,
            Some(OperationDetail {
                record: OperationRecord::of(&by_second, None),
                warnings: by_second.warnings().to_vec(),
            })
        );
        let page = store
            .list(&HistoryQuery::default())
            .await
            .expect("listing works");
        assert_eq!(
            page.operations.iter().map(|r| r.id).collect::<Vec<_>>(),
            [id(0x21), id(0x20)],
            "both handles list both operations"
        );
    }

    // A later save through one handle replaces what the other one wrote.
    let mut later = by_second.clone();
    later.succeed(at(90)).expect("a running operation succeeds");
    save(&first, &later, None).await;
    let seen = second.get(id(0x21)).await.expect("reading works");
    assert_eq!(
        seen.map(|detail| detail.record),
        Some(OperationRecord::of(&later, None))
    );
}
