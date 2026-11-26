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

    let copy = super::super::upgrade::unheld(&data, super::super::upgrade::Scope::Folder)
        .await
        .map_err(|refused| refused.message)
        .expect("the folder is upgraded")
        .expect("the copy is answered, for the command to say");
    assert_eq!(copy.from_version, 16);
    assert_eq!(copy.kept, data.join("ladderback.db.before-baseline"));

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

/// Key files that can't be moved, here in a folder this process can't
/// write, are refused in words about the key files, never as an operation
/// log that can't be opened.
#[cfg(unix)]
#[tokio::test]
async fn key_files_that_cant_be_moved_are_refused_naming_the_move() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let data = scratch.path().join("data");
    earlier_folder(&data).await;
    let held = DataDirLock::acquire(&data).expect("the folder is free");
    let mode = |mode| std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(&data, mode(0o500)).expect("the folder is made read-only");

    let refused = super::super::upgrade::under(&held, &data).await;

    std::fs::set_permissions(&data, mode(0o700)).expect("the folder is writable again");
    let refused = refused.expect_err("the move is refused");
    assert_eq!(refused.message, msg::CLI_UPGRADE_KEY_MOVE_FAILED);
    assert!(data.join("agent-id").exists() && data.join("agent-key").exists());
}

/// A copy the baseline refuses a row of is said in words of its own, naming
/// the table, with only the database's own words as the detail.
#[test]
fn a_copy_refused_a_row_is_said_naming_the_table() {
    let log = Path::new("/data/ladderback.db");
    let refused = super::super::startup::log_refused(
        log,
        &store::OpenError::BaselineCopy {
            version: 29,
            cause: store::CopyFailure::RefusedRow {
                table: "operations".to_owned(),
                detail: "CHECK constraint failed: status".to_owned(),
            },
        },
    );

    assert_eq!(refused.message, msg::CLI_STORE_BASELINE_COPY_REFUSED_ROW);
    let args: BTreeMap<&str, String> = refused
        .args
        .iter()
        .map(|(name, value)| (*name, format!("{value:?}")))
        .collect();
    assert!(args["table"].contains("operations"), "{args:?}");
    assert!(
        args["detail"].contains("CHECK constraint failed"),
        "{args:?}"
    );
}

/// Only a copy that failed for a reason another start may not meet, the
/// files or the database, says the next start tries again; one that fails
/// the same on every start says the log is untouched and what to report.
#[test]
fn only_a_copy_that_may_succeed_later_says_the_next_start_tries_again() {
    use store::CopyFailure;
    let detail = || "the system's words".to_owned();
    let table = || "operations".to_owned();
    let retried = [
        CopyFailure::Files { detail: detail() },
        CopyFailure::Database { detail: detail() },
    ];
    let lasting = [
        CopyFailure::OldMigration { detail: detail() },
        CopyFailure::Tables {
            old: vec![table()],
            baseline: vec![table()],
        },
        CopyFailure::Columns {
            table: table(),
            unmatched: vec!["status".to_owned()],
        },
        CopyFailure::RefusedRow {
            table: table(),
            detail: detail(),
        },
        CopyFailure::Counts {
            table: table(),
            source: 3,
            copied: 2,
            dropped: 0,
        },
        CopyFailure::ForeignKeys { broken: 1 },
        CopyFailure::Integrity { detail: detail() },
        CopyFailure::NotUtf8 {
            path: "/data/ladderback.db.baseline-new".into(),
        },
    ];
    let said = |cause: CopyFailure| {
        let refused = super::super::startup::log_refused(
            Path::new("/data/ladderback.db"),
            &store::OpenError::BaselineCopy { version: 16, cause },
        );
        l10n::Localizer::new(&[])
            .message(refused.message, &refused.args)
            .text()
            .to_owned()
    };

    for cause in retried {
        let text = said(cause);
        assert!(text.contains("the next start tries again"), "{text}");
    }
    for cause in lasting {
        let text = said(cause);
        assert!(!text.contains("tries again"), "{text}");
        assert!(text.contains("It was left as it was"), "{text}");
        assert!(text.contains("report"), "{text}");
    }
}

/// A copy any command made is said to the person who ran it, through the
/// catalog: where the old log is kept, each table's rows dropped as junk,
/// and a folder that couldn't be synced after it, as a warning.
#[test]
fn a_finished_copy_is_said_with_where_the_old_log_is_kept_and_what_it_dropped() {
    let copy = store::BaselineCopy {
        from_version: 16,
        kept: "/data/ladderback.db.before-baseline".into(),
        dropped: vec![store::Dropped {
            table: "sync_reports".to_owned(),
            rows: 2,
        }],
        unsynced: Some("Input/output error".to_owned()),
    };
    let localizer = l10n::Localizer::new(&[]);

    let said: Vec<String> = super::super::upgrade::copy_said(&copy)
        .iter()
        .map(|line| {
            localizer
                .message(line.message, &line.args)
                .text()
                .to_owned()
        })
        .collect();

    assert_eq!(said.len(), 3, "{said:?}");
    assert!(
        said[0].contains("/data/ladderback.db.before-baseline"),
        "{said:?}"
    );
    assert!(said[0].contains("16"), "{said:?}");
    assert!(
        said[1].contains("sync_reports") && said[1].contains('2'),
        "{said:?}"
    );
    assert!(
        said[2].starts_with("warning:") && said[2].contains("Input/output error"),
        "{said:?}"
    );
    assert!(
        super::super::upgrade::copy_said(&store::BaselineCopy {
            dropped: Vec::new(),
            unsynced: None,
            ..copy
        })
        .len()
            == 1
    );
}
