//! The one-off copy of an operation log the migrations before the baseline
//! schema left (§16, decisions 165 and 170): the store takes a log at any
//! of the old migrations a release or a build shipped through the rest of
//! them and copies it into the baseline on open, keeping the old log beside
//! it. A start that stops partway leaves the old log usable, and the next
//! start copies it again. Removed at 1.0, with the copy.

mod support;

use std::assert_matches;
use std::fs;
use std::path::{Path, PathBuf};

use app::ports::{HistoryQuery, OperationStore};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection};
use store::{Dropped, OpenError, SqliteStore};
use support::*;

/// The fixture of the database the old migrations left at `migration`.
fn fixture(migration: i64) -> &'static str {
    match migration {
        16 => include_str!("fixtures/at-0016.sql"),
        28 => include_str!("fixtures/at-0028.sql"),
        29 => include_str!("fixtures/at-0029.sql"),
        other => panic!("no fixture at migration {other}"),
    }
}

/// A connection to `path` that makes it when missing, as an earlier store's
/// first open did.
async fn connect(path: &Path) -> SqliteConnection {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await
        .expect("a connection to the old database")
}

/// The database the fixture at `migration` describes, at `path`, changed by
/// `change`: in WAL mode as every earlier store kept it, with no connection
/// left open.
async fn database_at(path: &Path, migration: i64, change: &str) {
    let mut raw = connect(path).await;
    sqlx::raw_sql(fixture(migration))
        .execute(&mut raw)
        .await
        .expect("the old database is filled");
    sqlx::raw_sql(sqlx::AssertSqlSafe(change.to_owned()))
        .execute(&mut raw)
        .await
        .expect("the old database is changed");
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&mut raw)
        .await
        .expect("the old database is in WAL mode");
    raw.close().await.expect("the old database is closed");
}

/// `log` with `suffix` after its name.
fn beside(log: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", log.display()))
}

/// The files a copy works in, none of which outlives it.
const WORK_FILES: [&str; 4] = [
    ".baseline-new",
    ".baseline-source",
    ".before-baseline-new",
    ".baseline-lock",
];

fn assert_no_work_files_left(log: &Path) {
    for suffix in WORK_FILES {
        assert!(
            !beside(log, suffix).exists(),
            "{suffix} is left beside the log"
        );
    }
}

/// How many rows `table` holds.
async fn rows(raw: &mut SqliteConnection, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM \"{table}\""
    )))
    .fetch_one(raw)
    .await
    .expect("a count")
}

/// The last migration the database at `path` records.
async fn last_migration(path: &Path) -> i64 {
    let mut raw = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await
        .expect("the database opens");
    let last = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&mut raw)
        .await
        .expect("the database records its migrations");
    raw.close().await.expect("the connection closes");
    last
}

/// Every table of the baseline but `operations`, and the rows it holds once
/// a log the old migrations left at 28 or 29 is copied: the fixture's, less
/// the junk the copy drops.
const HELD_FROM_28_OR_29: [(&str, i64); 27] = [
    ("op_warnings", 2),
    ("schedule_state", 1),
    ("retry_state", 1),
    ("api_tokens", 1),
    ("sessions", 1),
    ("sign_in_settings", 1),
    ("repo_stats", 1),
    ("release_check", 1),
    ("proposals", 1),
    ("hub_tls_key", 1),
    ("sync_outbox_meta", 1),
    ("sync_outbox", 1),
    ("pairing_tokens", 3),
    ("machines", 2),
    ("machine_tombstones", 1),
    ("sync_watermarks", 2),
    ("sync_reports", 3),
    ("sync_report_tallies", 2),
    ("repository_owners", 2),
    ("ownership_reports", 1),
    ("sync_plans", 1),
    ("sync_plan_snapshots", 1),
    ("sync_progress", 1),
    ("machine_leases", 1),
    ("machine_epoch_floors", 1),
    ("machine_maintenance_paused", 1),
    ("hub_recovery_floors", 1),
];

/// The tables a log at 16, Ladderback 0.5.0's, holds rows of once copied;
/// the tables later migrations made are empty.
const HELD_FROM_16: [(&str, i64); 11] = [
    ("op_warnings", 2),
    ("schedule_state", 1),
    ("retry_state", 1),
    ("api_tokens", 1),
    ("sessions", 1),
    ("sign_in_settings", 1),
    ("repo_stats", 1),
    ("release_check", 1),
    ("proposals", 1),
    ("pairing_tokens", 3),
    ("machines", 2),
];

/// The junk the copy drops from the fixtures at 28 and 29 (decision 165):
/// the current state of the removed machine ...a3, and the report its new
/// ID ...a1 sent again (#1513).
fn junk_of_28_or_29() -> Vec<Dropped> {
    [
        "ownership_reports",
        "sync_plan_snapshots",
        "sync_plans",
        "sync_progress",
        "sync_reports",
    ]
    .into_iter()
    .map(|table| Dropped {
        table: table.to_owned(),
        rows: 1,
    })
    .collect()
}

/// A log at each old migration a release or a build left, 16 (0.5.0's), 28
/// and 29, opens copied into the baseline: every row kept but the junk the
/// baseline's rules prove, which the store names; the old log kept beside
/// it as it was; no work file left; and the next open finds nothing to copy.
#[tokio::test]
async fn a_log_the_old_migrations_left_at_16_28_or_29_is_copied_into_the_baseline_on_open() {
    for (migration, operations, held, dropped) in [
        (16, 4, HELD_FROM_16.as_slice(), Vec::new()),
        (28, 5, HELD_FROM_28_OR_29.as_slice(), junk_of_28_or_29()),
        (29, 4, HELD_FROM_28_OR_29.as_slice(), junk_of_28_or_29()),
    ] {
        let scratch = Scratch::new();
        let log = scratch.path();
        database_at(&log, migration, "").await;

        let store = scratch.open().await;
        let copy = store
            .baseline_copy()
            .unwrap_or_else(|| panic!("the log at {migration} is copied"));
        assert_eq!(
            copy.from_version, migration,
            "the copy names where it began"
        );
        assert_eq!(copy.kept, beside(&log, ".before-baseline"));
        let mut named = copy.dropped.clone();
        named.sort_by(|a, b| a.table.cmp(&b.table));
        assert_eq!(
            named, dropped,
            "the junk dropped from the log at {migration}"
        );

        let page = store
            .list(&HistoryQuery::default())
            .await
            .expect("the history reads");
        assert_eq!(
            page.operations.len(),
            operations,
            "every operation of the log at {migration} reads back"
        );
        let mut raw = scratch.raw().await;
        assert_eq!(rows(&mut raw, "operations").await, operations as i64);
        for (table, count) in held {
            assert_eq!(
                rows(&mut raw, table).await,
                *count,
                "{table} from {migration}"
            );
        }
        if migration > 16 {
            let tallied: i64 = sqlx::query_scalar(
                "SELECT held FROM sync_report_tallies \
                 WHERE machine_id = '00000000-0000-0000-0000-0000000000a1'",
            )
            .fetch_one(&mut raw)
            .await
            .expect("the tally");
            assert_eq!(tallied, 1, "the tally counts the reports still held");
        }
        raw.close().await.expect("the connection closes");
        store.close().await;

        assert_eq!(
            last_migration(&beside(&log, ".before-baseline")).await,
            migration,
            "the old log is kept beside the new one, at its own migration"
        );
        assert_no_work_files_left(&log);

        let again = scratch.open().await;
        assert!(
            again.baseline_copy().is_none(),
            "a log in the baseline is never copied again"
        );
        again.close().await;
    }
}

/// What migrations 0017 to 0029 did to the rows of an earlier log reaches
/// the copy: an API token 0.5.0 kept opens what its scopes opened, and a
/// hook's level an earlier build called `agent` is `machine`.
#[tokio::test]
async fn the_old_migrations_a_log_lacked_reach_its_rows_before_the_copy() {
    let scratch = Scratch::new();
    database_at(&scratch.path(), 16, "").await;
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let permissions: String =
        sqlx::query_scalar("SELECT permissions FROM api_tokens WHERE id = 'tok-1'")
            .fetch_one(&mut raw)
            .await
            .expect("the token is kept");
    assert_eq!(
        permissions,
        "backups:read,backup-runs:start-and-cancel,snapshot-contents:read,daemon-log:read"
    );
    raw.close().await.expect("the connection closes");
    store.close().await;

    let scratch = Scratch::new();
    database_at(&scratch.path(), 28, "").await;
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let args: String = sqlx::query_scalar(
        "SELECT error_args FROM operations WHERE id = '00000000-0000-0000-0000-000000000005'",
    )
    .fetch_one(&mut raw)
    .await
    .expect("the failed hook is kept");
    assert!(
        args.contains(r#"["level",{"type":"text","value":"machine"}]"#),
        "the hook's level is machine: {args}"
    );
    raw.close().await.expect("the connection closes");
    store.close().await;
}

/// An earlier daemon that crashed left rows in its write-ahead log, not yet
/// in the database file: the copy holds them too.
#[tokio::test]
async fn rows_a_crashed_earlier_daemon_left_in_its_write_ahead_log_are_copied() {
    let old = Scratch::new();
    database_at(&old.path(), 29, "").await;
    let mut raw = old.raw().await;
    sqlx::query("PRAGMA wal_autocheckpoint = 0")
        .execute(&mut raw)
        .await
        .expect("no checkpoint");
    sqlx::query(
        r#"INSERT INTO operations (id, machine_id, kind, "trigger", status, queued_at, warning_count)
           VALUES ('00000000-0000-0000-0000-000000000009', 'local', 'check', 'manual', 'queued', 1800000000000, 0)"#,
    )
    .execute(&mut raw)
    .await
    .expect("a row in the write-ahead log");
    // The files as a crash leaves them, while the connection still holds
    // the row in the write-ahead log alone.
    let crashed = Scratch::new();
    for suffix in ["", "-wal"] {
        fs::copy(beside(&old.path(), suffix), beside(&crashed.path(), suffix))
            .expect("the crashed files are copied");
    }
    raw.close().await.expect("the connection closes");

    let store = crashed.open().await;
    let page = store
        .list(&HistoryQuery::default())
        .await
        .expect("the history reads");
    assert_eq!(
        page.operations.len(),
        5,
        "the row in the write-ahead log is copied"
    );
    store.close().await;
}

/// A start that stopped partway, at any step, left its work files and the
/// old log, which it never changes: the next start copies the log again,
/// and a kept copy an earlier start left is replaced by the new one.
#[tokio::test]
async fn a_start_that_stopped_partway_leaves_the_old_log_and_the_next_start_copies_it() {
    let scratch = Scratch::new();
    let log = scratch.path();
    database_at(&log, 29, "").await;
    for suffix in [".baseline-new", ".baseline-source", ".before-baseline-new"] {
        fs::write(beside(&log, suffix), "half a database").expect("a work file a crash left");
    }
    fs::write(beside(&log, ".baseline-lock"), "").expect("the lock a crash left");
    fs::write(beside(&log, ".before-baseline"), "an earlier start's copy")
        .expect("a kept copy a crash left");

    let store = scratch.open().await;
    assert!(store.baseline_copy().is_some(), "the log is copied");
    let page = store
        .list(&HistoryQuery::default())
        .await
        .expect("the history reads");
    assert_eq!(page.operations.len(), 4, "every operation reads back");
    store.close().await;
    assert_eq!(
        last_migration(&beside(&log, ".before-baseline")).await,
        29,
        "the kept copy is the old log's"
    );
    assert_no_work_files_left(&log);
}

/// A log holding a row the baseline refuses is left as it was, with no copy
/// kept and no work file but the lock, and the store says why; once the row
/// is mended, the next start copies it.
#[tokio::test]
async fn a_log_the_baseline_refuses_a_row_of_is_left_as_it_was_and_the_next_start_tries_again() {
    let scratch = Scratch::new();
    let log = scratch.path();
    database_at(
        &log,
        29,
        "UPDATE operations SET status = 'done' WHERE id = '00000000-0000-0000-0000-000000000003'",
    )
    .await;
    let before = fs::read(&log).expect("the log is readable");

    let error = open_error(&log).await;

    assert_matches!(error, OpenError::BaselineCopy { version: 29, .. });
    assert_eq!(
        fs::read(&log).expect("the log is readable"),
        before,
        "the log is left byte for byte as it was"
    );
    assert!(
        !beside(&log, ".before-baseline").exists(),
        "no copy is kept"
    );
    for suffix in [".baseline-new", ".baseline-source", ".before-baseline-new"] {
        assert!(!beside(&log, suffix).exists(), "{suffix} is left");
    }

    let mut raw = scratch.raw().await;
    sqlx::query(
        "UPDATE operations SET status = 'queued' WHERE id = '00000000-0000-0000-0000-000000000003'",
    )
    .execute(&mut raw)
    .await
    .expect("the row is mended");
    raw.close().await.expect("the connection closes");
    let store = scratch.open().await;
    assert!(store.baseline_copy().is_some(), "the next start copies it");
    store.close().await;
}

/// An old migration that fails on the log, here one whose table is already
/// there, stops the copy as a row the baseline refuses does.
#[tokio::test]
async fn an_old_migration_that_fails_on_the_log_leaves_it_as_it_was() {
    let scratch = Scratch::new();
    let log = scratch.path();
    database_at(
        &log,
        16,
        "CREATE TABLE hub_tls_key (id INTEGER PRIMARY KEY)",
    )
    .await;
    let before = fs::read(&log).expect("the log is readable");

    let error = open_error(&log).await;

    assert_matches!(error, OpenError::BaselineCopy { version: 16, .. });
    assert_eq!(fs::read(&log).expect("the log is readable"), before);
    assert!(
        !beside(&log, ".before-baseline").exists(),
        "no copy is kept"
    );
}

/// Two starts at once on the same old log, as a `pair` beside the daemon's
/// start: one copies it, the other waits and opens the copy.
#[tokio::test]
async fn two_starts_at_once_copy_the_log_once_and_both_open_it() {
    let scratch = Scratch::new();
    let log = scratch.path();
    database_at(&log, 29, "").await;

    let (first, second) = within(60, "both opens", async {
        tokio::join!(SqliteStore::open(&log), SqliteStore::open(&log))
    })
    .await;
    let first = first.expect("the first open");
    let second = second.expect("the second open");

    let copies = [&first, &second]
        .iter()
        .filter(|store| store.baseline_copy().is_some())
        .count();
    assert_eq!(copies, 1, "the log is copied once");
    for store in [&first, &second] {
        let page = store
            .list(&HistoryQuery::default())
            .await
            .expect("the history reads");
        assert_eq!(page.operations.len(), 4, "every operation reads back");
    }
    first.close().await;
    second.close().await;
}

/// The new log and the kept one are no more open than the old log was: it
/// holds the sessions and API tokens' fingerprints.
#[cfg(unix)]
#[tokio::test]
async fn the_new_log_and_the_kept_one_keep_the_old_logs_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    let log = scratch.path();
    database_at(&log, 29, "").await;
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).expect("the log is private");

    scratch.open().await.close().await;

    for path in [log.clone(), beside(&log, ".before-baseline")] {
        let mode = fs::metadata(&path)
            .expect("the file is there")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "{} is its owner's alone",
            path.display()
        );
    }
}
