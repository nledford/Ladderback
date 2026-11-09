//! Upgrading a database an earlier build wrote. The migration that adds the
//! one-maintenance-operation-per-repository rule must cope with a log that
//! already breaks it, because nothing stopped an earlier build from recording
//! two running maintenance operations on one repository.

mod support;

use std::fs;
use std::path::Path;

use app::ports::{OperationDetail, OperationRecord, OperationStore, Start};
use domain::RepositoryId;
use domain::execution::{FailureReason, OperationKind, OperationStatus, Target, Trigger};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection, Row};
use support::*;

/// Other repositories' IDs, to tell them from the demo repository's.
const LAPTOP_HEX: &str = "1212121212121212121212121212121212121212121212121212121212121212";
const SERVER_HEX: &str = "3434343434343434343434343434343434343434343434343434343434343434";

/// The database file as the first build left it: the first migration applied,
/// and no other.
async fn database_at_version_1(scratch: &Scratch) -> SqliteConnection {
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../migrations");
    let only_the_first = scratch.dir().join("first-migration-only");
    fs::create_dir(&only_the_first).expect("a directory for the first migration");
    fs::copy(
        migrations.join("0001_operations.sql"),
        only_the_first.join("0001_operations.sql"),
    )
    .expect("the first migration is copied");
    let first = Migrator::new(only_the_first.as_path())
        .await
        .expect("the first migration is read");
    let mut old = SqliteConnectOptions::new()
        .filename(scratch.path())
        .create_if_missing(true)
        .connect()
        .await
        .expect("the old database is created");
    first
        .run(&mut old)
        .await
        .expect("the first migration is applied");
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
        result: None,
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
    assert_eq!(applied_versions(&mut raw).await, [(1, true), (2, true)]);
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
    assert_eq!(applied_versions(&mut raw).await, [(1, true), (2, true)]);
    let check = started(queued_on_repository(0x20, OperationKind::Check));
    assert_eq!(
        store.try_start(&check).await,
        Ok(Start::Refused { running: id(0x10) })
    );
}
