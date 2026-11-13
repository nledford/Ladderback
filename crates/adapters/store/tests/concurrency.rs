//! The store under concurrent use: writers and readers at once, and a write
//! lock held by somebody else. Every wait is bounded, so a store that hangs
//! fails its test instead of stalling the run.

mod support;

use std::assert_matches;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use app::ports::{HistoryQuery, OperationStore, StoreError};
use domain::execution::{FailureReason, Operation};
use sqlx::sqlite::SqliteConnection;
use support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_writers_and_readers_at_once_all_succeed() {
    const WRITERS: u128 = 12;
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    let done = Arc::new(AtomicBool::new(false));

    let readers: Vec<_> = (0..4)
        .map(|_| {
            let store = Arc::clone(&store);
            let done = Arc::clone(&done);
            tokio::spawn(async move {
                let mut reads = 0_u32;
                while !done.load(Ordering::Relaxed) {
                    let page = store.list(&HistoryQuery::default()).await?;
                    if let Some(newest) = page.operations.first() {
                        store.get(newest.id).await?;
                    }
                    reads += 1;
                }
                Ok::<u32, StoreError>(reads)
            })
        })
        .collect();
    // More writers than connections: the extra ones wait their turn.
    let writers: Vec<_> = (0..WRITERS)
        .map(|w| {
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                let mut op = queued_backup(0x1000 + w);
                store.save(&op, None).await?;
                op.start(at(10)).expect("a queued operation starts");
                for n in 0..30 {
                    op.record_warning(warning(&format!("/home/demo/writer-{w}-file-{n}")))
                        .expect("a running operation records warnings");
                }
                store.save(&op, None).await?;
                op.succeed(at(70)).expect("a running operation succeeds");
                store.save(&op, Some(&backup_result())).await?;
                Ok::<(), StoreError>(())
            })
        })
        .collect();

    for (w, writer) in writers.into_iter().enumerate() {
        let finished = within(120, "a writer", writer)
            .await
            .expect("the writer ends");
        assert_eq!(finished, Ok(()), "writer {w}");
    }
    done.store(true, Ordering::Relaxed);
    for reader in readers {
        let reads = within(60, "a reader", reader)
            .await
            .expect("the reader ends");
        assert!(reads.is_ok(), "a reader met an error: {reads:?}");
    }

    for w in 0..WRITERS {
        let found = store
            .get(id(0x1000 + w))
            .await
            .expect("reading works")
            .expect("the writer's operation is there");
        assert_eq!(found.record.warning_count, 30, "writer {w}");
        assert_eq!(found.warnings.len(), 30, "writer {w}");
        assert_eq!(
            found.warnings[29].item.as_deref(),
            Some(format!("/home/demo/writer-{w}-file-29").as_str()),
            "writer {w}"
        );
    }
    let page = store
        .list(&HistoryQuery::default())
        .await
        .expect("listing works");
    assert_eq!(page.operations.len(), WRITERS as usize);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_never_sees_half_of_a_save() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    // Two states of one operation that differ in everything a save writes.
    let few = with_warnings(started(queued_backup(0x2000)), 5);
    let mut many = with_warnings(started(queued_backup(0x2000)), 25);
    many.succeed(at(70)).expect("a running operation succeeds");
    let result = backup_result();
    let few_detail = detail_of(&few, None);
    let many_detail = detail_of(&many, Some(&result));
    save(&store, &few, None).await;

    let done = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let store = Arc::clone(&store);
            let done = Arc::clone(&done);
            let (few_detail, many_detail) = (few_detail.clone(), many_detail.clone());
            tokio::spawn(async move {
                let mut reads = 0_u32;
                while !done.load(Ordering::Relaxed) {
                    let found = store
                        .get(id(0x2000))
                        .await
                        .map_err(|error| format!("{error:?}"))?;
                    if found != Some(few_detail.clone()) && found != Some(many_detail.clone()) {
                        return Err(format!("a read saw neither state, but {found:?}"));
                    }
                    reads += 1;
                }
                Ok::<u32, String>(reads)
            })
        })
        .collect();
    let writer = {
        let store = Arc::clone(&store);
        let (few, many, result) = (few.clone(), many.clone(), result.clone());
        tokio::spawn(async move {
            for _ in 0..120 {
                store.save(&many, Some(&result)).await?;
                store.save(&few, None).await?;
            }
            Ok::<(), StoreError>(())
        })
    };

    let written = within(120, "the writer", writer)
        .await
        .expect("the writer ends");
    done.store(true, Ordering::Relaxed);
    assert_eq!(written, Ok(()));
    for reader in readers {
        let reads = within(60, "a reader", reader)
            .await
            .expect("the reader ends");
        assert!(reads.is_ok(), "{reads:?}");
    }
}

/// One of the writers' versions of the same operation: its own number of
/// warnings, and its own failure.
fn version_of(n: u128, writer: u32) -> Operation {
    let mut op = started(queued_backup(n));
    for item in 0..(5 + writer * 4) {
        op.record_warning(warning(&format!("/home/demo/writer-{writer}-file-{item}")))
            .expect("a running operation records warnings");
    }
    op.fail(
        FailureReason::EngineFailed {
            detail: format!("writer {writer} failed"),
        },
        at(70),
    )
    .expect("a running operation fails");
    op
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_saves_of_one_operation_leave_exactly_one_writers_state() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    let mut raw = scratch.raw().await;

    for round in 0..5_u128 {
        let n = 0x3000 + round;
        let versions: Vec<Operation> = (0..6).map(|writer| version_of(n, writer)).collect();
        let saves: Vec<_> = versions
            .iter()
            .cloned()
            .map(|op| {
                let store = Arc::clone(&store);
                tokio::spawn(async move { store.save(&op, None).await })
            })
            .collect();
        for saving in saves {
            let saved = within(60, "a save", saving).await.expect("the save ends");
            assert_eq!(saved, Ok(()), "round {round}");
        }

        let found = store
            .get(id(n))
            .await
            .expect("reading works")
            .expect("the operation is there");
        let winners = versions
            .iter()
            .filter(|op| found == detail_of(op, None))
            .count();
        assert_eq!(
            winners, 1,
            "round {round}: the state is exactly one writer's, not a mix: {found:?}"
        );
        assert_eq!(rows_of(&mut raw, "operations", "id", n).await, 1);
        assert_eq!(
            rows_of(&mut raw, "op_warnings", "op_id", n).await,
            i64::try_from(found.warnings.len()).expect("a count"),
            "round {round}: no warnings left over from the other writers"
        );
    }
}

/// An operation row written the way another program could, in a transaction
/// of our own that is not committed yet.
async fn begin_a_write_of_a_foreign_row(raw: &mut SqliteConnection, n: u128) {
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *raw)
        .await
        .expect("the write lock is taken");
    sqlx::query(
        r#"INSERT INTO operations (id, agent_id, kind, "trigger", status, queued_at)
           VALUES (?1, 'local', 'backup', 'manual', 'queued', 1800000000000)"#,
    )
    .bind(id_text(n))
    .execute(&mut *raw)
    .await
    .expect("the foreign row is written");
}

#[tokio::test]
async fn reads_are_not_blocked_by_a_write_in_progress_and_see_only_what_is_committed() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let committed = succeeded(queued_backup(0x4000));
    save(&store, &committed, None).await;
    let mut raw = scratch.raw().await;
    begin_a_write_of_a_foreign_row(&mut raw, 0x4001).await;

    let (page, one, uncommitted) = within(10, "reads beside a write in progress", async {
        let page = store.list(&HistoryQuery::default()).await;
        let one = store.get(id(0x4000)).await;
        let uncommitted = store.get(id(0x4001)).await;
        (page, one, uncommitted)
    })
    .await;

    let listed: Vec<_> = page
        .expect("listing works")
        .operations
        .iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(listed, [id(0x4000)], "the uncommitted row is not listed");
    assert_eq!(one, Ok(Some(detail_of(&committed, None))));
    assert_eq!(uncommitted, Ok(None), "nor can it be read");

    sqlx::query("COMMIT")
        .execute(&mut raw)
        .await
        .expect("the write is committed");
    let now = store.get(id(0x4001)).await.expect("reading works");
    assert!(now.is_some(), "once committed it is there");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_waits_for_a_lock_that_is_released_soon() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    let mut raw = scratch.raw().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut raw)
        .await
        .expect("the write lock is taken");

    let op = with_warnings(started(queued_backup(0x5000)), 2);
    let saving = {
        let store = Arc::clone(&store);
        let op = op.clone();
        tokio::spawn(async move { store.save(&op, None).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!saving.is_finished(), "the save waits for the lock");

    sqlx::query("COMMIT")
        .execute(&mut raw)
        .await
        .expect("the write lock is released");
    let saved = within(10, "the save", saving).await.expect("the save ends");
    assert_eq!(saved, Ok(()), "and goes through when it is released");
    let found = store.get(id(0x5000)).await.expect("reading works");
    assert_eq!(found, Some(detail_of(&op, None)));
}

#[tokio::test]
async fn a_write_that_never_gets_the_lock_gives_up_with_unavailable_and_the_store_recovers() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut raw)
        .await
        .expect("the write lock is taken");
    let op = succeeded(queued_backup(0x6000));

    let started_waiting = Instant::now();
    let outcome = within(
        20,
        "a write against a lock nobody releases",
        store.save(&op, None),
    )
    .await;

    assert_matches!(&outcome, Err(StoreError::Unavailable { detail }) if !detail.is_empty());
    assert!(started_waiting.elapsed() < Duration::from_secs(20));

    sqlx::query("ROLLBACK")
        .execute(&mut raw)
        .await
        .expect("the write lock is released");
    save(&store, &op, None).await;
    assert_eq!(
        store.get(id(0x6000)).await,
        Ok(Some(detail_of(&op, None))),
        "the store works again once the lock is gone"
    );
}
