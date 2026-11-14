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
use l10n::{Localizer, msg};
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

#[then("the second Ladderback is refused, naming the data folder and the process that holds it")]
fn the_second_is_refused(agent: &mut Agent) {
    let holder = agent.events.pid();
    let data = Events::data_dir(agent);
    let (status, log) = events_steps::refusal(agent);
    assert!(!status.success(), "a refusal is a failure: {status}");
    // The refusal as the daemon's catalog words it for this folder and this
    // holder: bare digits could as well be a time in the log's own lines.
    let said = Localizer::new(&[]).message(
        msg::CLI_DATA_DIR_IN_USE,
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
