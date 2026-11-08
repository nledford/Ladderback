//! Shared helpers of the store's integration tests: scratch databases, raw
//! connections to the same file, and the operations the tests save.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use app::ports::{BackupSummary, EngineResult, OperationDetail, OperationRecord, OperationStore};
use domain::execution::{
    FailureReason, Operation, OperationKind, Target, Trigger, Warning, WarningKind,
};
use domain::{OperationId, PlanId, RepositoryId, SnapshotId, Timestamp, Uuid};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Row};
use store::{OpenError, SqliteStore};
use tempfile::TempDir;

/// A scratch directory holding one database, `steading.db`.
pub struct Scratch {
    dir: TempDir,
}

impl Scratch {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("steading-store-")
            .tempdir()
            .expect("a scratch directory");
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().join("steading.db")
    }

    /// The store on this scratch database; the first call creates it.
    pub async fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path())
            .await
            .expect("the scratch database opens")
    }

    /// A connection of our own to the same file, like another program's or an
    /// operator's `sqlite3`.
    pub async fn raw(&self) -> SqliteConnection {
        raw_connection(&self.path()).await
    }
}

pub async fn raw_connection(path: &Path) -> SqliteConnection {
    SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .expect("a raw connection to the scratch database")
}

/// Why `open` refused `path`; a path that opens fails the test.
pub async fn open_error(path: &Path) -> OpenError {
    match SqliteStore::open(path).await {
        Ok(_) => panic!("{} opened, but should have been refused", path.display()),
        Err(error) => error,
    }
}

/// `work`, or a failed test when it takes more than `seconds`: a store that
/// hangs must fail its test, not stall the run.
pub async fn within<T>(seconds: u64, what: &str, work: impl Future<Output = T>) -> T {
    match tokio::time::timeout(Duration::from_secs(seconds), work).await {
        Ok(done) => done,
        Err(_) => panic!("{what} didn't finish within {seconds} s"),
    }
}

pub fn id(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

/// The text the database keeps an operation's ID as: lowercase and hyphenated.
pub fn id_text(n: u128) -> String {
    Uuid::from_u128(n).to_string()
}

/// `seconds` after a fixed instant, on a whole second.
pub fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

/// `at(seconds)` as the Unix milliseconds the database keeps.
pub fn ms(seconds: i64) -> i64 {
    (1_800_000_000 + seconds) * 1000
}

pub fn plan(slug: &str) -> PlanId {
    PlanId::parse(slug).expect("a valid plan ID")
}

pub const REPOSITORY_HEX: &str = "abababababababababababababababababababababababababababababababab";
pub const SNAPSHOT_HEX: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
pub const PARENT_HEX: &str = "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

pub fn repository() -> RepositoryId {
    RepositoryId::parse(REPOSITORY_HEX).expect("a valid repository ID")
}

pub fn snapshot() -> SnapshotId {
    SnapshotId::parse(SNAPSHOT_HEX).expect("a valid snapshot ID")
}

pub fn parent_snapshot() -> SnapshotId {
    SnapshotId::parse(PARENT_HEX).expect("a valid snapshot ID")
}

/// A backup of the demo plan `slug` into the demo repository.
pub fn backup_target(slug: &str) -> Target {
    Target {
        plan: Some(plan(slug)),
        repository: Some(repository()),
    }
}

pub fn queued(n: u128, kind: OperationKind, trigger: Trigger, target: Target) -> Operation {
    Operation::queue(id(n), kind, trigger, target, at(0))
}

/// A queued scheduled backup of the demo plan `demo-home`.
pub fn queued_backup(n: u128) -> Operation {
    queued(
        n,
        OperationKind::Backup,
        Trigger::Schedule,
        backup_target("demo-home"),
    )
}

pub fn started(mut op: Operation) -> Operation {
    op.start(at(10)).expect("a queued operation starts");
    op
}

pub fn succeeded(op: Operation) -> Operation {
    let mut op = started(op);
    op.succeed(at(70)).expect("a running operation succeeds");
    op
}

pub fn failed(op: Operation, reason: FailureReason) -> Operation {
    let mut op = started(op);
    op.fail(reason, at(70)).expect("a running operation fails");
    op
}

/// A running operation, given `count` warnings; one that isn't running panics.
pub fn with_warnings(mut op: Operation, count: usize) -> Operation {
    for n in 0..count {
        op.record_warning(warning(&format!("/home/demo/file-{n}")))
            .expect("a running operation records warnings");
    }
    op
}

pub fn warning(item: &str) -> Warning {
    Warning {
        kind: WarningKind::SourceUnreadable,
        item: Some(item.to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    }
}

pub fn backup_summary() -> BackupSummary {
    BackupSummary {
        repository: repository(),
        snapshot: snapshot(),
        parent: Some(parent_snapshot()),
        files_new: 1,
        files_changed: 2,
        files_unmodified: 3,
        data_added: 4,
        data_added_packed: 5,
        error_count: 6,
    }
}

pub fn backup_result() -> EngineResult {
    EngineResult::Backup(backup_summary())
}

pub async fn save(store: &SqliteStore, op: &Operation, result: Option<&EngineResult>) {
    let saved = store.save(op, result).await;
    assert_eq!(saved, Ok(()), "saving operation {}", op.id().as_uuid());
}

/// What the log must hand back for `op` saved with `result`, worked out from
/// the operation alone.
pub fn detail_of(op: &Operation, result: Option<&EngineResult>) -> OperationDetail {
    OperationDetail {
        record: OperationRecord::of(op, result),
        warnings: op.warnings().to_vec(),
    }
}

/// One operation's columns as the database holds them.
#[derive(Debug, PartialEq)]
pub struct Stored {
    pub agent_id: String,
    pub kind: String,
    pub plan_id: Option<String>,
    pub repo_uuid: Option<String>,
    pub trigger: String,
    pub status: String,
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub snapshot_id: Option<String>,
    pub summary_json: Option<String>,
    pub error_code: Option<String>,
    pub error_args: Option<String>,
    pub error_detail: Option<String>,
    pub warning_count: i64,
}

/// The raw row of operation `n`; the operation must be there.
pub async fn stored_row(conn: &mut SqliteConnection, n: u128) -> Stored {
    let row = sqlx::query(
        r#"SELECT agent_id, kind, plan_id, repo_uuid, "trigger", status, queued_at, started_at,
                  finished_at, snapshot_id, summary_json, error_code, error_args, error_detail,
                  warning_count
           FROM operations WHERE id = ?1"#,
    )
    .bind(id_text(n))
    .fetch_one(&mut *conn)
    .await
    .expect("the operation has a row");
    Stored {
        agent_id: row.get("agent_id"),
        kind: row.get("kind"),
        plan_id: row.get("plan_id"),
        repo_uuid: row.get("repo_uuid"),
        trigger: row.get("trigger"),
        status: row.get("status"),
        queued_at: row.get("queued_at"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        snapshot_id: row.get("snapshot_id"),
        summary_json: row.get("summary_json"),
        error_code: row.get("error_code"),
        error_args: row.get("error_args"),
        error_detail: row.get("error_detail"),
        warning_count: row.get("warning_count"),
    }
}

/// One warning's columns as the database holds them.
#[derive(Debug, PartialEq)]
pub struct StoredWarning {
    pub item: Option<String>,
    pub during: String,
    pub code: String,
    pub detail: String,
}

/// The raw warning rows of operation `n`, in the order they were inserted.
pub async fn stored_warnings(conn: &mut SqliteConnection, n: u128) -> Vec<StoredWarning> {
    let rows = sqlx::query(
        "SELECT item, during, code, detail FROM op_warnings WHERE op_id = ?1 ORDER BY rowid",
    )
    .bind(id_text(n))
    .fetch_all(&mut *conn)
    .await
    .expect("warning rows are readable");
    rows.iter()
        .map(|row| StoredWarning {
            item: row.get("item"),
            during: text(row.get("during")),
            code: text(row.get("code")),
            detail: text(row.get("detail")),
        })
        .collect()
}

/// A column the store never leaves NULL.
fn text(column: Option<String>) -> String {
    column.expect("the store never writes NULL here")
}

/// The count a `SELECT COUNT(*) …` with no parameters returns.
pub async fn count(conn: &mut SqliteConnection, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(&mut *conn)
        .await
        .expect("a count")
}

/// How many rows of `table` hold operation `n`'s ID in `column`.
pub async fn rows_of(conn: &mut SqliteConnection, table: &str, column: &str, n: u128) -> i64 {
    let sql = match (table, column) {
        ("operations", "id") => "SELECT COUNT(*) FROM operations WHERE id = ?1",
        ("op_warnings", "op_id") => "SELECT COUNT(*) FROM op_warnings WHERE op_id = ?1",
        _ => panic!("no such count: {table}.{column}"),
    };
    sqlx::query_scalar::<_, i64>(sql)
        .bind(id_text(n))
        .fetch_one(&mut *conn)
        .await
        .expect("a count")
}
