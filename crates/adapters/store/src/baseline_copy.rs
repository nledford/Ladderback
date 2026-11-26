//! The one-off copy of an operation log the migrations before the baseline
//! schema left into the baseline (§16, decisions 165 and 170), so a machine
//! running 0.5.0 or a build after it upgrades by installing this version. A
//! deliberate exception to "no shims": removed at 1.0, with
//! `migrations-before-baseline/`.
//!
//! Only a process that holds the data folder's lock copies, so no other
//! Ladderback, 0.5.0's daemon among them, has the log open while it is
//! replaced:
//!
//! 1. The log must hold migrations 1 to N of the old set, N at most 29,
//!    each applied with the checksum of the file Ladderback shipped (0.5.0
//!    stopped at 16). Any other is refused with [`OpenError::OldSchema`]
//!    and left as it was.
//! 2. The log's write-ahead log is checkpointed into it, and the whole log
//!    copied to `<log>.before-baseline-new` (`VACUUM INTO`).
//! 3. A copy of that, `<log>.baseline-source`, gets the old migrations
//!    N+1 to 29, as an earlier build's start would have run them.
//! 4. A new log, `<log>.baseline-new`, gets the baseline schema, then every
//!    row of the source, table by table in one transaction, with each row's
//!    rowid, so the order the store reads in stays. `operations.machine_id`,
//!    which the baseline dropped, is left out. Only rows the baseline's
//!    rules prove are junk are dropped, each counted: the plans, progress
//!    and ownership reports of machines the hub had removed, which the
//!    baseline's cascades would have deleted with them, and a second report
//!    of an operation's happening the hub took under another machine ID
//!    (#1513), the first one kept; the tally of a machine that lost one
//!    counts what it still holds. The counts, `foreign_key_check` and
//!    `integrity_check` must agree.
//! 5. The source is removed, the copy from step 2 becomes
//!    `<log>.before-baseline`, or `<log>.before-baseline-2` and on when an
//!    earlier start kept one (never replaced: it may be the only untouched
//!    original), the log's emptied `-wal` and `-shm` are removed, and the
//!    new log is renamed over the old one.
//!
//! Every work file is made empty with the old log's permissions before
//! step 2, so none is ever more open than the log whose rows it holds.
//!
//! The old log is never written until the rename, which replaces it whole,
//! so a copy that stops at any step leaves it usable: the next one removes
//! the work files and copies it again. A step that fails does the same now
//! and says why with [`OpenError::BaselineCopy`].

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePool, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, ConnectOptions, Connection, Row};

use crate::report::cap;
use crate::{BUSY_TIMEOUT, MIGRATOR, OpenError, sql};

/// The migrations before the baseline, as Ladderback shipped them, 0001 to
/// 0029, embedded when the crate is compiled.
static OLD_MIGRATOR: Migrator = sqlx::migrate!("./migrations-before-baseline");

/// The baseline schema's own migration, the one the copy builds on; the
/// open runs any later one afterwards, as on every log.
const BASELINE_VERSION: i64 = 1;

/// The files beside the log, by the suffix after its name.
const KEPT: &str = ".before-baseline";
const KEPT_NEW: &str = ".before-baseline-new";
const SOURCE: &str = ".baseline-source";
const NEW: &str = ".baseline-new";

/// The tables that hold a machine's current state, which go with the
/// machine (the baseline's cascades).
const CASCADED: [&str; 4] = [
    "sync_plans",
    "sync_plan_snapshots",
    "sync_progress",
    "ownership_reports",
];

/// The tables a foreign key refers to, copied first.
const PARENTS: [&str; 2] = ["machines", "operations"];

/// The old columns the baseline dropped, by table.
const DROPPED_COLUMNS: [(&str, &str); 1] = [("operations", "machine_id")];

/// What the copy of an old log into the baseline did, for the daemon's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaselineCopy {
    /// The last migration of the old set the log held.
    pub from_version: i64,
    /// Where the old log is kept: `<log>.before-baseline`.
    pub kept: PathBuf,
    /// The rows the baseline's rules prove are junk, dropped, by table.
    pub dropped: Vec<Dropped>,
    /// Why the folder couldn't be synced once the new log took the old
    /// one's place, in the operating system's words. The copy is done and
    /// the new log is the one in use; a power loss before the folder
    /// reaches the disk could bring the old log back, which the next start
    /// copies again. A warning, never a failure.
    pub unsynced: Option<String>,
}

/// Rows of one table the copy dropped as junk (decision 165): a removed
/// machine's current state, or a second report of an operation's happening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub table: String,
    pub rows: u64,
}

/// Why a copy of an old log into the baseline failed (decision 170). Each
/// detail is the database's or the operating system's own words, capped;
/// the rest is for the caller's catalog to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyFailure {
    /// A work file beside the log couldn't be made, copied, synced, moved
    /// or removed.
    Files { detail: String },
    /// A database step failed for a reason another try may not meet, such
    /// as a full disk, an I/O error or a lock.
    Database { detail: String },
    /// One of the old migrations the log lacked failed on its rows.
    OldMigration { detail: String },
    /// The log's tables, with the old migrations run, are not the ones the
    /// baseline copies.
    Tables {
        old: Vec<String>,
        baseline: Vec<String>,
    },
    /// A table's columns are not the ones migration 29 left: those named
    /// are missing or extra.
    Columns {
        table: String,
        unmatched: Vec<String>,
    },
    /// A row of `table` breaks one of the baseline's constraints.
    RefusedRow { table: String, detail: String },
    /// The rows copied and dropped of `table` don't add up to its rows.
    Counts {
        table: String,
        source: u64,
        copied: u64,
        dropped: u64,
    },
    /// The new log breaks `broken` of its foreign keys.
    ForeignKeys { broken: usize },
    /// The new log fails SQLite's integrity check.
    Integrity { detail: String },
    /// A work file's path isn't UTF-8, which a statement needs.
    NotUtf8 { path: PathBuf },
}

impl CopyFailure {
    fn files(error: impl std::fmt::Display) -> Self {
        Self::Files {
            detail: cap(error.to_string()),
        }
    }

    fn database(error: sqlx::Error) -> Self {
        Self::Database {
            detail: cap(error.to_string()),
        }
    }
}

/// Whether `error` is one another try may not meet: an I/O error, a full
/// disk, a lock or too little memory, rather than one of the data's own.
fn transient(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db
            .code()
            .and_then(|code| code.parse::<i32>().ok())
            // BUSY, LOCKED, NOMEM, READONLY, IOERR, FULL, CANTOPEN, PROTOCOL.
            .is_none_or(|code| matches!(code & 0xff, 5..=8 | 10 | 13..=15)),
        _ => true,
    }
}

/// Copies the old log at `log` into the baseline, as the module says.
/// `None` when `log` isn't one the migrations before the baseline left.
pub(crate) async fn copy(log: &Path) -> Result<Option<BaselineCopy>, OpenError> {
    let recorded = recorded(log).await.map_err(|error| OpenError::Database {
        detail: cap(error.to_string()),
    })?;
    let Some(recorded) = recorded else {
        return Ok(None);
    };
    let last = recorded.last().map_or(0, |migration| migration.version);
    if !of_the_old_set(&recorded) {
        return Err(OpenError::OldSchema { version: last });
    }
    let work = Work::beside(log);
    work.clear()
        .map_err(|error| failed(last, CopyFailure::files(error)))?;
    match work.run().await {
        Ok((dropped, swapped)) => Ok(Some(BaselineCopy {
            from_version: last,
            kept: swapped.kept,
            dropped,
            unsynced: swapped.unsynced,
        })),
        Err(stop) => {
            // Best effort: the next copy removes what is left all the same.
            let _ = work.clear();
            Err(match stop {
                Stop::InUse => OpenError::LogInUse { version: last },
                Stop::Failed(cause) => failed(last, cause),
            })
        }
    }
}

/// A copy of the log at `version` that failed, saying why.
const fn failed(version: i64, cause: CopyFailure) -> OpenError {
    OpenError::BaselineCopy { version, cause }
}

/// One migration a log records.
struct Recorded {
    version: i64,
    success: bool,
    checksum: Vec<u8>,
}

/// Whether `log` is one the migrations before the baseline left, whatever
/// migrations it holds. It only reads.
pub(crate) async fn is_old(log: &Path) -> Result<bool, sqlx::Error> {
    recorded(log).await.map(|recorded| recorded.is_some())
}

/// The migrations `log` records, in order, when the first is the old set's;
/// `None` when it isn't (a baseline log).
async fn recorded(log: &Path) -> Result<Option<Vec<Recorded>>, sqlx::Error> {
    let mut conn = SqliteConnectOptions::new()
        .filename(log)
        .busy_timeout(BUSY_TIMEOUT)
        .connect()
        .await?;
    let read = read_recorded(&mut conn).await;
    let closed = conn.close().await;
    let read = read?;
    closed?;
    Ok(read)
}

async fn read_recorded(conn: &mut SqliteConnection) -> Result<Option<Vec<Recorded>>, sqlx::Error> {
    let has: bool = sqlx::query_scalar(sql::HAS_MIGRATIONS)
        .fetch_one(&mut *conn)
        .await?;
    if !has {
        return Ok(None);
    }
    let old: Option<i64> = sqlx::query_scalar(sql::OLD_SET_VERSION)
        .bind(sql::OLD_SET_FIRST_CHECKSUM)
        .fetch_one(&mut *conn)
        .await?;
    if old.is_none() {
        return Ok(None);
    }
    let rows =
        sqlx::query("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut *conn)
            .await?;
    rows.iter()
        .map(|row| {
            Ok(Recorded {
                version: row.try_get("version")?,
                success: row.try_get("success")?,
                checksum: row.try_get("checksum")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map(Some)
}

/// Whether `recorded` is migrations 1 to N of the old set, each applied
/// with the checksum of the file Ladderback shipped.
fn of_the_old_set(recorded: &[Recorded]) -> bool {
    !recorded.is_empty()
        && recorded.len() <= OLD_MIGRATOR.migrations.len()
        && recorded.iter().zip(OLD_MIGRATOR.iter()).all(|(had, old)| {
            had.version == old.version && had.success && had.checksum == *old.checksum
        })
}

/// `log` with `suffix` after its name.
fn beside(log: &Path, suffix: &str) -> PathBuf {
    let mut name = log.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The files of one copy, beside the log.
struct Work {
    log: PathBuf,
    kept: PathBuf,
    kept_new: PathBuf,
    source: PathBuf,
    new: PathBuf,
}

impl Work {
    fn beside(log: &Path) -> Self {
        Self {
            log: log.to_owned(),
            kept: beside(log, KEPT),
            kept_new: beside(log, KEPT_NEW),
            source: beside(log, SOURCE),
            new: beside(log, NEW),
        }
    }

    /// Removes the work files a copy that stopped partway left, with their
    /// own journals.
    fn clear(&self) -> io::Result<()> {
        for file in [&self.kept_new, &self.source, &self.new] {
            for suffix in ["", "-journal", "-wal", "-shm"] {
                remove_if_there(&beside(file, suffix))?;
            }
        }
        Ok(())
    }

    /// Steps 2 to 5; the old log is unchanged by all but the last rename.
    async fn run(&self) -> Result<(Vec<Dropped>, Swapped), Stop> {
        let dropped = self.copy_rows().await?;
        remove_if_there(&self.source).map_err(CopyFailure::files)?;
        let swapped = self.swap().map_err(CopyFailure::files)?;
        Ok((dropped, swapped))
    }

    /// Steps 2 to 4: the snapshot, the source and the new log, each a
    /// copy of the old log's rows, made first empty and as open as the old
    /// log, never more, before any row is written to it.
    async fn copy_rows(&self) -> Result<Vec<Dropped>, Stop> {
        self.make_files().map_err(CopyFailure::files)?;
        snapshot(&self.log, &self.kept_new).await?;
        copy_file(self.kept_new.clone(), self.source.clone()).await?;
        migrate_old(&self.source).await?;
        Ok(build(&self.new, &self.source).await?)
    }

    /// Makes each work file empty with the old log's permissions. SQLite
    /// writes a snapshot into an empty file and takes it as a new
    /// database, and gives a database's journals its permissions.
    fn make_files(&self) -> io::Result<()> {
        let permissions = fs::metadata(&self.log)?.permissions();
        for file in [&self.kept_new, &self.source, &self.new] {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                options.mode(permissions.mode() & 0o777);
            }
            options.open(file)?;
            // The umask may have taken more away than the log's mode did.
            fs::set_permissions(file, permissions.clone())?;
        }
        Ok(())
    }

    /// Step 5: the new log in the old one's place, the old one kept, at
    /// the path it answers. Once the rename is made, the copy is done:
    /// the folder's sync after it can only warn. A step that fails before
    /// it leaves the old log in use, so the copy this attempt kept is
    /// removed: every failed attempt would otherwise leave one more.
    fn swap(&self) -> io::Result<Swapped> {
        for file in [&self.kept_new, &self.new] {
            File::open(file)?.sync_all()?;
        }
        let kept = self.keep()?;
        match self.replace() {
            Ok(()) => Ok(Swapped::after_rename(kept, sync_dir(self.folder()))),
            Err(error) => {
                // Best effort: the error says why the copy stopped.
                let _ = remove_if_there(&kept);
                Err(error)
            }
        }
    }

    /// Renames the new log over the old one, once the old one's emptied
    /// `-wal` and `-shm` are gone and the kept copy's name is durable.
    fn replace(&self) -> io::Result<()> {
        // Empty since the checkpoint: a crash right after leaves the old
        // log whole.
        for suffix in ["-wal", "-shm"] {
            remove_if_there(&beside(&self.log, suffix))?;
        }
        sync_dir(self.folder())?;
        fs::rename(&self.new, &self.log)
    }

    /// The folder the log is in.
    fn folder(&self) -> &Path {
        match self.log.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder,
            _ => Path::new("."),
        }
    }

    /// Gives the copy from step 2 the first free name of
    /// `<log>.before-baseline`, `<log>.before-baseline-2`, …: a link fails
    /// rather than replace a file, so a kept copy is never lost. The link
    /// is made durable before the work name is dropped, so a crash between
    /// leaves the copy under one name or both, never neither.
    fn keep(&self) -> io::Result<PathBuf> {
        let mut n: u32 = 1;
        loop {
            let kept = if n == 1 {
                self.kept.clone()
            } else {
                beside(&self.kept, &format!("-{n}"))
            };
            match fs::hard_link(&self.kept_new, &kept) {
                Ok(()) => {
                    sync_dir(self.folder())?;
                    fs::remove_file(&self.kept_new)?;
                    return Ok(kept);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    n = n.checked_add(1).ok_or(error)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Makes the renames durable.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

async fn copy_file(from: PathBuf, to: PathBuf) -> Result<(), CopyFailure> {
    tokio::task::spawn_blocking(move || fs::copy(from, to))
        .await
        .map_err(CopyFailure::files)?
        .map(|_| ())
        .map_err(CopyFailure::files)
}

/// A path as SQLite takes it in a statement.
fn utf8(path: &Path) -> Result<&str, CopyFailure> {
    path.to_str().ok_or_else(|| CopyFailure::NotUtf8 {
        path: path.to_path_buf(),
    })
}

/// One connection to `path`, as the store's but for WAL: foreign keys on.
/// A pool of one rather than a bare connection: a migrator run on a bare
/// connection makes the future not `Send` to the compiler.
async fn connect(path: &Path, create: bool) -> Result<SqlitePool, CopyFailure> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create)
        .busy_timeout(BUSY_TIMEOUT);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(CopyFailure::database)
}

/// Closes `pool` after `work`, whatever `work` did, and answers `work`'s.
async fn closing<T, E>(pool: SqlitePool, work: Result<T, E>) -> Result<T, E> {
    pool.close().await;
    work
}

/// The new log in the old one's place, and where the old one is kept.
struct Swapped {
    kept: PathBuf,
    /// Why the folder couldn't be synced after the rename, if it couldn't.
    unsynced: Option<String>,
}

impl Swapped {
    /// The swap once its rename is made: the folder's `synced` after it
    /// can only warn, as the new log is the one in use.
    fn after_rename(kept: PathBuf, synced: io::Result<()>) -> Self {
        Self {
            kept,
            unsynced: synced.err().map(|error| error.to_string()),
        }
    }
}

/// Why a copy stopped.
#[derive(Debug)]
enum Stop {
    /// Another program reads the log, so its write-ahead log can't be
    /// checkpointed into it.
    InUse,
    /// A step failed.
    Failed(CopyFailure),
}

impl From<CopyFailure> for Stop {
    fn from(cause: CopyFailure) -> Self {
        Self::Failed(cause)
    }
}

/// Step 2: the log, its write-ahead log checkpointed into it first, copied
/// whole to `to`.
async fn snapshot(log: &Path, to: &Path) -> Result<(), Stop> {
    let pool = connect(log, false).await?;
    let work = async {
        let mut conn = pool.acquire().await.map_err(CopyFailure::database)?;
        let row = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut *conn)
            .await
            .map_err(CopyFailure::database)?;
        let busy: i64 = row.try_get(0).map_err(CopyFailure::database)?;
        if busy != 0 {
            return Err(Stop::InUse);
        }
        sqlx::query("VACUUM INTO ?")
            .bind(utf8(to)?)
            .execute(&mut *conn)
            .await
            .map_err(CopyFailure::database)?;
        Ok(())
    }
    .await;
    closing(pool, work).await
}

/// Step 3: the old migrations the source lacks, as an earlier build's
/// start would have run them. One that fails on the log's rows fails the
/// same on every try; one that meets a full disk or a lock may not.
async fn migrate_old(source: &Path) -> Result<(), CopyFailure> {
    let pool = connect(source, false).await?;
    let work = OLD_MIGRATOR.run(&pool).await.map_err(|error| match error {
        MigrateError::Execute(inner) | MigrateError::ExecuteMigration(inner, _)
            if transient(&inner) =>
        {
            CopyFailure::database(inner)
        }
        other => CopyFailure::OldMigration {
            detail: cap(other.to_string()),
        },
    });
    closing(pool, work).await
}

/// Step 4: the new log at `new`, in the baseline, with the rows of the
/// source at `source`.
async fn build(new: &Path, source: &Path) -> Result<Vec<Dropped>, CopyFailure> {
    let pool = connect(new, true).await?;
    let work = async {
        MIGRATOR
            .run_to(BASELINE_VERSION, &pool)
            .await
            .map_err(|error| CopyFailure::Database {
                detail: cap(error.to_string()),
            })?;
        let mut conn = pool.acquire().await.map_err(CopyFailure::database)?;
        fill(&mut conn, source).await
    }
    .await;
    closing(pool, work).await
}

async fn fill(db: &mut SqliteConnection, source: &Path) -> Result<Vec<Dropped>, CopyFailure> {
    let text = CopyFailure::database;
    sqlx::query("ATTACH DATABASE ? AS old")
        .bind(utf8(source)?)
        .execute(&mut *db)
        .await
        .map_err(text)?;
    let tables = table_names(db, "main").await?;
    let old_tables = table_names(db, "old").await?;
    if tables != old_tables {
        return Err(CopyFailure::Tables {
            old: old_tables,
            baseline: tables,
        });
    }
    let mut tx = db.begin().await.map_err(text)?;
    let mut dropped = Vec::new();
    let mut order: Vec<String> = PARENTS.iter().map(|table| (*table).to_owned()).collect();
    order.extend(
        tables
            .into_iter()
            .filter(|table| !PARENTS.contains(&table.as_str())),
    );
    for table in order {
        let junk = copy_table(&mut tx, &table).await?;
        if junk > 0 {
            dropped.push(Dropped { table, rows: junk });
        }
    }
    // A machine's tally counts the reports it still holds.
    sqlx::query(
        "UPDATE main.sync_report_tallies SET held = (
            SELECT COUNT(*) FROM main.sync_reports AS r
            WHERE r.machine_id = sync_report_tallies.machine_id)
         WHERE machine_id IN (SELECT machine_id FROM old.sync_reports AS r
            WHERE r.id NOT IN (SELECT id FROM main.sync_reports))",
    )
    .execute(&mut *tx)
    .await
    .map_err(text)?;
    // An ID the old log gave is never given again.
    sqlx::query(
        "UPDATE main.sqlite_sequence
         SET seq = MAX(seq, (SELECT seq FROM old.sqlite_sequence WHERE name = 'sync_reports'))
         WHERE name = 'sync_reports'",
    )
    .execute(&mut *tx)
    .await
    .map_err(text)?;
    sqlx::query(
        "INSERT INTO main.sqlite_sequence (name, seq)
         SELECT name, seq FROM old.sqlite_sequence WHERE name = 'sync_reports'
           AND NOT EXISTS (SELECT 1 FROM main.sqlite_sequence WHERE name = 'sync_reports')",
    )
    .execute(&mut *tx)
    .await
    .map_err(text)?;
    let broken = sqlx::query("PRAGMA main.foreign_key_check")
        .fetch_all(&mut *tx)
        .await
        .map_err(text)?;
    if !broken.is_empty() {
        return Err(CopyFailure::ForeignKeys {
            broken: broken.len(),
        });
    }
    tx.commit().await.map_err(text)?;
    sqlx::query("DETACH DATABASE old")
        .execute(&mut *db)
        .await
        .map_err(text)?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *db)
        .await
        .map_err(text)?;
    if integrity != "ok" {
        return Err(CopyFailure::Integrity {
            detail: cap(integrity),
        });
    }
    Ok(dropped)
}

/// The tables of `schema`, by name, but SQLite's and sqlx's own.
async fn table_names(db: &mut SqliteConnection, schema: &str) -> Result<Vec<String>, CopyFailure> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT name FROM {schema}.sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name"
    )))
    .fetch_all(db)
    .await
    .map_err(CopyFailure::database)
}

/// One column of a table.
struct Column {
    name: String,
    ty: String,
    pk: i64,
}

async fn columns(
    db: &mut SqliteConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<Column>, CopyFailure> {
    let text = CopyFailure::database;
    let rows = sqlx::query(AssertSqlSafe(format!(
        "PRAGMA {schema}.table_info(\"{table}\")"
    )))
    .fetch_all(db)
    .await
    .map_err(text)?;
    rows.iter()
        .map(|row| {
            Ok(Column {
                name: row.try_get("name").map_err(text)?,
                ty: row.try_get("type").map_err(text)?,
                pk: row.try_get("pk").map_err(text)?,
            })
        })
        .collect()
}

async fn count(db: &mut SqliteConnection, sql: String) -> Result<u64, CopyFailure> {
    let n: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
        .fetch_one(db)
        .await
        .map_err(CopyFailure::database)?;
    // SQLite's COUNT is never negative.
    Ok(u64::try_from(n).unwrap_or(0))
}

/// Copies the rows of `table` the baseline keeps, with their rowids, and
/// answers how many it dropped as junk. The table's name is the
/// baseline's own, never outside text.
async fn copy_table(db: &mut SqliteConnection, table: &str) -> Result<u64, CopyFailure> {
    let kept = columns(db, "main", table).await?;
    let had: Vec<String> = columns(db, "old", table)
        .await?
        .into_iter()
        .map(|column| column.name)
        .collect();
    let missing = kept
        .iter()
        .filter(|column| !had.contains(&column.name))
        .map(|column| column.name.as_str());
    let extra = had.iter().map(String::as_str).filter(|name| {
        !kept.iter().any(|column| column.name == *name)
            && !DROPPED_COLUMNS.contains(&(table, *name))
    });
    let unmatched: Vec<String> = missing.chain(extra).map(str::to_owned).collect();
    if !unmatched.is_empty() {
        return Err(CopyFailure::Columns {
            table: table.to_owned(),
            unmatched,
        });
    }
    let keys: Vec<&Column> = kept.iter().filter(|column| column.pk > 0).collect();
    let rowid_alias = matches!(keys.as_slice(), [key] if key.ty.eq_ignore_ascii_case("INTEGER"));
    let names: Vec<String> = (!rowid_alias)
        .then(|| "rowid".to_owned())
        .into_iter()
        .chain(kept.iter().map(|column| format!("\"{}\"", column.name)))
        .collect();
    // The rows the baseline's rules keep; those of the old log's that don't
    // match are the junk the copy drops.
    let keep = if CASCADED.contains(&table) {
        Some("r.machine_id IN (SELECT id FROM old.machines)")
    } else if table == "sync_reports" {
        Some(
            "r.id = (SELECT MIN(first.id) FROM old.sync_reports AS first \
             WHERE first.op_id = r.op_id AND first.happening = r.happening)",
        )
    } else {
        None
    };
    let list = names.join(", ");
    let selected = names
        .iter()
        .map(|name| format!("r.{name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = count(db, format!("SELECT COUNT(*) FROM old.\"{table}\"")).await?;
    let junk = match keep {
        Some(keep) => {
            count(
                db,
                format!("SELECT COUNT(*) FROM old.\"{table}\" AS r WHERE NOT ({keep})"),
            )
            .await?
        }
        None => 0,
    };
    let filter = keep.map_or_else(String::new, |keep| format!("WHERE {keep}"));
    sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO main.\"{table}\" ({list}) SELECT {selected} FROM old.\"{table}\" AS r \
         {filter} ORDER BY r.rowid"
    )))
    .execute(&mut *db)
    .await
    .map_err(|error| {
        if transient(&error) {
            CopyFailure::database(error)
        } else {
            CopyFailure::RefusedRow {
                table: table.to_owned(),
                detail: cap(error.to_string()),
            }
        }
    })?;
    let copied = count(db, format!("SELECT COUNT(*) FROM main.\"{table}\"")).await?;
    if copied.checked_add(junk) != Some(source) {
        return Err(CopyFailure::Counts {
            table: table.to_owned(),
            source,
            copied,
            dropped: junk,
        });
    }
    Ok(junk)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log the migrations before the baseline left at 29, at `log`, in
    /// the rollback-journal mode, so SQLite never opens its `-shm`.
    async fn old_log_at_29(log: &Path) {
        let mut raw = SqliteConnectOptions::new()
            .filename(log)
            .create_if_missing(true)
            .connect()
            .await
            .expect("the old log is created");
        sqlx::raw_sql(include_str!("../tests/fixtures/at-0029.sql"))
            .execute(&mut raw)
            .await
            .expect("the old log is filled");
        raw.close().await.expect("the old log is closed");
    }

    /// A swap that fails after it kept the snapshot but before the rename
    /// leaves the old log in use, so the copy it kept is removed: a start
    /// that fails this way again and again never fills the disk with
    /// copies of the log.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_swap_that_fails_before_the_rename_removes_the_copy_it_kept() {
        let folder = tempfile::tempdir().expect("a scratch folder");
        let log = folder.path().join("ladderback.db");
        old_log_at_29(&log).await;
        let before = fs::read(&log).expect("the log reads");
        // A `-shm` the swap can't remove, as a folder with a file in it.
        let shm = beside(&log, "-shm");
        fs::create_dir(&shm).expect("a folder in the way");
        fs::write(shm.join("held"), "").expect("a file in it");

        let error = copy(&log).await.expect_err("the swap fails");

        assert!(
            matches!(error, OpenError::BaselineCopy { version: 29, .. }),
            "{error:?}"
        );
        assert_eq!(fs::read(&log).expect("the log reads"), before);
        let mut left: Vec<String> = fs::read_dir(folder.path())
            .expect("the folder lists")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        left.sort();
        assert_eq!(left, ["ladderback.db", "ladderback.db-shm"]);
    }

    /// The work files hold the old log's rows, the sessions and API
    /// tokens' fingerprints among them: from the moment each is made, it
    /// is no more open than the old log, whatever the process's umask.
    #[cfg(unix)]
    #[tokio::test]
    async fn every_work_file_is_as_private_as_the_old_log_while_the_copy_runs() {
        use std::os::unix::fs::PermissionsExt;

        let folder = tempfile::tempdir().expect("a scratch folder");
        let log = folder.path().join("ladderback.db");
        let mut raw = SqliteConnectOptions::new()
            .filename(&log)
            .create_if_missing(true)
            .connect()
            .await
            .expect("the old log is created");
        sqlx::raw_sql(include_str!("../tests/fixtures/at-0029.sql"))
            .execute(&mut raw)
            .await
            .expect("the old log is filled");
        raw.close().await.expect("the old log is closed");
        fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).expect("the log is private");

        let work = Work::beside(&log);
        work.copy_rows().await.expect("the rows are copied");

        for file in [&work.kept_new, &work.source, &work.new] {
            let mode = fs::metadata(file)
                .expect("the work file is there")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "{} is its owner's alone",
                file.display()
            );
        }
    }

    /// The folder's sync after the rename failing leaves the copy done:
    /// the new log is the one in use, so it is a finished copy with a
    /// warning in the operating system's words, never a failure that says
    /// the old log was left as it was.
    #[test]
    fn a_folder_that_cant_be_synced_after_the_rename_is_a_finished_copy_with_a_warning() {
        let kept = PathBuf::from("ladderback.db.before-baseline");

        let swapped = Swapped::after_rename(kept.clone(), Err(io::Error::other("no sync")));

        assert_eq!(swapped.kept, kept);
        assert_eq!(swapped.unsynced.as_deref(), Some("no sync"));
        let synced = Swapped::after_rename(kept, Ok(()));
        assert_eq!(synced.unsynced, None);
    }
}
