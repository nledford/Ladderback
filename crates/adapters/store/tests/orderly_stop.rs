//! What the SQLite store does to the log as an orderly stop closes it
//! (decision 92): every running operation ends failed with the reason the
//! daemon gives, at the stop's time, in one transaction, and the log is
//! closed after it; a failure part way through ends none, so the next start
//! ends them as after a crash, and queued and finished operations are left
//! as they were.

mod support;

use std::assert_matches;

use app::ports::{OperationStore, Start, StoreError};
use domain::execution::{FailureReason, Operation, OperationKind};
use support::*;

/// `op` as an orderly stop at `at(60)` ends it.
fn shut_down(mut op: Operation) -> Operation {
    op.fail(FailureReason::DaemonShutDown, at(60))
        .expect("a running operation fails");
    op
}

#[tokio::test]
async fn an_orderly_stop_ends_each_running_operation_at_its_time_and_leaves_the_rest() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let running = with_warnings(started(queued_backup(0x10)), 2);
    let waiting = queued_backup(0x11);
    let done = succeeded(queued_backup(0x12));
    for op in [&running, &waiting, &done] {
        save(&store, op, None).await;
    }

    let ended = within(
        30,
        "the close",
        store.close_ending_running(&FailureReason::DaemonShutDown, at(60)),
    )
    .await;
    assert_eq!(ended, Ok(1), "one operation was running");
    assert_matches!(
        store.get(running.id()).await,
        Err(StoreError::Unavailable { .. }),
        "the log is closed"
    );

    let again = scratch.open().await;
    assert_eq!(
        again.get(running.id()).await,
        Ok(Some(detail_of(&shut_down(running.clone()), None))),
        "the running operation ended failed at the stop's time, its warnings kept"
    );
    assert_eq!(
        again.get(waiting.id()).await,
        Ok(Some(detail_of(&waiting, None)))
    );
    assert_eq!(again.get(done.id()).await, Ok(Some(detail_of(&done, None))));
    let recovered = again
        .recover(&FailureReason::DaemonStopped)
        .await
        .expect("recovery works");
    assert_eq!(
        recovered.ended,
        Vec::new(),
        "the next start has nothing to end"
    );
    again.close().await;
}

/// A backup whose snapshot was written when the stop came, as one in its
/// after hooks, keeps the snapshot on its record: History links it.
#[tokio::test]
async fn an_orderly_stop_keeps_the_snapshot_of_a_backup_it_ends() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let running = started(queued_backup(0x10));
    let result = backup_result();
    save(&store, &running, Some(&result)).await;

    assert_eq!(
        store
            .close_ending_running(&FailureReason::DaemonShutDown, at(60))
            .await,
        Ok(1)
    );

    let again = scratch.open().await;
    assert_eq!(
        again.get(running.id()).await,
        Ok(Some(detail_of(&shut_down(running.clone()), Some(&result)))),
        "the backup ended at the stop's time, its snapshot kept"
    );
    again.close().await;
}

#[tokio::test]
async fn an_orderly_stop_frees_the_repository_a_running_maintenance_operation_held() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let holder = started(queued_on_repository(0x10, OperationKind::Prune));
    assert_eq!(store.try_start(&holder).await, Ok(Start::Started));

    assert_eq!(
        store
            .close_ending_running(&FailureReason::DaemonShutDown, at(60))
            .await,
        Ok(1)
    );

    let again = scratch.open().await;
    assert_eq!(
        again
            .try_start(&started(queued_on_repository(0x11, OperationKind::Check)))
            .await,
        Ok(Start::Started),
        "the prune no longer holds the repository"
    );
    again.close().await;
}

#[tokio::test]
async fn an_orderly_stop_whose_end_fails_part_way_ends_none_and_still_closes_the_log() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let numbers = [0x10, 0x11, 0x12];
    for n in numbers {
        save(&store, &started(queued_backup(n)), None).await;
    }
    let mut before = Vec::new();
    for n in numbers {
        before.push(stored_row(&mut raw, n).await);
    }
    // The update of the middle row fails, as a full disk would there.
    let refuse = format!(
        "CREATE TRIGGER refuse_poisoned BEFORE UPDATE ON operations
         WHEN NEW.id = '{}'
         BEGIN SELECT RAISE(ABORT, 'boom'); END",
        id_text(0x11)
    );
    sqlx::query(sqlx::AssertSqlSafe(refuse))
        .execute(&mut raw)
        .await
        .expect("the fault is set up");

    let ended = store
        .close_ending_running(&FailureReason::DaemonShutDown, at(60))
        .await;
    assert_matches!(ended, Err(StoreError::Unavailable { .. }));
    assert_matches!(
        store.get(id(0x10)).await,
        Err(StoreError::Unavailable { .. }),
        "the log is closed all the same"
    );
    for (n, before) in numbers.into_iter().zip(before) {
        assert_eq!(
            stored_row(&mut raw, n).await,
            before,
            "row {n:#x} is still running, for the next start to end"
        );
    }
}
