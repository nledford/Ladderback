//! What the SQLite store does to a log a stopped daemon left behind: the
//! unfinished operations end failed with `daemon-stopped`, in the columns and
//! across opens of the same file, and a failure part way through ends none.

mod support;

use app::ports::{OperationDetail, OperationRecord, OperationStore, Start, StoreError};
use domain::execution::{FailureReason, OperationKind, OperationStatus};
use support::*;

fn daemon_stopped() -> OperationStatus {
    OperationStatus::Failed {
        reason: FailureReason::DaemonStopped,
    }
}

#[tokio::test]
async fn an_operation_a_closed_store_left_running_is_ended_by_the_next_one_to_open() {
    let scratch = Scratch::new();
    let running = with_warnings(started(queued_backup(0x10)), 2);
    let waiting = queued_backup(0x11);

    let first = scratch.open().await;
    save(&first, &running, None).await;
    save(&first, &waiting, None).await;
    first.close().await;

    let ended = |op: &domain::execution::Operation| OperationDetail {
        record: OperationRecord {
            status: daemon_stopped(),
            finished_at: None,
            ..OperationRecord::of(op, None)
        },
        warnings: op.warnings().to_vec(),
    };

    let second = scratch.open().await;
    let count = within(
        30,
        "recovery",
        second.fail_unfinished(&FailureReason::DaemonStopped),
    )
    .await;
    assert_eq!(count, Ok(2), "both unfinished operations are ended");
    for op in [&running, &waiting] {
        assert_eq!(
            second.get(op.id()).await,
            Ok(Some(ended(op))),
            "operation {} reads as failed: daemon-stopped",
            op.id().as_uuid()
        );
    }
    second.close().await;

    let third = scratch.open().await;
    for op in [&running, &waiting] {
        assert_eq!(
            third.get(op.id()).await,
            Ok(Some(ended(op))),
            "operation {} is still failed when the file is opened again",
            op.id().as_uuid()
        );
    }
    assert_eq!(
        third.fail_unfinished(&FailureReason::DaemonStopped).await,
        Ok(0),
        "nothing is left to end"
    );
    third.close().await;
}

#[tokio::test]
async fn recovery_changes_only_the_status_and_failure_columns_of_unfinished_rows() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    let mut cancelled = started(queued_backup(0x15));
    cancelled
        .cancel(at(40))
        .expect("a running operation cancels");
    let mut warned = with_warnings(started(queued_backup(0x16)), 1);
    warned
        .succeed(at(70))
        .expect("a running operation succeeds");

    save(
        &store,
        &with_warnings(started(queued_backup(0x10)), 2),
        None,
    )
    .await;
    save(&store, &queued_backup(0x11), None).await;
    save(
        &store,
        &succeeded(queued_backup(0x12)),
        Some(&backup_result()),
    )
    .await;
    save(
        &store,
        &failed(
            queued_backup(0x13),
            FailureReason::EngineFailed {
                detail: "disk full".to_owned(),
            },
        ),
        None,
    )
    .await;
    save(
        &store,
        &failed(
            queued_backup(0x14),
            FailureReason::EngineExited { exit_code: Some(7) },
        ),
        None,
    )
    .await;
    save(&store, &cancelled, None).await;
    save(&store, &warned, None).await;

    let unfinished = [0x10, 0x11];
    let finished = [0x12, 0x13, 0x14, 0x15, 0x16];
    let mut rows_before = Vec::new();
    let mut warnings_before = Vec::new();
    for n in unfinished.into_iter().chain(finished) {
        rows_before.push((n, stored_row(&mut raw, n).await));
        warnings_before.push((n, stored_warnings(&mut raw, n).await));
    }

    assert_eq!(
        store.fail_unfinished(&FailureReason::DaemonStopped).await,
        Ok(2)
    );

    for ((n, before), (_, warnings)) in rows_before.into_iter().zip(warnings_before) {
        let after = stored_row(&mut raw, n).await;
        if unfinished.contains(&n) {
            assert!(
                before.status == "queued" || before.status == "running",
                "operation {n:#x} was unfinished: {before:?}"
            );
            let expected = Stored {
                status: "failed".to_owned(),
                error_code: Some("daemon-stopped".to_owned()),
                error_args: Some("[]".to_owned()),
                error_detail: None,
                ..before
            };
            assert_eq!(
                after, expected,
                "only the status and failure columns of {n:#x} changed"
            );
        } else {
            assert_eq!(after, before, "finished row {n:#x} is identical");
        }
        assert_eq!(
            stored_warnings(&mut raw, n).await,
            warnings,
            "the warnings of {n:#x} are as they were"
        );
    }
}

#[tokio::test]
async fn a_failure_part_way_through_leaves_every_row_as_it_was() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    save(&store, &queued_backup(0x10), None).await;
    save(
        &store,
        &with_warnings(started(queued_backup(0x11)), 1),
        None,
    )
    .await;
    save(&store, &queued_backup(0x12), None).await;
    save(&store, &started(queued_backup(0x13)), None).await;
    save(
        &store,
        &started(queued_on_repository(0x14, OperationKind::Prune)),
        None,
    )
    .await;
    save(&store, &queued_backup(0x15), None).await;
    save(&store, &succeeded(queued_backup(0x16)), None).await;

    let numbers = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16];
    let mut rows_before = Vec::new();
    let mut seen_before = Vec::new();
    for n in numbers {
        rows_before.push(stored_row(&mut raw, n).await);
        seen_before.push(store.get(id(n)).await.expect("reading works"));
    }

    // The update of one unfinished row in the middle of the log fails, as a
    // full disk would at that point.
    let refuse = format!(
        "CREATE TRIGGER refuse_poisoned BEFORE UPDATE ON operations
         WHEN NEW.id = '{}'
         BEGIN SELECT RAISE(ABORT, 'boom'); END",
        id_text(0x13)
    );
    sqlx::query(&refuse)
        .execute(&mut raw)
        .await
        .expect("the fault is set up");

    let outcome = store.fail_unfinished(&FailureReason::DaemonStopped).await;
    assert!(
        matches!(outcome, Err(StoreError::Unavailable { .. })),
        "{outcome:?}"
    );

    for ((n, before), seen) in numbers.into_iter().zip(rows_before).zip(seen_before) {
        assert_eq!(
            stored_row(&mut raw, n).await,
            before,
            "row {n:#x} is as it was"
        );
        assert_eq!(
            store.get(id(n)).await.expect("reading works"),
            seen,
            "operation {n:#x} reads as it did"
        );
    }
}

#[tokio::test]
async fn recovery_frees_the_repository_a_running_maintenance_operation_held_across_the_unique_index()
 {
    let scratch = Scratch::new();
    let store = scratch.open().await;

    let holder = started(queued_on_repository(0x10, OperationKind::Prune));
    assert_eq!(store.try_start(&holder).await, Ok(Start::Started));
    let blocked = store
        .save(
            &started(queued_on_repository(0x11, OperationKind::Prune)),
            None,
        )
        .await;
    assert!(
        matches!(blocked, Err(StoreError::Unavailable { .. })),
        "the unique index holds the repository while the prune runs: {blocked:?}"
    );

    assert_eq!(
        store.fail_unfinished(&FailureReason::DaemonStopped).await,
        Ok(1)
    );

    let next = started(queued_on_repository(0x12, OperationKind::Prune));
    save(&store, &next, None).await;
    assert_eq!(
        store.get(id(0x12)).await,
        Ok(Some(detail_of(&next, None))),
        "a running prune is stored once the old one is ended"
    );
    assert_eq!(
        store
            .try_start(&started(queued_on_repository(0x13, OperationKind::Check)))
            .await,
        Ok(Start::Refused { running: id(0x12) }),
        "and it holds the repository in its turn"
    );
}
