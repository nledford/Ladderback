//! Maintenance serialization (I7) in SQLite: what the database itself
//! refuses, starts racing through separate handles on one file, and what a
//! start leaves in it. What every operation log must do is the port's
//! contract suite, which `contract.rs` runs.

mod support;

use std::sync::Arc;
use std::time::Duration;

use app::ports::{OperationStore, Start, StoreError};
use domain::execution::{Operation, OperationKind};
use sqlx::sqlite::SqliteConnection;
use support::*;
use tokio::sync::Barrier;

/// Another repository's ID, to tell it from the demo repository's.
const OTHER_REPOSITORY_HEX: &str =
    "1212121212121212121212121212121212121212121212121212121212121212";

/// Writes a row as another program could: straight into the table, under the
/// ID text `id`.
async fn insert_text(
    raw: &mut SqliteConnection,
    id: &str,
    kind: &str,
    status: &str,
    repository: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO operations (id, agent_id, kind, "trigger", status, repo_uuid, queued_at)
           VALUES (?1, 'local', ?2, 'manual', ?3, ?4, 1800000000000)"#,
    )
    .bind(id)
    .bind(kind)
    .bind(status)
    .bind(repository)
    .execute(&mut *raw)
    .await
    .map(|_| ())
}

async fn insert(
    raw: &mut SqliteConnection,
    n: u128,
    kind: &str,
    status: &str,
    repository: Option<&str>,
) -> Result<(), sqlx::Error> {
    insert_text(raw, &id_text(n), kind, status, repository).await
}

/// Whether the database turned a write down as a second running maintenance
/// operation on a repository: the unique index says so, naming its column.
fn refused_as_a_second<T>(outcome: &Result<T, sqlx::Error>) -> bool {
    matches!(
        outcome,
        Err(sqlx::Error::Database(error))
            if error.is_unique_violation() && error.message().contains("operations.repo_uuid")
    )
}

/// How many rows of `operations` `sql` counts, for the demo repository.
async fn rows_on_the_repository(raw: &mut SqliteConnection, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(REPOSITORY_HEX)
        .fetch_one(&mut *raw)
        .await
        .expect("a count")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_racing_through_two_handles_let_exactly_one_win_every_round() {
    const ROUNDS: u128 = 20;
    const RACERS: u128 = 8;
    let scratch = Scratch::new();
    let handles = [
        Arc::new(scratch.open().await),
        Arc::new(scratch.open().await),
    ];
    let mut raw = scratch.raw().await;

    for round in 0..ROUNDS {
        let racers: Vec<Operation> = (0..RACERS)
            .map(|racer| {
                let kind = if racer % 2 == 0 {
                    OperationKind::Prune
                } else {
                    OperationKind::Check
                };
                started(queued_on_repository(0x7000 + round * 0x10 + racer, kind))
            })
            .collect();
        let barrier = Arc::new(Barrier::new(racers.len()));
        let starts: Vec<_> = racers
            .iter()
            .enumerate()
            .map(|(racer, op)| {
                let store = Arc::clone(&handles[racer % 2]);
                let barrier = Arc::clone(&barrier);
                let op = op.clone();
                tokio::spawn(async move {
                    barrier.wait().await;
                    store.try_start(&op).await
                })
            })
            .collect();
        let mut decided = Vec::new();
        for start in starts {
            decided.push(
                within(60, "a racing start", start)
                    .await
                    .expect("the start's task ends"),
            );
        }

        let winners: Vec<&Operation> = racers
            .iter()
            .zip(&decided)
            .filter(|(_, decision)| **decision == Ok(Start::Started))
            .map(|(op, _)| op)
            .collect();
        assert_eq!(
            winners.len(),
            1,
            "round {round}: exactly one start wins: {decided:?}"
        );
        let winner = winners[0];
        for (op, decision) in racers.iter().zip(&decided) {
            if op.id() != winner.id() {
                assert_eq!(
                    decision,
                    &Ok(Start::Refused {
                        running: winner.id()
                    }),
                    "round {round}: a loser names the winner"
                );
            }
        }
        assert_eq!(
            rows_on_the_repository(
                &mut raw,
                "SELECT COUNT(*) FROM operations WHERE repo_uuid = ?1 AND status = 'running'"
            )
            .await,
            1,
            "round {round}: the file holds one running operation"
        );
        assert_eq!(
            rows_on_the_repository(
                &mut raw,
                "SELECT COUNT(*) FROM operations WHERE repo_uuid = ?1"
            )
            .await,
            i64::try_from(round).expect("a small number") + 1,
            "round {round}: the losers stored nothing"
        );

        // The winner ends, through either handle, and frees the repository
        // for the next round.
        let mut ended = winner.clone();
        ended.succeed(at(70)).expect("a running operation succeeds");
        let ending = &handles[usize::try_from(round).expect("a small number") % 2];
        save(ending, &ended, None).await;
    }
    for handle in handles {
        handle.close().await;
    }
}

#[tokio::test]
async fn the_database_itself_refuses_a_second_running_maintenance_operation() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    // One running maintenance operation per repository, whichever kind it is.
    let first = insert(&mut raw, 1, "forget", "running", Some(REPOSITORY_HEX)).await;
    assert!(first.is_ok(), "the first one is written: {first:?}");
    for (n, kind) in [(2, "forget"), (3, "prune"), (4, "check")] {
        let second = insert(&mut raw, n, kind, "running", Some(REPOSITORY_HEX)).await;
        assert!(
            refused_as_a_second(&second),
            "a second running {kind} is refused: {second:?}"
        );
    }

    // Nothing else counts: other kinds, operations that aren't running,
    // another repository, and no repository at all.
    let allowed: [(&str, &str, Option<&str>); 14] = [
        ("backup", "running", Some(REPOSITORY_HEX)),
        ("backup", "running", Some(REPOSITORY_HEX)),
        ("identify", "running", Some(REPOSITORY_HEX)),
        ("fake", "running", Some(REPOSITORY_HEX)),
        ("prune", "queued", Some(REPOSITORY_HEX)),
        ("prune", "queued", Some(REPOSITORY_HEX)),
        ("check", "succeeded", Some(REPOSITORY_HEX)),
        ("check", "succeeded_with_warnings", Some(REPOSITORY_HEX)),
        ("prune", "failed", Some(REPOSITORY_HEX)),
        ("forget", "cancelled", Some(REPOSITORY_HEX)),
        ("prune", "running", Some(OTHER_REPOSITORY_HEX)),
        ("prune", "running", None),
        ("check", "running", None),
        ("forget", "running", None),
    ];
    for (n, (kind, status, repository)) in (100..).zip(allowed) {
        let outcome = insert(&mut raw, n, kind, status, repository).await;
        assert!(
            outcome.is_ok(),
            "a {status} {kind} on {repository:?} is written: {outcome:?}"
        );
    }

    // A queued operation can't become the second running one either, until
    // the first one has ended.
    let wake = "UPDATE operations SET status = 'running' WHERE id = ?1";
    let queued_prune = id_text(104);
    let waking = sqlx::query(wake)
        .bind(&queued_prune)
        .execute(&mut raw)
        .await;
    assert!(
        refused_as_a_second(&waking),
        "a queued prune can't start while the repository is held: {waking:?}"
    );
    sqlx::query("UPDATE operations SET status = 'succeeded' WHERE id = ?1")
        .bind(id_text(1))
        .execute(&mut raw)
        .await
        .expect("the first one ends");
    let waking = sqlx::query(wake)
        .bind(&queued_prune)
        .execute(&mut raw)
        .await;
    assert!(waking.is_ok(), "and can once it has: {waking:?}");
    store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_waits_for_a_write_in_progress_and_then_sees_what_it_committed() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    let mut raw = scratch.raw().await;
    // Another program takes the write lock and records a prune on the
    // repository, without committing yet.
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut raw)
        .await
        .expect("the write lock is taken");
    insert(&mut raw, 0x7100, "prune", "running", Some(REPOSITORY_HEX))
        .await
        .expect("the foreign prune is written");

    let check = started(queued_on_repository(0x7101, OperationKind::Check));
    let starting = {
        let store = Arc::clone(&store);
        let check = check.clone();
        tokio::spawn(async move { store.try_start(&check).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!starting.is_finished(), "the start waits for the lock");

    sqlx::query("COMMIT")
        .execute(&mut raw)
        .await
        .expect("the write lock is released");
    let decided = within(10, "the start", starting)
        .await
        .expect("the start ends");
    assert_eq!(
        decided,
        Ok(Start::Refused {
            running: id(0x7100)
        }),
        "it decides on what was committed while it waited"
    );
    assert_eq!(store.get(id(0x7101)).await, Ok(None));
}

/// Makes storing a warning for `/home/demo/boom` fail, as a full disk would
/// at the same point: after the operation's row is written.
const REFUSE_BOOM: &str = "CREATE TRIGGER refuse_boom BEFORE INSERT ON op_warnings
    WHEN NEW.item = '/home/demo/boom'
    BEGIN SELECT RAISE(ABORT, 'boom'); END";

#[tokio::test]
async fn a_start_that_fails_halfway_stores_nothing_and_holds_nothing() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    sqlx::query(REFUSE_BOOM)
        .execute(&mut raw)
        .await
        .expect("the fault is set up");
    let mut prune = with_warnings(
        started(queued_on_repository(0x7200, OperationKind::Prune)),
        1,
    );
    prune
        .record_warning(warning("/home/demo/boom"))
        .expect("a running operation records warnings");

    let outcome = store.try_start(&prune).await;

    assert!(
        matches!(outcome, Err(StoreError::Unavailable { .. })),
        "{outcome:?}"
    );
    assert_eq!(rows_of(&mut raw, "operations", "id", 0x7200).await, 0);
    assert_eq!(rows_of(&mut raw, "op_warnings", "op_id", 0x7200).await, 0);

    // So the repository is free for the next one.
    let check = started(queued_on_repository(0x7201, OperationKind::Check));
    assert_eq!(store.try_start(&check).await, Ok(Start::Started));
}

#[tokio::test]
async fn a_refused_start_leaves_every_row_as_it_was() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let holder = with_warnings(
        started(queued_on_repository(0x7300, OperationKind::Prune)),
        2,
    );
    assert_eq!(store.try_start(&holder).await, Ok(Start::Started));
    let waiting = queued_on_repository(0x7301, OperationKind::Check);
    save(&store, &waiting, None).await;
    let holder_row = stored_row(&mut raw, 0x7300).await;
    let holder_warnings = stored_warnings(&mut raw, 0x7300).await;
    let waiting_row = stored_row(&mut raw, 0x7301).await;
    let operations = count(&mut raw, "SELECT COUNT(*) FROM operations").await;
    let warnings = count(&mut raw, "SELECT COUNT(*) FROM op_warnings").await;

    // The queued check tries to start, and so does a newcomer, each with
    // warnings of its own.
    let loser = with_warnings(started(waiting.clone()), 3);
    let newcomer = with_warnings(
        started(queued_on_repository(0x7302, OperationKind::Prune)),
        1,
    );
    for op in [&loser, &newcomer] {
        assert_eq!(
            store.try_start(op).await,
            Ok(Start::Refused {
                running: id(0x7300)
            })
        );
    }

    assert_eq!(stored_row(&mut raw, 0x7300).await, holder_row);
    assert_eq!(stored_warnings(&mut raw, 0x7300).await, holder_warnings);
    assert_eq!(
        stored_row(&mut raw, 0x7301).await,
        waiting_row,
        "the queued check is still queued"
    );
    assert_eq!(rows_of(&mut raw, "operations", "id", 0x7302).await, 0);
    assert_eq!(
        count(&mut raw, "SELECT COUNT(*) FROM operations").await,
        operations
    );
    assert_eq!(
        count(&mut raw, "SELECT COUNT(*) FROM op_warnings").await,
        warnings,
        "none of the refused warnings were written"
    );
}

#[tokio::test]
async fn a_holder_that_cannot_be_read_fails_the_start_instead_of_letting_it_through() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    insert_text(
        &mut raw,
        "not-an-operation-id",
        "prune",
        "running",
        Some(REPOSITORY_HEX),
    )
    .await
    .expect("the foreign row is written");

    let outcome = store
        .try_start(&started(queued_on_repository(0x7400, OperationKind::Prune)))
        .await;

    assert!(
        matches!(&outcome, Err(StoreError::Corrupt { detail }) if detail.contains("not-an-operation-id")),
        "{outcome:?}"
    );
    assert_eq!(rows_of(&mut raw, "operations", "id", 0x7400).await, 0);
}
