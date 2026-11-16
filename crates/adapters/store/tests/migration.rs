//! Upgrading a database an earlier build wrote. The migration that adds the
//! one-maintenance-operation-per-repository rule must cope with a log that
//! already breaks it, because nothing stopped an earlier build from recording
//! two running maintenance operations on one repository.

mod support;

use std::assert_matches;
use std::fs;
use std::path::Path;

use app::ports::{
    BackupSummary, EngineResult, OperationDetail, OperationRecord, OperationStore, Start,
};
use domain::RepositoryId;
use domain::execution::{FailureReason, OperationKind, OperationStatus, Target, Trigger};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection, Row};
use store::OpenError;
use support::*;

/// Other repositories' IDs, to tell them from the demo repository's.
const LAPTOP_HEX: &str = "1212121212121212121212121212121212121212121212121212121212121212";
const SERVER_HEX: &str = "3434343434343434343434343434343434343434343434343434343434343434";

/// The database file as the first build left it: the first migration applied,
/// and no other.
async fn database_at_version_1(scratch: &Scratch) -> SqliteConnection {
    database_at_version(scratch, 1).await
}

/// The database file as a build that knew the first `version` migrations
/// left it: those applied, and no later one.
async fn database_at_version(scratch: &Scratch, version: usize) -> SqliteConnection {
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../migrations");
    let mut files: Vec<_> = fs::read_dir(&migrations)
        .expect("the migrations are listed")
        .map(|entry| entry.expect("a migration's entry").file_name())
        .collect();
    files.sort();
    let only_the_first = scratch.dir().join("first-migrations-only");
    fs::create_dir(&only_the_first).expect("a directory for the first migrations");
    for file in files.iter().take(version) {
        fs::copy(migrations.join(file), only_the_first.join(file)).expect("a migration is copied");
    }
    let first = Migrator::new(only_the_first.as_path())
        .await
        .expect("the first migrations are read");
    let mut old = SqliteConnectOptions::new()
        .filename(scratch.path())
        .create_if_missing(true)
        .connect()
        .await
        .expect("the old database is created");
    first
        .run(&mut old)
        .await
        .expect("the first migrations are applied");
    old
}

/// A row as the first build wrote it, for the operation `n`.
async fn legacy(
    old: &mut SqliteConnection,
    n: u128,
    kind: &str,
    status: &str,
    repository: Option<&str>,
) {
    let (started_at, finished_at) = match status {
        "queued" => (None, None),
        "running" => (Some(ms(10)), None),
        _ => (Some(ms(10)), Some(ms(70))),
    };
    let failure =
        (status == "failed").then_some(("operation-failed", "[]", "repository not found"));
    sqlx::query(
        r#"INSERT INTO operations (id, agent_id, kind, repo_uuid, "trigger", status, queued_at,
                                   started_at, finished_at, error_code, error_args, error_detail)
           VALUES (?1, 'local', ?2, ?3, 'manual', ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
    )
    .bind(id_text(n))
    .bind(kind)
    .bind(repository)
    .bind(status)
    .bind(ms(0))
    .bind(started_at)
    .bind(finished_at)
    .bind(failure.map(|(code, _, _)| code))
    .bind(failure.map(|(_, args, _)| args))
    .bind(failure.map(|(_, _, detail)| detail))
    .execute(&mut *old)
    .await
    .expect("the old row is written");
}

/// The raw rows of the operations `numbers`.
async fn rows_of_operations(conn: &mut SqliteConnection, numbers: &[u128]) -> Vec<Stored> {
    let mut rows = Vec::new();
    for n in numbers {
        rows.push(stored_row(conn, *n).await);
    }
    rows
}

/// The versions of the migrations the database records as applied.
async fn applied_versions(conn: &mut SqliteConnection) -> Vec<(i64, bool)> {
    sqlx::query("SELECT version, success FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&mut *conn)
        .await
        .expect("the migration table is readable")
        .iter()
        .map(|row| (row.get("version"), row.get("success")))
        .collect()
}

/// What a demoted prune or check of the demo repository reads back as: failed
/// without an exit status, started and never finished.
fn demoted(n: u128, kind: OperationKind) -> OperationRecord {
    OperationRecord {
        id: id(n),
        kind,
        trigger: Trigger::Manual,
        target: Target {
            plan: None,
            repository: Some(repository()),
        },
        status: OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: None },
        },
        queued_at: at(0),
        started_at: Some(at(10)),
        finished_at: None,
        warning_count: 0,
        warning_kinds: Vec::new(),
        result: None,
        repository_alias: None,
        locations: None,
    }
}

#[tokio::test]
async fn an_old_database_gains_the_rule_and_keeps_what_it_holds() {
    let scratch = Scratch::new();
    let mut old = database_at_version_1(&scratch).await;
    // The demo repository ran three maintenance operations at once, written
    // here newest first: the newest is the one that stays running.
    legacy(&mut old, 0x12, "prune", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x10, "prune", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x11, "check", "running", Some(REPOSITORY_HEX)).await;
    // Beside them, operations that never counted.
    legacy(&mut old, 0x13, "backup", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x14, "identify", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x15, "fake", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x16, "prune", "queued", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x17, "check", "succeeded", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x18, "prune", "failed", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x19, "check", "cancelled", Some(REPOSITORY_HEX)).await;
    // Another repository with one running maintenance operation, and
    // operations that name no repository, which are never held to the rule.
    legacy(&mut old, 0x20, "prune", "running", Some(LAPTOP_HEX)).await;
    legacy(&mut old, 0x30, "prune", "running", None).await;
    legacy(&mut old, 0x31, "check", "running", None).await;
    // A third, whose older operation is a forget, which only the table knows.
    legacy(&mut old, 0x40, "forget", "running", Some(SERVER_HEX)).await;
    legacy(&mut old, 0x41, "prune", "running", Some(SERVER_HEX)).await;

    let untouched = [
        0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x20, 0x30, 0x31, 0x41,
    ];
    let before = rows_of_operations(&mut old, &untouched).await;
    let older_prune = stored_row(&mut old, 0x10).await;
    let older_check = stored_row(&mut old, 0x11).await;
    let older_forget = stored_row(&mut old, 0x40).await;
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    assert_eq!(
        rows_of_operations(&mut raw, &untouched).await,
        before,
        "the newest running maintenance operation of each repository, and every row that isn't one, are as they were"
    );
    // The older ones are failed, in the way a crashed worker is: the engine
    // ended without telling an exit status, and nothing says when.
    for (n, mut expected) in [
        (0x10, older_prune),
        (0x11, older_check),
        (0x40, older_forget),
    ] {
        expected.status = "failed".to_owned();
        expected.error_code = Some("engine-exited".to_owned());
        expected.error_args = Some("[]".to_owned());
        assert_eq!(
            stored_row(&mut raw, n).await,
            expected,
            "operation {n:#x} is failed, and nothing else about it changed"
        );
    }
    for (n, kind) in [(0x10, OperationKind::Prune), (0x11, OperationKind::Check)] {
        assert_eq!(
            store.get(id(n)).await,
            Ok(Some(OperationDetail {
                record: demoted(n, kind),
                warnings: Vec::new()
            })),
            "the log reads operation {n:#x} back as failed without an exit status"
        );
    }
    for running in [0x12, 0x20, 0x41] {
        let found = store.get(id(running)).await;
        let status = found.map(|found| found.map(|detail| detail.record.status));
        assert_eq!(
            status,
            Ok(Some(OperationStatus::Running)),
            "operation {running:#x} still runs"
        );
    }

    // The rule holds from here on.
    assert_eq!(
        applied_versions(&mut raw).await,
        [
            (1, true),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
            (7, true),
            (8, true),
            (9, true),
            (10, true),
            (11, true),
            (12, true),
            (13, true),
            (14, true)
        ]
    );
    let prune = started(queued_on_repository(0x50, OperationKind::Prune));
    assert_eq!(
        store.try_start(&prune).await,
        Ok(Start::Refused { running: id(0x12) }),
        "the repository is held by the newest of its running operations"
    );
    let laptop_check = started(queued(
        0x51,
        OperationKind::Check,
        Trigger::Manual,
        Target {
            plan: None,
            repository: Some(RepositoryId::parse(LAPTOP_HEX).expect("a valid repository ID")),
        },
    ));
    assert_eq!(
        store.try_start(&laptop_check).await,
        Ok(Start::Refused { running: id(0x20) })
    );
    let no_repository = started(queued(
        0x52,
        OperationKind::Prune,
        Trigger::Manual,
        Target::default(),
    ));
    assert_eq!(store.try_start(&no_repository).await, Ok(Start::Started));
}

#[tokio::test]
async fn an_old_database_with_nothing_to_fix_is_upgraded_without_changing_a_row() {
    let scratch = Scratch::new();
    let mut old = database_at_version_1(&scratch).await;
    legacy(&mut old, 0x10, "prune", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x11, "backup", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x12, "backup", "running", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x13, "check", "queued", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x14, "check", "succeeded", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x15, "prune", "failed", None).await;
    let everything = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15];
    let before = rows_of_operations(&mut old, &everything).await;
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    assert_eq!(rows_of_operations(&mut raw, &everything).await, before);
    assert_eq!(
        applied_versions(&mut raw).await,
        [
            (1, true),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
            (7, true),
            (8, true),
            (9, true),
            (10, true),
            (11, true),
            (12, true),
            (13, true),
            (14, true)
        ]
    );
    let check = started(queued_on_repository(0x20, OperationKind::Check));
    assert_eq!(
        store.try_start(&check).await,
        Ok(Start::Refused { running: id(0x10) })
    );
}

/// The rows a build that didn't keep the alias or the bytes processed wrote
/// read back as they were: no alias, and no bytes processed, never a guess.
#[tokio::test]
async fn a_log_from_before_the_alias_was_kept_reads_back_without_one() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 3).await;
    legacy(&mut old, 0x60, "check", "succeeded", Some(REPOSITORY_HEX)).await;
    let summary = format!(
        r#"{{"type":"backup","repository":"{REPOSITORY_HEX}","snapshot":"{SNAPSHOT_HEX}","parent":null,"files_new":1,"files_changed":2,"files_unmodified":3,"data_added":4,"data_added_packed":5,"error_count":6}}"#
    );
    sqlx::query(
        r#"INSERT INTO operations (id, agent_id, kind, plan_id, repo_uuid, "trigger", status,
                                   queued_at, started_at, finished_at, snapshot_id, summary_json)
           VALUES (?1, 'local', 'backup', 'demo-home', ?2, 'schedule', 'succeeded', ?3, ?4, ?5,
                   ?6, ?7)"#,
    )
    .bind(id_text(0x61))
    .bind(REPOSITORY_HEX)
    .bind(ms(0))
    .bind(ms(10))
    .bind(ms(70))
    .bind(SNAPSHOT_HEX)
    .bind(summary)
    .execute(&mut old)
    .await
    .expect("the old backup row is written");
    let both = [0x60, 0x61];
    let before = rows_of_operations(&mut old, &both).await;
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    assert_eq!(
        rows_of_operations(&mut raw, &both).await,
        before,
        "the upgrade changes no row"
    );
    assert_eq!(
        applied_versions(&mut raw).await,
        [
            (1, true),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
            (7, true),
            (8, true),
            (9, true),
            (10, true),
            (11, true),
            (12, true),
            (13, true),
            (14, true)
        ]
    );
    let check = store
        .get(id(0x60))
        .await
        .expect("reading works")
        .expect("the check is in the log");
    assert_eq!(check.record.kind, OperationKind::Check);
    assert_eq!(check.record.repository_alias, None);
    assert_eq!(
        check.record.locations, None,
        "an earlier row records no location"
    );
    let backup = store
        .get(id(0x61))
        .await
        .expect("reading works")
        .expect("the backup is in the log");
    assert_eq!(
        backup.record.result,
        Some(EngineResult::Backup(BackupSummary {
            parent: None,
            bytes_processed: None,
            ..backup_summary()
        }))
    );
    assert_eq!(backup.record.repository_alias, None);
    assert_eq!(backup.record.locations, None);
}

/// A log that already has a `repo_alias` column, which no build of Ladderback
/// wrote, is refused: the migration that adds the column fails, and the log
/// stays at the version it was, its rows as they were.
#[tokio::test]
async fn a_log_that_already_has_the_alias_column_is_refused_and_left_at_its_version() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 3).await;
    legacy(&mut old, 0x70, "check", "succeeded", Some(REPOSITORY_HEX)).await;
    sqlx::query("ALTER TABLE operations ADD COLUMN repo_alias TEXT")
        .execute(&mut old)
        .await
        .expect("the column is added by hand");
    let before = rows_of_operations(&mut old, &[0x70]).await;
    old.close().await.expect("the old database is closed");

    let error = open_error(&scratch.path()).await;

    assert_matches!(&error, OpenError::Migration { detail } if !detail.is_empty());
    let mut raw = scratch.raw().await;
    assert_eq!(
        applied_versions(&mut raw).await,
        [(1, true), (2, true), (3, true)],
        "the migration that failed is not recorded"
    );
    assert_eq!(rows_of_operations(&mut raw, &[0x70]).await, before);
}

/// A log a build that kept the alias wrote gains the index of the operations
/// that succeeded, which the Repositories page asks whether a plan ever
/// succeeded (decision 53), and keeps every row as it was.
#[tokio::test]
async fn a_log_at_version_4_gains_the_index_of_successes_and_changes_no_row() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 4).await;
    legacy(&mut old, 0x80, "check", "succeeded", Some(REPOSITORY_HEX)).await;
    legacy(&mut old, 0x81, "backup", "failed", Some(REPOSITORY_HEX)).await;
    let both = [0x80, 0x81];
    let before = rows_of_operations(&mut old, &both).await;
    old.close().await.expect("the old database is closed");

    let _store = scratch.open().await;
    let mut raw = scratch.raw().await;
    assert_eq!(
        rows_of_operations(&mut raw, &both).await,
        before,
        "the upgrade changes no row"
    );
    assert_eq!(
        applied_versions(&mut raw).await,
        [
            (1, true),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
            (7, true),
            (8, true),
            (9, true),
            (10, true),
            (11, true),
            (12, true),
            (13, true),
            (14, true)
        ]
    );
    let index: Option<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'ops_succeeded_by_plan'",
    )
    .fetch_optional(&mut raw)
    .await
    .expect("the schema is readable");
    let index = index.expect("the upgrade adds the index");
    assert!(
        index.contains("ON operations(plan_id)")
            && index.contains("WHERE status IN ('succeeded', 'succeeded_with_warnings')"),
        "{index}"
    );
}

/// The API tokens' columns, as text, to compare a row before and after.
async fn api_token_row(conn: &mut SqliteConnection) -> Vec<String> {
    let row = sqlx::query(
        "SELECT id, label, hex(digest), scopes, password, created_at, \
         ifnull(last_used_at, 'NULL') FROM api_tokens",
    )
    .fetch_one(conn)
    .await
    .expect("the API token is readable");
    (0..7)
        .map(|column| {
            row.try_get::<String, _>(column)
                .or_else(|_| row.try_get::<i64, _>(column).map(|n| n.to_string()))
                .expect("a column reads as text or a number")
        })
        .collect()
}

#[tokio::test]
async fn a_database_at_version_9_gains_no_session_and_no_session_length_and_keeps_its_api_tokens() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 9).await;
    sqlx::query(
        "INSERT INTO api_tokens (id, label, digest, scopes, password, created_at, last_used_at) \
         VALUES ('t-1', 'demo-token', x'0101', 'read', -5, 1800000000000, NULL)",
    )
    .execute(&mut old)
    .await
    .expect("an API token of version 9");
    let before = api_token_row(&mut old).await;
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let sessions = app::ports::SessionStore::all(&store).await;
    assert!(
        matches!(&sessions, Ok(none) if none.is_empty()),
        "{sessions:?}"
    );
    let length = app::ports::SessionStore::length(&store).await;
    assert!(matches!(length, Ok(None)), "{length:?}");
    let mut raw = scratch.raw().await;
    assert_eq!(api_token_row(&mut raw).await, before);
    assert_eq!(applied_versions(&mut raw).await.last(), Some(&(14, true)));
}

#[tokio::test]
async fn a_database_at_version_10_gains_no_reading_and_keeps_its_sessions_length() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 10).await;
    sqlx::query("INSERT INTO sign_in_settings (id, session_length) VALUES (1, '7-days')")
        .execute(&mut old)
        .await
        .expect("a session length of version 10");
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let demo = domain::RepositoryAlias::parse("demo").expect("an alias");
    let latest = app::ports::StatsStore::latest(&store, &demo).await;
    assert!(matches!(latest, Ok(None)), "{latest:?}");
    let length = app::ports::SessionStore::length(&store).await;
    assert!(
        matches!(length, Ok(Some(domain::access::SessionLength::SevenDays))),
        "{length:?}"
    );
    let mut raw = scratch.raw().await;
    assert_eq!(applied_versions(&mut raw).await.last(), Some(&(14, true)));
}

#[tokio::test]
async fn a_database_at_version_11_gains_no_release_check_and_keeps_its_session_length() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 11).await;
    sqlx::query("INSERT INTO sign_in_settings (id, session_length) VALUES (1, '7-days')")
        .execute(&mut old)
        .await
        .expect("a session length of version 11");
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let check = app::ports::ScheduleStore::release_check(&store).await;
    assert!(matches!(check, Ok(None)), "{check:?}");
    let length = app::ports::SessionStore::length(&store).await;
    assert!(
        matches!(length, Ok(Some(domain::access::SessionLength::SevenDays))),
        "{length:?}"
    );
    let mut raw = scratch.raw().await;
    assert_eq!(applied_versions(&mut raw).await.last(), Some(&(14, true)));
}

#[tokio::test]
async fn a_database_at_version_12_gains_no_proposal_and_keeps_its_release_check() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 12).await;
    sqlx::query("INSERT INTO release_check (id, checked_at, update_version) VALUES (1, 0, NULL)")
        .execute(&mut old)
        .await
        .expect("a release check of version 12");
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let proposals = app::ports::ProposalStore::all(&store).await;
    assert_eq!(proposals, Ok(Vec::new()));
    let check = app::ports::ScheduleStore::release_check(&store).await;
    assert!(matches!(check, Ok(Some(_))), "{check:?}");
    let mut raw = scratch.raw().await;
    assert_eq!(applied_versions(&mut raw).await.last(), Some(&(14, true)));
}

#[tokio::test]
async fn a_database_at_version_13_keeps_its_proposals_with_nothing_decided() {
    let scratch = Scratch::new();
    let mut old = database_at_version(&scratch, 13).await;
    sqlx::query(concat!(
        "INSERT INTO proposals (id, token_id, token_label, base, changes, rationale, status, ",
        "created_at, expires_at) VALUES ('aaaaaaaaaaaa', NULL, NULL, ?, '[]', '', 'pending', ",
        "0, 172800000)"
    ))
    .bind("0".repeat(64))
    .execute(&mut old)
    .await
    .expect("a proposal of version 13");
    old.close().await.expect("the old database is closed");

    let store = scratch.open().await;
    let proposals = app::ports::ProposalStore::all(&store).await.unwrap();
    let [kept] = proposals.as_slice() else {
        panic!("one proposal: {proposals:?}");
    };
    assert_eq!(kept.status, domain::planning::ProposalStatus::Pending);
    assert_eq!(kept.decided_at, None);
    assert!(kept.reasons.is_empty());
    assert_eq!(kept.approved_from, None);
    assert_eq!(kept.applied_as, None);
    let mut raw = scratch.raw().await;
    assert_eq!(applied_versions(&mut raw).await.last(), Some(&(14, true)));
}
