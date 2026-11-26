//! The one-off upgrade of a data folder an earlier release left (§16,
//! decision 170): only a process that holds the data folder makes it, so a
//! command that runs beside the daemon takes the lock for it, or changes
//! nothing while another Ladderback holds the folder.

use std::collections::BTreeMap;
use std::path::Path;

use sqlx::{ConnectOptions, Connection};

use super::*;

/// The ID an earlier build kept in `agent-id`.
const EARLIER_ID: &str = "0190a6e4-7a3b-7c3d-8e4f-0123456789ab";

/// A canary, never a real key (§13.9).
const KEY_CANARY: &str = "cacacacacacacacacacacacacacacacacacacacacacacacacacacacacacacaca\n";

/// Writes `text` to `name` in `data`, its owner's alone.
fn write_private(data: &Path, name: &str, text: &str) {
    std::fs::write(data.join(name), text).expect("the file is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(data.join(name), std::fs::Permissions::from_mode(0o600))
            .expect("the file is its owner's alone");
    }
}

/// The operation log 0.5.0 left, at migration 16, in WAL mode as every
/// earlier store kept it, with no connection left open.
async fn earlier_log(data: &Path) {
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(data.join("ladderback.db"))
        .create_if_missing(true)
        .connect()
        .await
        .expect("the old log is created");
    sqlx::raw_sql(include_str!(
        "../../../crates/adapters/store/tests/fixtures/at-0016.sql"
    ))
    .execute(&mut raw)
    .await
    .expect("the old log is filled");
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&mut raw)
        .await
        .expect("the old log is in WAL mode");
    raw.close().await.expect("the old log is closed");
}

/// A data folder as 0.5.0 left it: its log at migration 16, its machine ID
/// and key under their earlier names.
async fn earlier_folder(data: &Path) {
    std::fs::create_dir_all(data).expect("the data folder");
    write_private(data, "agent-id", &format!("{EARLIER_ID}\n"));
    write_private(data, "agent-key", KEY_CANARY);
    earlier_log(data).await;
}

/// Every file in `data`, with its bytes.
fn contents(data: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(data)
        .expect("the data folder lists")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().into_string().expect("a UTF-8 name");
            (name, std::fs::read(entry.path()).expect("the file reads"))
        })
        .collect()
}

/// An earlier release's folder while another Ladderback holds it, 0.5.0's
/// daemon not yet stopped: a command beside it is refused, saying that
/// Ladderback is running, and every file is left byte for byte as it was,
/// whether the key files or only the log are from before.
#[tokio::test]
async fn a_command_beside_a_ladderback_that_holds_an_earlier_folder_changes_nothing() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let whole = scratch.path().join("whole");
    earlier_folder(&whole).await;
    let log_only = scratch.path().join("log-only");
    std::fs::create_dir_all(&log_only).expect("the data folder");
    write_private(&log_only, "machine-id", &format!("{EARLIER_ID}\n"));
    earlier_log(&log_only).await;

    for data in [whole, log_only] {
        let held = DataDirLock::acquire(&data).expect("the folder is free");
        let before = contents(&data);

        let refused = super::super::upgrade::unheld(&data, super::super::upgrade::Scope::Folder)
            .await
            .expect_err("the upgrade is refused while the folder is held");

        assert_eq!(refused.message, msg::CLI_UPGRADE_LADDERBACK_RUNNING);
        assert_eq!(
            contents(&data),
            before,
            "{}: every file is left as it was",
            data.display()
        );
        drop(held);
    }
}

/// An earlier release's folder nobody holds: a command beside no daemon
/// upgrades it itself, keeping the machine's ID and key and the old log,
/// and lets go of the folder.
#[tokio::test]
async fn a_command_on_an_earlier_folder_nobody_holds_upgrades_it_and_lets_it_go() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let data = scratch.path().join("data");
    earlier_folder(&data).await;

    super::super::upgrade::unheld(&data, super::super::upgrade::Scope::Folder)
        .await
        .map_err(|refused| refused.message)
        .expect("the folder is upgraded");

    let id = agent_id::load(&data)
        .expect("the ID reads")
        .expect("the machine has its ID");
    assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
    assert_eq!(
        std::fs::read_to_string(data.join("machine-key")).expect("the key is kept"),
        KEY_CANARY
    );
    assert!(
        !store::SqliteStore::needs_copy(&data.join("ladderback.db"))
            .await
            .expect("the log reads"),
        "the log is in the baseline"
    );
    assert!(data.join("ladderback.db.before-baseline").exists());
    drop(DataDirLock::acquire(&data).expect("the folder is let go of"));
}

/// A folder this version made, beside the daemon that holds it: a command
/// goes ahead, as before the upgrade existed.
#[tokio::test]
async fn a_command_on_a_current_folder_beside_the_daemon_goes_ahead() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let data = scratch.path().join("data");
    std::fs::create_dir_all(&data).expect("the data folder");
    agent_id::load_or_create(&data).expect("an ID");
    store::SqliteStore::open(&data.join("ladderback.db"))
        .await
        .expect("a log")
        .close()
        .await;
    let held = DataDirLock::acquire(&data).expect("the folder is free");

    super::super::upgrade::unheld(&data, super::super::upgrade::Scope::Folder)
        .await
        .map_err(|refused| refused.message)
        .expect("nothing to upgrade");
    drop(held);
}

/// An earlier key beside a `machine-key` of its own, the ID still under
/// its earlier name: which key is the machine's is the person's to say,
/// so the upgrade is refused in words of its own, naming both files, and
/// every file is left as it was.
#[tokio::test]
async fn an_earlier_key_beside_another_machine_key_is_refused_naming_both() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let data = scratch.path().join("data");
    earlier_folder(&data).await;
    write_private(&data, "machine-key", "0b0b\n");
    let before = contents(&data);

    let refused = super::super::upgrade::unheld(&data, super::super::upgrade::Scope::Folder)
        .await
        .expect_err("two keys are refused");

    assert_eq!(refused.message, msg::CLI_UPGRADE_TWO_KEYS);
    for (name, file) in [("earlier", "agent-key"), ("own", "machine-key")] {
        let named = data.join(file).display().to_string();
        assert!(
            refused
                .args
                .iter()
                .any(|(arg, value)| *arg == name && *value == MsgArg::Text(named.clone())),
            "{name} is {named}: {:?}",
            refused.args
        );
    }
    let mut after = contents(&data);
    after.remove(platform::data_dir_lock::LOCK_FILE);
    assert_eq!(after, before, "every file is left as it was");
}

/// A command that never opens the log, `hub-status` or `export-profile`,
/// upgrades only the key files: an earlier log the copy refuses, here one
/// whose migration was changed after it ran, neither refuses it nor is
/// copied, and is left byte for byte as it was. `pair`, which opens the
/// log, upgrades it too and is refused.
#[tokio::test]
async fn a_command_that_never_opens_the_log_upgrades_only_the_key_files() {
    use super::super::upgrade::{Scope, unheld};
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let data = scratch.path().join("data");
    earlier_folder(&data).await;
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(data.join("ladderback.db"))
        .connect()
        .await
        .expect("the old log opens");
    sqlx::query("UPDATE _sqlx_migrations SET checksum = zeroblob(48) WHERE version = 12")
        .execute(&mut raw)
        .await
        .expect("a migration changed after it ran");
    raw.close().await.expect("the old log is closed");
    let log = std::fs::read(data.join("ladderback.db")).expect("the log reads");

    unheld(&data, Scope::Keys)
        .await
        .map_err(|refused| refused.message)
        .expect("the key files are upgraded");

    assert!(data.join("machine-id").exists() && data.join("machine-key").exists());
    assert_eq!(
        std::fs::read(data.join("ladderback.db")).expect("the log reads"),
        log
    );
    assert!(!data.join("ladderback.db.before-baseline").exists());
    let refused = unheld(&data, Scope::Folder)
        .await
        .expect_err("a command that opens the log is refused");
    assert_eq!(refused.message, msg::CLI_STORE_OLD_SCHEMA);
}
