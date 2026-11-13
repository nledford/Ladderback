//! A save is all or nothing: one that fails halfway changes nothing, and
//! leaves the store as usable as it was.

mod support;

use std::assert_matches;

use app::ports::{OperationStore, StoreError};
use support::*;

/// Makes storing a warning for `/home/demo/boom` fail, as a full disk would
/// at the same point: after the operation's row is written and its old
/// warnings are gone.
const REFUSE_BOOM: &str = "CREATE TRIGGER refuse_boom BEFORE INSERT ON op_warnings
    WHEN NEW.item = '/home/demo/boom'
    BEGIN SELECT RAISE(ABORT, 'boom'); END";

#[tokio::test]
async fn a_save_that_fails_halfway_leaves_the_old_state_exactly_as_it_was() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let old = with_warnings(started(queued_backup(0x10)), 3);
    save(&store, &old, None).await;
    let seen_before = store.get(id(0x10)).await.expect("reading works");
    let row_before = stored_row(&mut raw, 0x10).await;
    let warnings_before = stored_warnings(&mut raw, 0x10).await;
    assert_eq!(warnings_before.len(), 3);

    // A newer state of the same operation, whose fourth warning can't be stored.
    let mut newer = old.clone();
    newer
        .record_warning(warning("/home/demo/boom"))
        .expect("a running operation records warnings");
    newer.succeed(at(70)).expect("a running operation succeeds");
    sqlx::query(REFUSE_BOOM)
        .execute(&mut raw)
        .await
        .expect("the fault is set up");
    let outcome = store.save(&newer, Some(&backup_result())).await;

    assert_matches!(outcome, Err(StoreError::Unavailable { .. }));
    assert_eq!(
        store.get(id(0x10)).await.expect("reading works"),
        seen_before
    );
    assert_eq!(
        stored_row(&mut raw, 0x10).await,
        row_before,
        "the row is as it was"
    );
    assert_eq!(
        stored_warnings(&mut raw, 0x10).await,
        warnings_before,
        "and so are its warnings, in their order"
    );
}

#[tokio::test]
async fn a_first_save_that_fails_leaves_no_trace() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    sqlx::query(REFUSE_BOOM)
        .execute(&mut raw)
        .await
        .expect("the fault is set up");
    let mut op = started(queued_backup(0x20));
    op.record_warning(warning("/home/demo/fine"))
        .expect("a running operation records warnings");
    op.record_warning(warning("/home/demo/boom"))
        .expect("a running operation records warnings");

    let outcome = store.save(&op, None).await;

    assert_matches!(outcome, Err(StoreError::Unavailable { .. }));
    assert_eq!(rows_of(&mut raw, "operations", "id", 0x20).await, 0);
    assert_eq!(rows_of(&mut raw, "op_warnings", "op_id", 0x20).await, 0);
    assert_eq!(store.get(id(0x20)).await, Ok(None));
}

#[tokio::test]
async fn the_store_is_as_usable_as_before_once_a_save_has_failed() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    sqlx::query(REFUSE_BOOM)
        .execute(&mut raw)
        .await
        .expect("the fault is set up");
    let mut op = started(queued_backup(0x30));
    op.record_warning(warning("/home/demo/boom"))
        .expect("a running operation records warnings");

    // More failures than the store has connections, so each is seen again.
    for attempt in 0..8 {
        let outcome = store.save(&op, None).await;
        assert_matches!(
            outcome,
            Err(StoreError::Unavailable { .. }),
            "attempt {attempt}: {outcome:?}"
        );
    }
    // Other saves and the reads work as ever.
    let other = succeeded(queued_backup(0x31));
    save(&store, &other, None).await;
    assert!(store.get(id(0x31)).await.expect("reading works").is_some());

    // And the very operation that failed is saved once the fault is gone.
    sqlx::query("DROP TRIGGER refuse_boom")
        .execute(&mut raw)
        .await
        .expect("the fault is gone");
    save(&store, &op, None).await;
    let found = store.get(id(0x30)).await.expect("reading works");
    assert_eq!(
        found.map(|detail| detail.warnings),
        Some(op.warnings().to_vec())
    );
}
