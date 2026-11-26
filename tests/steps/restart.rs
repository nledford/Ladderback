//! Steps for `features/execution/restart.feature`: the real `ladderback serve`
//! process, killed and started again on the same folders, and a second one
//! started on folders the first holds. What the daemon says on the wire after
//! its start is read once, with no waiting: recovery has finished before it
//! says it listens. A run the scheduler starts once the daemon is back (the
//! missed run of `features/execution/refused_run.feature`) is waited for.

use std::time::{Duration, Instant};

use api::MsgArg;
use cucumber::{given, then, when};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::{OperationId, PlanId, RepositoryId, SignedDuration, Timestamp, Uuid};
use l10n::Localizer;
use platform::SystemClock;
use store::SqliteStore;

use app::ports::{Clock, OperationStore, PlannedRun, ScheduleStore, Start};

use super::events_steps::{self, Events};
use super::operations_steps::{exchange_at, history_page, shows_status, start_backup};
use super::{Agent, blocking};

/// How long the history may take to list a run the schedule refused. The
/// runner stops a scenario at 120 s and leaves its daemon running, so a step
/// that waits as long fails as a stopped run; the refused run is written
/// within a second of the start, so half that is ample.
const REFUSED_RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// The ID of the backup a scenario left queued in the log while the daemon
/// was down: the daemon takes it up again once it is up, so the history may
/// list it as queued until then.
const LEFT_QUEUED: u128 = 0x0bac_0e1d;

/// When the seeded prune was queued, in seconds since the Unix epoch.
const SEEDED_QUEUED_AT: i64 = 1_700_000_000;

/// A backup of `plan` that is running, which the scenario then means by "the
/// operation".
#[given(expr = "the daemon is running a backup of the plan {string}")]
#[when(expr = "the daemon is running a backup of the plan {string}")]
async fn running_a_backup(agent: &mut Agent, plan: String) {
    let id = start_backup(agent, &plan);
    agent.ops.note_started(id.clone());
    shows_status(agent, &id, "running").await;
}

#[given("the log, while the daemon is down, shows a running prune of the repository")]
async fn a_prune_left_running(agent: &mut Agent) {
    // The repository's ID is the one the daemon identifies it by: the ID of
    // its config file.
    let hex = super::forget_steps::with_repository(agent, |repository| {
        repository.config().id.to_hex().as_str().to_owned()
    })
    .await;
    let repository = RepositoryId::parse(&hex).expect("the repository's ID");
    let id = OperationId::from_uuid(Uuid::from_u128(0x5eed));
    let queued = Timestamp::from_second(SEEDED_QUEUED_AT).expect("a time");
    let started = Timestamp::from_second(SEEDED_QUEUED_AT + 1).expect("a time");
    let mut prune = Operation::queue(
        id,
        OperationKind::Prune,
        Trigger::Manual,
        Target {
            plan: None,
            repository: Some(repository),
        },
        queued,
    );
    prune.start(started).expect("a queued operation starts");
    let path = Events::data_dir(agent).join("ladderback.db");
    let store = SqliteStore::open(&path)
        .await
        .expect("the operation log opens while the daemon is down");
    assert_eq!(
        store.try_start(&prune).await.expect("the prune is stored"),
        Start::Started,
        "the log held no other maintenance on the repository"
    );
    // Closed, so the file is quiet when the daemon opens it.
    store.close().await;
    agent
        .ops
        .note_started(id.as_uuid().hyphenated().to_string());
}

#[given(expr = "the log, while the daemon is down, shows a queued backup of the plan {string}")]
async fn a_backup_left_queued(agent: &mut Agent, plan: String) {
    let id = OperationId::from_uuid(Uuid::from_u128(LEFT_QUEUED));
    let queued = Timestamp::from_second(SEEDED_QUEUED_AT).expect("a time");
    let backup = Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Schedule,
        Target {
            plan: Some(PlanId::try_from(plan).expect("a plan ID")),
            repository: None,
        },
        queued,
    );
    let path = Events::data_dir(agent).join("ladderback.db");
    let store = SqliteStore::open(&path)
        .await
        .expect("the operation log opens while the daemon is down");
    store
        .save(&backup, None)
        .await
        .expect("the queued backup is stored");
    // Closed, so the file is quiet when the daemon opens it.
    store.close().await;
    agent
        .ops
        .note_started(id.as_uuid().hyphenated().to_string());
}

#[given("the daemon is killed")]
#[when("the daemon is killed")]
fn the_daemon_is_killed(agent: &mut Agent) {
    agent.events.kill();
}

#[when("Ladderback is started again on the same folders")]
async fn started_again(agent: &mut Agent) {
    events_steps::restart(agent).await;
}

#[when("a second Ladderback is started on the same folders")]
async fn a_second_is_started(agent: &mut Agent) {
    events_steps::start_second(agent).await;
}

#[then(expr = "the history, read once, shows the operation failed with the code {string}")]
async fn history_shows_failed(agent: &mut Agent, code: String) {
    let id = agent.ops.the_operation();
    let page = history_page(agent, "limit=50").await;
    let operation = page
        .get("operations")
        .and_then(serde_json::Value::as_array)
        .and_then(|operations| {
            operations
                .iter()
                .find(|op| op.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str()))
        })
        .unwrap_or_else(|| panic!("the history lists the operation {id}: {page}"));
    assert_eq!(
        operation.get("status").and_then(serde_json::Value::as_str),
        Some("failed"),
        "the operation's status: {operation}"
    );
    assert_eq!(
        operation
            .get("failure-code")
            .and_then(serde_json::Value::as_str),
        Some(code.as_str()),
        "the operation's failure code: {operation}"
    );
}

/// The snapshot the backup wrote, by ID, on its record in History, whatever
/// the operation's end (decision 92).
#[then("the history, read once, shows the snapshot the backup wrote")]
async fn history_shows_the_snapshot(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    let written = super::backup_steps::newest_snapshot(agent).await;
    let written = written.id.to_hex().as_str().to_owned();
    let page = history_page(agent, "limit=50").await;
    let operation = page
        .get("operations")
        .and_then(serde_json::Value::as_array)
        .and_then(|operations| {
            operations
                .iter()
                .find(|op| op.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str()))
        })
        .unwrap_or_else(|| panic!("the history lists the operation {id}: {page}"));
    assert_eq!(
        operation
            .get("result")
            .and_then(|result| result.get("snapshot"))
            .and_then(serde_json::Value::as_str),
        Some(written.as_str()),
        "the operation's snapshot: {operation}"
    );
}

#[then("a prune of the repository is accepted")]
async fn a_prune_is_accepted(agent: &mut Agent) {
    let addr = agent.events.addr();
    // The daemon identifies the repository first, which derives its key.
    let (status, body) = blocking(move || {
        exchange_at(
            addr,
            "POST",
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"prune"}"#,
        )
    })
    .await;
    assert_eq!(status, 202, "the prune is accepted: {body}");
}

#[then(
    "the second Ladderback is refused, naming the data folder, the process that holds it and how to stop the installed service for good"
)]
fn the_second_is_refused(agent: &mut Agent) {
    let holder = agent.events.pid();
    let data = Events::data_dir(agent);
    let (status, log) = events_steps::refusal(agent);
    assert!(!status.success(), "a refusal is a failure: {status}");
    // The refusal as the daemon's catalog words it for this folder and this
    // holder: bare digits could as well be a time in the log's own lines.
    let said = Localizer::new(&[]).message(
        events_steps::DATA_DIR_IN_USE,
        &[
            ("path", MsgArg::Text(data.display().to_string())),
            ("pid", MsgArg::Text(holder.to_string())),
        ],
    );
    assert!(
        log.contains(said.text()),
        "the refusal says {:?}:\n{log}",
        said.text()
    );
    // A killed service is started again by its manager (#1035): the refusal
    // names the commands that stop it for good, as the person types them.
    let stop = if cfg!(target_os = "macos") {
        "launchctl bootout gui/$(id -u)/io.github.nledford.ladderback"
    } else {
        "systemctl --user stop ladderback"
    };
    for command in [stop, "ladderback remove-service"] {
        assert!(
            log.contains(command),
            "the refusal names {command:?}:\n{log}"
        );
    }
}

/// The schedule kept `plan`'s next run, as the daemon keeps it while it is
/// off, for an hour before now: the next start finds it missed.
#[given(expr = "the schedule, while the daemon was down, kept the plan {string} due an hour ago")]
async fn a_run_kept_due(agent: &mut Agent, plan: String) {
    let run = PlannedRun {
        plan: PlanId::try_from(plan).expect("a plan ID"),
        at: SystemClock.now() - SignedDuration::from_hours(1),
    };
    let path = Events::data_dir(agent).join("ladderback.db");
    let store = SqliteStore::open(&path)
        .await
        .expect("the operation log opens while the daemon is down");
    store.plan_run(&run).await.expect("the planned run is kept");
    // Closed, so the file is quiet when the daemon opens it.
    store.close().await;
}

#[then(
    expr = "the history comes to list one backup of the plan {string}, failed with the code {string} and never started"
)]
async fn history_lists_one_failed_backup(agent: &mut Agent, plan: String, code: String) {
    let started = Instant::now();
    let backup = loop {
        let page = history_page(agent, "limit=50").await;
        let backups: Vec<serde_json::Value> = page
            .get("operations")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|op| {
                op.get("kind").and_then(serde_json::Value::as_str) == Some("backup")
                    && op.get("plan").and_then(serde_json::Value::as_str) == Some(plan.as_str())
            })
            .cloned()
            .collect();
        let left_queued = Uuid::from_u128(LEFT_QUEUED).hyphenated().to_string();
        let waiting = |op: &serde_json::Value| {
            op.get("status").and_then(serde_json::Value::as_str) == Some("queued")
                && op.get("id").and_then(serde_json::Value::as_str) == Some(left_queued.as_str())
        };
        if let [backup] = backups.as_slice()
            && !waiting(backup)
        {
            // Settle, then look once more: a second row written just after
            // the first would otherwise go unseen.
            tokio::time::sleep(Duration::from_millis(500)).await;
            let again = history_page(agent, "limit=50").await;
            let count = again
                .get("operations")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter(|op| {
                    op.get("kind").and_then(serde_json::Value::as_str) == Some("backup")
                        && op.get("plan").and_then(serde_json::Value::as_str) == Some(plan.as_str())
                })
                .count();
            assert_eq!(
                count, 1,
                "the history lists more than one backup of {plan}: {again}"
            );
            break backup.clone();
        }
        assert!(
            backups.is_empty() || matches!(backups.as_slice(), [one] if waiting(one)),
            "the history lists more than one backup of {plan}: {page}"
        );
        assert!(
            started.elapsed() < REFUSED_RUN_TIMEOUT,
            "no backup of {plan} came within {} s: {page}",
            REFUSED_RUN_TIMEOUT.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    // Kept once, failed: never queued first and then failed.
    let text = |key: &str| backup.get(key).and_then(serde_json::Value::as_str);
    assert_eq!(text("status"), Some("failed"), "the backup: {backup}");
    assert_eq!(
        text("failure-code"),
        Some(code.as_str()),
        "the backup: {backup}"
    );
    assert_eq!(
        backup.get("started-at"),
        Some(&serde_json::Value::Null),
        "the backup never started: {backup}"
    );
}

/// The operation log of the daemon's data folder, and the copy of its
/// bytes a step keeps to compare with.
fn operation_log(agent: &Agent) -> (std::path::PathBuf, std::path::PathBuf) {
    let log = Events::data_dir(agent).join("ladderback.db");
    (log, Events::root(agent).join("ladderback.db.before"))
}

/// The log a killed daemon left is replaced by the one
/// `crates/adapters/store/tests/fixtures/at-0029.sql` describes, the 29
/// migrations before the baseline schema applied, in WAL mode as the store
/// keeps it (decision 165), but with the checksum of migration 12 one no
/// release shipped: the store copies only a log whose migrations are those
/// a release shipped (decision 170).
#[given(
    "the data folder holds an operation log from before the baseline schema whose migrations no release shipped"
)]
async fn an_old_operation_log(agent: &mut Agent) {
    use sqlx::{ConnectOptions as _, Connection as _};

    let (log, kept) = operation_log(agent);
    for suffix in ["", "-wal", "-shm"] {
        let file = format!("{}{suffix}", log.display());
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("{file} can't be removed: {error}"),
        }
    }
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&log)
        .create_if_missing(true)
        .connect()
        .await
        .expect("the old operation log is created");
    sqlx::raw_sql(include_str!(
        "../../crates/adapters/store/tests/fixtures/at-0029.sql"
    ))
    .execute(&mut raw)
    .await
    .expect("the old operation log is filled");
    sqlx::query("UPDATE _sqlx_migrations SET checksum = zeroblob(48) WHERE version = 12")
        .execute(&mut raw)
        .await
        .expect("migration 12 is given a checksum no release shipped");
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&mut raw)
        .await
        .expect("the old operation log is in WAL mode");
    raw.close().await.expect("the old operation log is closed");
    std::fs::copy(&log, kept).expect("the old operation log is kept to compare");
}

/// The refusal as the daemon's catalog words it for this log, at its last
/// migration, and no script to run: the store copies a log it can copy
/// itself (decision 170).
#[then(
    "Ladderback refuses to start, saying the operation log is from an earlier version it can't copy"
)]
fn refuses_an_old_log(agent: &mut Agent) {
    let (log, _) = operation_log(agent);
    let (status, said) = events_steps::refusal(agent);
    assert!(!status.success(), "a refusal is a failure: {status}");
    let refusal = Localizer::new(&[]).message(
        l10n::msg::CLI_STORE_OLD_SCHEMA,
        &[
            ("path", MsgArg::Text(log.display().to_string())),
            ("version", MsgArg::Text("29".to_owned())),
        ],
    );
    assert!(
        said.contains(refusal.text()),
        "the refusal says {:?}:\n{said}",
        refusal.text()
    );
    assert!(
        !said.contains("copy-to-baseline"),
        "the refusal names no script to run:\n{said}"
    );
}

#[then("the operation log is left as it was")]
fn the_old_log_is_left(agent: &mut Agent) {
    let (log, kept) = operation_log(agent);
    assert!(
        std::fs::read(log).expect("the operation log is readable")
            == std::fs::read(kept).expect("the copy is readable"),
        "the operation log is left byte for byte as it was"
    );
}
