//! Opening and closing the store: what it refuses, what it leaves alone, and
//! what a closed store answers.

mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use app::ports::{HistoryQuery, OperationRecord, OperationStore, Start, StoreError};
use domain::execution::{FailureReason, OperationKind};
use sqlx::Connection;
use store::{OpenError, SqliteStore};
use support::*;
use tokio::task::JoinHandle;

/// The names in `dir`, sorted.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("the directory is readable")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_new_database_is_created_and_uses_write_ahead_logging() {
    let scratch = Scratch::new();
    assert!(!scratch.path().exists());
    let store = scratch.open().await;
    assert!(scratch.path().exists(), "opening creates the file");

    // The mode is kept in the file, so a connection of our own sees it.
    let mut raw = scratch.raw().await;
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut raw)
        .await
        .expect("the journal mode is readable");
    assert_eq!(mode, "wal");
    store.close().await;
}

/// A database this build wrote, with an operation in it, that another program
/// then damaged with `damage`; `open` must refuse it and change nothing.
async fn refusal_of_a_damaged_database(damage: &str) -> OpenError {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    save(
        &store,
        &with_warnings(started(queued_backup(0x10)), 2),
        None,
    )
    .await;
    store.close().await;
    let mut raw = scratch.raw().await;
    sqlx::query(damage)
        .execute(&mut raw)
        .await
        .expect("the damage is done");
    raw.close().await.expect("the connection closes");
    let before = fs::read(scratch.path()).expect("the database file is readable");

    let error = open_error(&scratch.path()).await;

    assert_eq!(
        fs::read(scratch.path()).expect("the database file is readable"),
        before,
        "a database that is refused is left byte for byte as it was"
    );
    assert_eq!(
        entries(scratch.dir()),
        ["ladderback.db"],
        "and a refused open leaves no connection to it behind"
    );
    error
}

#[tokio::test]
async fn a_database_from_a_newer_version_is_refused_and_left_untouched() {
    let error = refusal_of_a_damaged_database(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
         VALUES (9999, 'from the future', 1, X'00', 0)",
    )
    .await;
    assert!(
        matches!(error, OpenError::NewerSchema { version: 9999 }),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_migration_edited_since_it_was_applied_is_refused() {
    let error = refusal_of_a_damaged_database(
        "UPDATE _sqlx_migrations SET checksum = X'0102' WHERE version = 1",
    )
    .await;
    assert!(
        matches!(&error, OpenError::Migration { detail } if !detail.is_empty()),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_migration_left_half_applied_is_refused() {
    let error =
        refusal_of_a_damaged_database("UPDATE _sqlx_migrations SET success = 0 WHERE version = 1")
            .await;
    assert!(
        matches!(&error, OpenError::Migration { detail } if !detail.is_empty()),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_file_that_is_not_a_database_is_refused_and_left_as_it_was() {
    let scratch = Scratch::new();
    let notes = "Shopping list, not a database.\n".repeat(400);
    fs::write(scratch.path(), &notes).expect("the file is written");

    let error = open_error(&scratch.path()).await;

    assert!(
        matches!(&error, OpenError::Database { detail } if !detail.is_empty()),
        "{error:?}"
    );
    assert_eq!(
        fs::read_to_string(scratch.path()).expect("the file is still readable"),
        notes
    );
}

#[tokio::test]
async fn a_directory_that_does_not_exist_is_refused_and_not_created() {
    let scratch = Scratch::new();
    let missing = scratch.dir().join("missing");

    let error = open_error(&missing.join("ladderback.db")).await;

    assert!(
        matches!(&error, OpenError::Database { detail } if !detail.is_empty()),
        "{error:?}"
    );
    assert!(
        !missing.exists(),
        "the data directory is not the store's to create"
    );
}

#[tokio::test]
async fn a_directory_in_place_of_the_file_is_refused() {
    let scratch = Scratch::new();
    let error = open_error(scratch.dir()).await;
    assert!(matches!(error, OpenError::Database { .. }), "{error:?}");
}

/// Puts a directory's permissions back, even when the test fails.
#[cfg(unix)]
struct Restore(std::path::PathBuf);

#[cfg(unix)]
impl Drop for Restore {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_directory_that_cannot_be_written_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let locked = scratch.dir().join("locked");
    fs::create_dir(&locked).expect("the directory is created");
    let _restore = Restore(locked.clone());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555))
        .expect("the directory is locked");
    if fs::File::create(locked.join("probe")).is_ok() {
        testkit::skip!("this user can write to a directory without write permission");
    }

    let error = open_error(&locked.join("ladderback.db")).await;

    assert!(
        matches!(&error, OpenError::Database { detail } if !detail.is_empty()),
        "{error:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_path_with_spaces_and_odd_characters_works() {
    let scratch = Scratch::new();
    // Like `~/Library/Application Support`, and worse: nothing in a path is
    // special to the store, whatever it would mean in a URL.
    let dir = scratch.dir().join("demo data #1 ?x=y%20 ☃");
    fs::create_dir(&dir).expect("the directory is created");
    let path = dir.join("ladderback.db");
    let op = queued_backup(0x10);

    let store = store::SqliteStore::open(&path)
        .await
        .expect("the database opens");
    save(&store, &op, None).await;
    store.close().await;

    let reopened = store::SqliteStore::open(&path)
        .await
        .expect("the database opens again");
    let found = reopened.get(id(0x10)).await.expect("reading works");
    assert_eq!(
        found.map(|detail| detail.record),
        Some(OperationRecord::of(&op, None))
    );
    assert!(path.exists(), "the database is where the path says");
}

#[tokio::test]
async fn a_closed_store_answers_every_call_with_unavailable() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let op = queued_backup(0x10);
    save(&store, &op, None).await;
    store.close().await;

    let saved = store.save(&op, None).await;
    assert!(
        matches!(saved, Err(StoreError::Unavailable { .. })),
        "{saved:?}"
    );
    let prune = started(queued_on_repository(0x11, OperationKind::Prune));
    let begun = store.try_start(&prune).await;
    assert!(
        matches!(begun, Err(StoreError::Unavailable { .. })),
        "{begun:?}"
    );
    let found = store.get(id(0x10)).await;
    assert!(
        matches!(found, Err(StoreError::Unavailable { .. })),
        "{found:?}"
    );
    let listed = store.list(&HistoryQuery::default()).await;
    assert!(
        matches!(listed, Err(StoreError::Unavailable { .. })),
        "{listed:?}"
    );
    // Closing again is harmless.
    within(30, "closing twice", store.close()).await;

    // What was saved before is there for the next one.
    let reopened = scratch.open().await;
    let kept = reopened.get(id(0x10)).await.expect("reading works");
    assert_eq!(
        kept.map(|detail| detail.record),
        Some(OperationRecord::of(&op, None))
    );
}

/// A write the store can have in flight when it is closed, of operation 0x10.
#[derive(Clone, Copy, Debug)]
enum Write {
    Save,
    Start,
    FailUnfinished,
}

impl Write {
    /// What the store holds before the write: `fail_unfinished` needs a
    /// running operation to end.
    async fn prepare(self, store: &SqliteStore) {
        if let Write::FailUnfinished = self {
            save(store, &with_warnings(started(queued_backup(0x10)), 3), None).await;
        }
    }

    /// Starts the write in a task of its own; the task fails on an answer
    /// other than the one the write gives when it succeeds.
    fn spawn(self, store: &Arc<SqliteStore>) -> JoinHandle<Result<(), StoreError>> {
        let store = Arc::clone(store);
        tokio::spawn(async move {
            match self {
                Write::Save => {
                    let op = with_warnings(started(queued_backup(0x10)), 3);
                    store.save(&op, None).await
                }
                Write::Start => {
                    let op = started(queued_on_repository(0x10, OperationKind::Prune));
                    let start = store.try_start(&op).await?;
                    assert_eq!(start, Start::Started);
                    Ok(())
                }
                Write::FailUnfinished => {
                    let ended = store.fail_unfinished(&FailureReason::DaemonStopped).await?;
                    assert_eq!(ended, 1);
                    Ok(())
                }
            }
        })
    }

    /// The status operation 0x10 has, and the warnings it keeps, once the
    /// write has ended.
    fn outcome(self) -> (&'static str, usize) {
        match self {
            Write::Save => ("running", 3),
            Write::Start => ("running", 0),
            Write::FailUnfinished => ("failed", 3),
        }
    }
}

/// Holds the write lock from another connection, so `write` started now has
/// its connection and waits for the lock: a write in flight. `close` must not
/// return before that write has ended, and the write must be kept. Reads
/// made while `close` waits wait behind it and find the store closed.
async fn closing_waits_for(write: Write, scratch: &Scratch, store: Arc<SqliteStore>) {
    write.prepare(&store).await;
    let mut raw = scratch.raw().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut raw)
        .await
        .expect("the write lock is taken");

    let writing = write.spawn(&store);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut closing = Box::pin(store.close());
    let early = tokio::time::timeout(Duration::from_millis(300), &mut closing).await;
    assert!(
        early.is_err(),
        "close returned with a write still in flight: {write:?}"
    );
    let reading = {
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            let everything = HistoryQuery::default();
            tokio::join!(store.get(id(0x10)), store.list(&everything))
        })
    };

    sqlx::query("COMMIT")
        .execute(&mut raw)
        .await
        .expect("the write lock is released");
    within(30, "closing", closing).await;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM operations WHERE id = ?1")
        .bind(id_text(0x10))
        .fetch_optional(&mut raw)
        .await
        .expect("the status reads");
    let warnings = stored_warnings(&mut raw, 0x10).await.len();

    let written = within(30, "the write", writing)
        .await
        .expect("the task ends");
    assert_eq!(written, Ok(()), "the write in flight completed: {write:?}");
    let (expected_status, expected_warnings) = write.outcome();
    assert_eq!(
        (status.as_deref(), warnings),
        (Some(expected_status), expected_warnings),
        "close returned before the write it waited for had ended: {write:?}"
    );
    let (found, listed) = within(30, "the reads", reading)
        .await
        .expect("the task ends");
    assert!(
        matches!(found, Err(StoreError::Unavailable { .. })),
        "a read made while close waited: {found:?}"
    );
    assert!(
        matches!(listed, Err(StoreError::Unavailable { .. })),
        "a list made while close waited: {listed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_waits_for_a_write_in_flight() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    closing_waits_for(Write::Save, &scratch, store).await;
}

/// sqlx 0.8's `Pool::close` gives the pool a permit back for every idle
/// connection it closes, permits those connections had already returned, so
/// with two idle connections it stops waiting for the one a write still has
/// (#168).
async fn closing_waits_beside_idle_connections_for(write: Write) {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    // Two reads at once, so the pool opens a second connection; both are idle
    // again by the time the write is in flight.
    let (first, second) = tokio::join!(store.get(id(0x01)), store.get(id(0x02)));
    assert!(matches!(first, Ok(None)), "{first:?}");
    assert!(matches!(second, Ok(None)), "{second:?}");
    closing_waits_for(write, &scratch, store).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_waits_for_a_save_in_flight_beside_idle_connections() {
    closing_waits_beside_idle_connections_for(Write::Save).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_waits_for_a_start_in_flight_beside_idle_connections() {
    closing_waits_beside_idle_connections_for(Write::Start).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_waits_for_failing_the_unfinished_beside_idle_connections() {
    closing_waits_beside_idle_connections_for(Write::FailUnfinished).await;
}
