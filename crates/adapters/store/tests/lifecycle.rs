//! Opening and closing the store: what it refuses, what it leaves alone, and
//! what a closed store answers.

mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use app::ports::{HistoryQuery, OperationRecord, OperationStore, StoreError};
use sqlx::Connection;
use store::OpenError;
use support::*;

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
        ["steading.db"],
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

    let error = open_error(&missing.join("steading.db")).await;

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

    let error = open_error(&locked.join("steading.db")).await;

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
    let path = dir.join("steading.db");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_waits_for_a_write_in_flight() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    // Another connection holds the write lock, so the save below has its
    // connection and is waiting for the lock: a write in flight.
    let mut raw = scratch.raw().await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut raw)
        .await
        .expect("the write lock is taken");

    let op = with_warnings(started(queued_backup(0x10)), 3);
    let saving = {
        let store = Arc::clone(&store);
        let op = op.clone();
        tokio::spawn(async move { store.save(&op, None).await })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut closing = Box::pin(store.close());
    let early = tokio::time::timeout(Duration::from_millis(300), &mut closing).await;
    assert!(
        early.is_err(),
        "close returned with a write still in flight"
    );

    sqlx::query("COMMIT")
        .execute(&mut raw)
        .await
        .expect("the write lock is released");
    within(30, "closing", closing).await;
    let saved = within(30, "the save", saving).await.expect("the task ends");
    assert_eq!(saved, Ok(()), "the write in flight completed");

    let reopened = scratch.open().await;
    let found = reopened.get(id(0x10)).await.expect("reading works");
    assert_eq!(
        found.map(|detail| detail.warnings),
        Some(op.warnings().to_vec()),
        "and was kept, warnings and all"
    );
}
