//! Steps for `features/execution/upgrade.feature`: a data folder as an
//! earlier Ladderback left it, its operation log at one of the old
//! migrations and its machine ID and key under their earlier names, started
//! under this build (§16, decision 170).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use api::MsgArg;
use cucumber::{given, then, when};
use l10n::Localizer;
use platform::data_dir_lock::DataDirLock;
use sqlx::{ConnectOptions as _, Connection as _};

use super::Agent;
use super::events_steps::Events;
use super::interop_steps;
use super::operations_steps::{history_page, shows_status, start_backup};

/// What the data folder held before the scenario gave its files their
/// earlier names.
#[derive(Debug, Default)]
pub(crate) struct UpgradeE2e {
    /// The machine ID the daemon kept, as its file held it.
    machine_id: Option<String>,
    /// The migration the earlier operation log was at.
    migration: Option<i64>,
    /// The data folder's lock, held as another Ladderback holds it.
    holder: Option<DataDirLock>,
    /// Every file of the data folder and its bytes, once it was held.
    held_files: Option<BTreeMap<String, Vec<u8>>>,
}

/// Every file in `data`, with its bytes.
fn files_of(data: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(data)
        .expect("the data folder lists")
        .map(|entry| entry.expect("an entry of the data folder").path())
        .filter(|path| path.is_file())
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let bytes = fs::read(&path).expect("the file reads");
            (name.expect("a file has a name"), bytes)
        })
        .collect()
}

/// A canary, never a real key (§13.9): 64 hex digits, as a machine key's
/// seed is kept.
const KEY_CANARY: &str = "cacacacacacacacacacacacacacacacacacacacacacacacacacacacacacacaca\n";

/// The operations the fixtures' earlier version logged, in the history.
const EARLIER_OPERATIONS: [&str; 4] = [
    "00000000-0000-0000-0000-000000000001",
    "00000000-0000-0000-0000-000000000002",
    "00000000-0000-0000-0000-000000000003",
    "00000000-0000-0000-0000-000000000004",
];

/// The fixture of the operation log the old migrations left at `migration`.
fn earlier_log(migration: i64) -> &'static str {
    match migration {
        16 => include_str!("../../crates/adapters/store/tests/fixtures/at-0016.sql"),
        28 => include_str!("../../crates/adapters/store/tests/fixtures/at-0028.sql"),
        29 => include_str!("../../crates/adapters/store/tests/fixtures/at-0029.sql"),
        other => panic!("no fixture of an operation log at migration {other}"),
    }
}

/// `file` in the data folder, removed when there.
fn remove_if_there(file: &Path) {
    match fs::remove_file(file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("{} can't be removed: {error}", file.display()),
    }
}

/// Writes `text` to `path`, readable and writable by its owner only, as
/// every key file is.
fn write_private(path: &Path, text: &str) {
    fs::write(path, text).expect("the key file is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("the key file is its owner's alone");
    }
}

#[given(
    expr = "the data folder is as an earlier Ladderback left it: its operation log at migration {int}, its machine ID and key under their earlier names"
)]
async fn an_earlier_data_folder(agent: &mut Agent, migration: i64) {
    let data = Events::data_dir(agent);
    let id = fs::read_to_string(data.join("machine-id")).expect("the daemon kept a machine ID");
    fs::rename(data.join("machine-id"), data.join("agent-id")).expect("the ID's earlier name");
    remove_if_there(&data.join("machine-key"));
    write_private(&data.join("agent-key"), KEY_CANARY);
    agent.upgrade.machine_id = Some(id.trim().to_owned());
    agent.upgrade.migration = Some(migration);

    let log = data.join("ladderback.db");
    for suffix in ["", "-wal", "-shm"] {
        remove_if_there(&PathBuf::from(format!("{}{suffix}", log.display())));
    }
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&log)
        .create_if_missing(true)
        .connect()
        .await
        .expect("the earlier operation log is created");
    sqlx::raw_sql(earlier_log(migration))
        .execute(&mut raw)
        .await
        .expect("the earlier operation log is filled");
    // In WAL mode, as every earlier store kept it.
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&mut raw)
        .await
        .expect("the earlier operation log is in WAL mode");
    raw.close()
        .await
        .expect("the earlier operation log is closed");
}

#[then("the machine keeps the machine ID and key it had under their earlier names")]
fn keeps_its_id_and_key(agent: &mut Agent) {
    let data = Events::data_dir(agent);
    let id = fs::read_to_string(data.join("machine-id")).expect("the machine ID is kept");
    assert_eq!(
        Some(id.trim()),
        agent.upgrade.machine_id.as_deref(),
        "the machine ID is the one kept as the agent ID"
    );
    let key = fs::read_to_string(data.join("machine-key")).expect("the machine key is kept");
    assert!(key == KEY_CANARY, "the machine key is the agent key it was");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(data.join("machine-key"))
            .expect("the machine key's metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the machine key is its owner's alone");
    }
    for earlier in ["agent-id", "agent-key"] {
        assert!(
            !data.join(earlier).exists(),
            "{earlier} is gone: its file is the machine's now"
        );
    }
}

#[then("the history shows the operations the earlier version logged")]
async fn history_shows_the_earlier_operations(agent: &mut Agent) {
    let page = history_page(agent, "limit=50").await;
    let listed: Vec<&str> = page
        .get("operations")
        .and_then(serde_json::Value::as_array)
        .expect("the page lists operations")
        .iter()
        .filter_map(|op| op.get("id").and_then(serde_json::Value::as_str))
        .collect();
    for id in EARLIER_OPERATIONS {
        assert!(listed.contains(&id), "the history lists {id}: {listed:?}");
    }
}

#[then("the earlier operation log is kept beside the new one")]
async fn the_earlier_log_is_kept(agent: &mut Agent) {
    let kept = Events::data_dir(agent).join("ladderback.db.before-baseline");
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&kept)
        .read_only(true)
        .connect()
        .await
        .expect("the earlier operation log is kept");
    let last: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&mut raw)
        .await
        .expect("the kept log records its migrations");
    raw.close().await.expect("the kept log is closed");
    assert_eq!(
        Some(last),
        agent.upgrade.migration,
        "the kept log is the earlier one, as it was"
    );
}

#[then(expr = "a backup of the plan {string} the daemon runs now is tagged with that machine ID")]
async fn a_backup_is_tagged_with_the_id(agent: &mut Agent, plan: String) {
    let id = start_backup(agent, &plan);
    shows_status(agent, &id, "succeeded").await;
    let snapshot = super::backup_steps::newest_snapshot(agent).await;
    let tag = format!(
        "ladderback-machine:{}",
        agent
            .upgrade
            .machine_id
            .as_deref()
            .expect("the machine ID was kept")
    );
    assert!(
        snapshot.tags.iter().any(|each| each.as_str() == tag),
        "the snapshot is tagged {tag}: {:?}",
        snapshot.tags
    );
}

#[given("another Ladderback still holds that data folder")]
fn another_ladderback_holds_it(agent: &mut Agent) {
    let data = Events::data_dir(agent);
    let holder = DataDirLock::acquire(&data).expect("the data folder is free");
    agent.upgrade.holder = Some(holder);
    agent.upgrade.held_files = Some(files_of(&data));
}

#[when(expr = "the owner runs {string} on that data folder")]
async fn runs_on_that_data_folder(agent: &mut Agent, command: String) {
    let data = Events::data_dir(agent);
    agent
        .interop
        .env
        .push(("LADDERBACK_DATA_DIR".to_owned(), data.display().to_string()));
    let args: Vec<&str> = command.split_whitespace().collect();
    interop_steps::run(agent, &args).await;
}

#[then("it is refused, saying Ladderback is running on the folder and must be stopped first")]
fn refused_while_held(agent: &mut Agent) {
    let data = Events::data_dir(agent);
    let ran = interop_steps::ran(agent);
    assert_eq!(ran.code, Some(1), "the command is refused: {}", ran.stderr);
    let refusal = Localizer::new(&[]).message(
        l10n::msg::CLI_UPGRADE_LADDERBACK_RUNNING,
        &[("path", MsgArg::Text(data.display().to_string()))],
    );
    assert!(
        ran.stderr.contains(refusal.text()),
        "the refusal says {:?}:\n{}",
        refusal.text(),
        ran.stderr
    );
}

#[then("every file in the data folder is as it was")]
fn every_file_is_as_it_was(agent: &mut Agent) {
    let data = Events::data_dir(agent);
    let before = agent
        .upgrade
        .held_files
        .as_ref()
        .expect("the folder was held");
    assert_eq!(
        &files_of(&data),
        before,
        "every file is left byte for byte as it was"
    );
}
