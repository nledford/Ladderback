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
//!    new log is renamed over the old one. Both new files take the old
//!    log's permissions.
//!
//! The old log is never written until the rename, which replaces it whole,
//! so a copy that stops at any step leaves it usable: the next one removes
//! the work files and copies it again. A step that fails does the same now
//! and says why with [`OpenError::BaselineCopy`].

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use sqlx::migrate::Migrator;
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
}

/// Rows of one table the copy dropped as junk (decision 165): a removed
/// machine's current state, or a second report of an operation's happening.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub table: String,
    pub rows: u64,
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
        .map_err(|error| failed(last, format!("a work file can't be removed: {error}")))?;
    match work.run().await {
        Ok((dropped, kept)) => Ok(Some(BaselineCopy {
            from_version: last,
            kept,
            dropped,
        })),
        Err(detail) => {
            // Best effort: the next start removes what is left all the same.
            let _ = work.clear();
            Err(failed(last, detail))
        }
    }
}

/// A copy of the log at `version` that failed, saying why.
fn failed(version: i64, detail: String) -> OpenError {
    OpenError::BaselineCopy {
        version,
        detail: cap(detail),
    }
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

    /// Steps 2 to 5, each failure in the database's or the operating
    /// system's words; the old log is unchanged by all but the last rename.
    async fn run(&self) -> Result<(Vec<Dropped>, PathBuf), String> {
        snapshot(&self.log, &self.kept_new).await?;
        copy_file(self.kept_new.clone(), self.source.clone()).await?;
        migrate_old(&self.source).await?;
        let dropped = build(&self.new, &self.source).await?;
        remove_if_there(&self.source).map_err(|error| error.to_string())?;
        let kept = self.swap().map_err(|error| error.to_string())?;
        Ok((dropped, kept))
    }

    /// Step 5: the new log in the old one's place, the old one kept, at
    /// the path it answers.
    fn swap(&self) -> io::Result<PathBuf> {
        let permissions = fs::metadata(&self.log)?.permissions();
        for file in [&self.kept_new, &self.new] {
            fs::set_permissions(file, permissions.clone())?;
            File::open(file)?.sync_all()?;
        }
        let kept = self.keep()?;
        // Empty since the checkpoint: a crash right after leaves the old
        // log whole.
        for suffix in ["-wal", "-shm"] {
            remove_if_there(&beside(&self.log, suffix))?;
        }
        // The kept copy's name is durable before the old log's goes.
        sync_dir(self.folder())?;
        fs::rename(&self.new, &self.log)?;
        sync_dir(self.folder())?;
        Ok(kept)
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

async fn copy_file(from: PathBuf, to: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || fs::copy(from, to))
        .await
        .map_err(|error| error.to_string())?
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// A path as SQLite takes it in a statement.
fn utf8(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

/// One connection to `path`, as the store's but for WAL: foreign keys on.
/// A pool of one rather than a bare connection: a migrator run on a bare
/// connection makes the future not `Send` to the compiler.
async fn connect(path: &Path, create: bool) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create)
        .busy_timeout(BUSY_TIMEOUT);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| error.to_string())
}

/// Closes `pool` after `work`, whatever `work` did, and answers `work`'s.
async fn closing<T>(pool: SqlitePool, work: Result<T, String>) -> Result<T, String> {
    pool.close().await;
    work
}

/// Step 2: the log, its write-ahead log checkpointed into it first, copied
/// whole to `to`.
async fn snapshot(log: &Path, to: &Path) -> Result<(), String> {
    let pool = connect(log, false).await?;
    let work = async {
        let mut conn = pool.acquire().await.map_err(|error| error.to_string())?;
        let row = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut *conn)
            .await
            .map_err(|error| error.to_string())?;
        let busy: i64 = row.try_get(0).map_err(|error| error.to_string())?;
        if busy != 0 {
            return Err("another program has it open".to_owned());
        }
        sqlx::query("VACUUM INTO ?")
            .bind(utf8(to)?)
            .execute(&mut *conn)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    .await;
    closing(pool, work).await
}

/// Step 3: the old migrations the source lacks, as an earlier build's
/// start would have run them.
async fn migrate_old(source: &Path) -> Result<(), String> {
    let pool = connect(source, false).await?;
    let work = OLD_MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| format!("an old migration failed: {error}"));
    closing(pool, work).await
}

/// Step 4: the new log at `new`, in the baseline, with the rows of the
/// source at `source`.
async fn build(new: &Path, source: &Path) -> Result<Vec<Dropped>, String> {
    let pool = connect(new, true).await?;
    let work = async {
        MIGRATOR
            .run_to(BASELINE_VERSION, &pool)
            .await
            .map_err(|error| error.to_string())?;
        let mut conn = pool.acquire().await.map_err(|error| error.to_string())?;
        fill(&mut conn, source).await
    }
    .await;
    closing(pool, work).await
}

async fn fill(db: &mut SqliteConnection, source: &Path) -> Result<Vec<Dropped>, String> {
    let text = |error: sqlx::Error| error.to_string();
    sqlx::query("ATTACH DATABASE ? AS old")
        .bind(utf8(source)?)
        .execute(&mut *db)
        .await
        .map_err(text)?;
    let tables = table_names(db, "main").await?;
    let old_tables = table_names(db, "old").await?;
    if tables != old_tables {
        return Err(format!(
            "the old log's tables ({}) are not the baseline's ({})",
            old_tables.join(", "),
            tables.join(", ")
        ));
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
        return Err(format!(
            "the new log breaks {} of its foreign keys",
            broken.len()
        ));
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
        return Err(format!(
            "the new log fails its integrity check: {integrity}"
        ));
    }
    Ok(dropped)
}

/// The tables of `schema`, by name, but SQLite's and sqlx's own.
async fn table_names(db: &mut SqliteConnection, schema: &str) -> Result<Vec<String>, String> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT name FROM {schema}.sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name"
    )))
    .fetch_all(db)
    .await
    .map_err(|error| error.to_string())
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
) -> Result<Vec<Column>, String> {
    let text = |error: sqlx::Error| error.to_string();
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

async fn count(db: &mut SqliteConnection, sql: String) -> Result<u64, String> {
    let n: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
        .fetch_one(db)
        .await
        .map_err(|error| error.to_string())?;
    u64::try_from(n).map_err(|error| error.to_string())
}

/// Copies the rows of `table` the baseline keeps, with their rowids, and
/// answers how many it dropped as junk. The table's name is the
/// baseline's own, never outside text.
async fn copy_table(db: &mut SqliteConnection, table: &str) -> Result<u64, String> {
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
    let unmatched: Vec<&str> = missing.chain(extra).collect();
    if !unmatched.is_empty() {
        return Err(format!(
            "{table}'s columns are not the ones at migration 29: {}",
            unmatched.join(", ")
        ));
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
    .map_err(|error| format!("{table} holds a row the baseline refuses: {error}"))?;
    let copied = count(db, format!("SELECT COUNT(*) FROM main.\"{table}\"")).await?;
    if copied.checked_add(junk) != Some(source) {
        return Err(format!(
            "{table}: the old log holds {source} rows, but {copied} were copied and {junk} dropped"
        ));
    }
    Ok(junk)
}
