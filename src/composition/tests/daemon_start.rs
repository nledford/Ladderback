use super::support::*;
use super::*;

/// A disk that never answers, as a mount whose server is down.
struct Stalled;

#[async_trait]
impl NormalizeSources for Stalled {
    async fn normalized(
        &self,
        _: &[domain::planning::Source],
    ) -> Result<Vec<domain::planning::Source>, app::ports::SourcesError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn a_look_at_the_sources_that_never_answers_is_given_up() {
    // One look to walk the sources, then one per source for the holder.
    let limit = std::time::Duration::from_millis(50);
    let mut plan = Plan::new(
        domain::PlanId::parse("home").expect("an ID"),
        domain::RepositoryAlias::parse("nas").expect("an alias"),
        ["/home/user", "/srv/disk"]
            .map(|source| domain::planning::Source::parse(source).expect("a source"))
            .to_vec(),
    );
    plan.filter.globs = vec!["!docker/app/cache".to_owned()];
    let said = tokio::time::timeout(
        limit * 3 + std::time::Duration::from_millis(200),
        never_matching_glob_warnings(&plan, &WithinLimit(Stalled, limit)),
    )
    .await
    .expect("the look is given up");
    assert_eq!(
        said,
        [glob_never_matches("home", "!docker/app/cache", None)]
    );
}

#[tokio::test]
async fn the_daemon_starts_with_no_configuration() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    // An agent that was never configured has nothing to do, and starts.
    let state = daemon_at(Some(config), Some(data), None).await;
    assert!(
        state.is_ok(),
        "an empty configuration serves: {:?}",
        state.err().map(|refused| refused.message)
    );
}

#[cfg(unix)]
#[test]
fn the_workers_cache_folder_is_made_for_the_daemons_user_alone() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = tempfile::tempdir().expect("a scratch dir");
    let dir = scratch.path().join("cache").join("ladderback");
    assert_eq!(workers_cache(Some(dir.clone())), Some(dir.clone()));
    let mode = std::fs::metadata(&dir)
        .expect("the folder is made")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "{mode:o}");
}

#[test]
fn a_cache_folder_that_cannot_be_made_is_said_once_and_left_out() {
    captured_log::capture();
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let file = scratch.path().join("a-file");
    std::fs::write(&file, b"").expect("the file in the way");
    let dir = file.join("cache");
    assert_eq!(workers_cache(Some(dir.clone())), None);
    let shown = dir.display().to_string();
    let said: Vec<_> = captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains(&shown))
        .collect();
    let [(level, line)] = said.as_slice() else {
        panic!("one line names the folder: {said:?}");
    };
    assert_eq!(*level, log::Level::Warn);
    assert!(line.contains("without a cache"), "{line}");
}

/// A folder that is there but can't be written to (another user's, or
/// read-only) is found once at start, not by every job.
#[cfg(unix)]
#[test]
fn a_cache_folder_that_cannot_be_written_to_is_said_once_with_the_reason_and_left_out() {
    use std::os::unix::fs::PermissionsExt;

    captured_log::capture();
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let dir = scratch.path().join("ladderback");
    std::fs::create_dir(&dir).expect("the cache folder");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500))
        .expect("a read-only cache folder");
    assert_eq!(workers_cache(Some(dir.clone())), None);
    let shown = dir.display().to_string();
    let said: Vec<_> = captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains(&shown))
        .collect();
    let [(level, line)] = said.as_slice() else {
        panic!("one line names the folder: {said:?}");
    };
    assert_eq!(*level, log::Level::Warn);
    assert!(line.contains("can't be written to"), "{line}");
    // EACCES, the OS's reason, as the OS words it.
    let reason = std::io::Error::from_raw_os_error(13).to_string();
    assert!(line.contains(&reason), "{line}");
}

/// A secret read from a file other users can read is said in the
/// daemon's log, by the file's path masked of the secrets read with it
/// and its mode, never the secret (#386, I4).
#[cfg(unix)]
#[tokio::test]
async fn a_secret_file_others_can_read_is_logged_by_its_masked_path_and_mode() {
    use std::os::unix::fs::PermissionsExt;

    captured_log::capture();
    let scratch = tempfile::tempdir().expect("a scratch dir");
    // The folder is named as the secret is, by chance.
    let folder = scratch.path().join("demo-secret-value");
    std::fs::create_dir(&folder).expect("the folder");
    let path = folder.join("nas-password");
    std::fs::write(&path, b"demo-secret-value\n").expect("the password file");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("the file's mode");
    let scratch_path = scratch.path().to_str().expect("a scratch path is text");
    let resolved = ResolveRepository::new(Arc::new(SystemSecrets::default()))
        .telling(Arc::new(log_secret_warning))
        .resolve(&demo_settings(SecretRef::File {
            path: path.to_str().expect("a scratch path is text").to_owned(),
            read: FileRead::FirstLine,
        }))
        .await
        .expect("the file is read");
    assert_eq!(
        resolved.access.password,
        SecretValue::new("demo-secret-value")
    );
    let shown = format!("{scratch_path}/***/nas-password");
    let said: Vec<_> = captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains(&shown))
        .collect();
    let [(level, line)] = said.as_slice() else {
        panic!("one line names the file: {said:?}");
    };
    assert_eq!(*level, log::Level::Warn);
    assert!(line.contains("SecretFileReadable"), "{line}");
    assert!(line.contains("0644"), "{line}");
    assert!(
        !captured_log::lines()
            .iter()
            .any(|(_, line)| line.contains("demo-secret-value")),
        "the secret is in the log"
    );
}

#[test]
fn without_a_cache_folder_the_workers_run_without_one() {
    captured_log::capture();
    assert_eq!(workers_cache(None), None);
    assert!(
        captured_log::lines().iter().any(
            |(level, line)| *level == log::Level::Warn && line.contains("LADDERBACK_CACHE_DIR")
        ),
        "the log says how to give the workers one"
    );
}

#[tokio::test]
async fn the_daemon_ends_what_a_crash_left_unfinished_before_it_hands_over_its_state() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    // The log an earlier run left behind: a backup that was running when
    // that run stopped.
    let earlier = SqliteStore::open(&database).await.expect("the log opens");
    let id = OperationId::from_uuid(Uuid::from_u128(1));
    earlier
        .save(&running_backup(id), None)
        .await
        .expect("the running backup is saved");
    earlier.close().await;

    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the daemon starts");
    drop(daemon);

    let log = SqliteStore::open(&database)
        .await
        .expect("the log opens again");
    let detail = log
        .get(id)
        .await
        .expect("the log reads")
        .expect("the backup is still in the log");
    assert_eq!(
        detail.record.status,
        OperationStatus::Failed {
            reason: FailureReason::DaemonStopped
        }
    );
}

/// The daemon listens before it has handed the backups queued before the
/// start back to the queue (decision 73): a start by hand in between, of a
/// plan with one, is refused as if that backup waited already, so the plan
/// never has two runs waiting (decision 75, point 4; #1140).
#[tokio::test]
async fn a_start_by_hand_before_its_plans_queued_backup_is_handed_back_is_refused() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    let earlier = SqliteStore::open(&database).await.expect("the log opens");
    let queued = domain::execution::Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(0x1140)),
        OperationKind::Backup,
        Trigger::Schedule,
        domain::execution::Target {
            plan: Some(domain::PlanId::parse("home").expect("a plan's name")),
            repository: None,
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    );
    earlier
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    earlier.close().await;

    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the daemon starts");
    let refused = daemon
        .state
        .operations
        .start_backup("home".to_owned(), Trigger::Manual)
        .await;

    assert!(
        matches!(refused, Err(http_adapter::StartError::RunWaiting)),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_proposal_a_stop_left_being_applied_is_failed_once_the_daemon_starts() {
    use app::ports::{ProposalStore, StoredProposal};
    use domain::planning::{ConfigRevision, ProposalId, ProposalStatus, Proposer, Rationale};

    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    // The store an earlier run left behind: a proposal it was applying
    // when it stopped.
    let earlier = SqliteStore::open(&database).await.expect("the store opens");
    let id = ProposalId::from_bytes(&std::array::from_fn(|_| 1));
    let now = domain::Timestamp::now();
    let proposal = StoredProposal {
        id: id.clone(),
        proposer: Proposer::NoToken,
        base: ConfigRevision::of(b""),
        changes: "[]".to_owned(),
        rationale: Rationale::parse("Nightly backups.".to_owned()).expect("a rationale"),
        status: ProposalStatus::Pending,
        created_at: now,
        expires_at: now + domain::SignedDuration::from_hours(48),
        decided_at: None,
        reasons: Vec::new(),
        approved_from: None,
        applied_as: None,
    };
    ProposalStore::add(&earlier, &proposal, 10, 10)
        .await
        .expect("the proposal is kept");
    assert!(
        ProposalStore::approve(&earlier, &id, now, None)
            .await
            .expect("the approval is kept"),
        "the proposal is being applied"
    );
    earlier.close().await;

    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the daemon starts");
    drop(daemon);

    let store = SqliteStore::open(&database)
        .await
        .expect("the store opens again");
    let kept = ProposalStore::get(&store, &id)
        .await
        .expect("the store reads")
        .expect("the proposal is still kept");
    assert_eq!(kept.status, ProposalStatus::Failed);
}

/// Leaves at `database` a log that shows `id` as a running backup and
/// refuses every change to its operations, as a failing disk would.
async fn a_log_that_refuses_changes(database: &std::path::Path, id: OperationId) {
    use sqlx::{ConnectOptions, Connection};
    let earlier = SqliteStore::open(database).await.expect("the log opens");
    earlier
        .save(&running_backup(id), None)
        .await
        .expect("the running backup is saved");
    earlier.close().await;
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .connect()
        .await
        .expect("a raw connection to the log");
    sqlx::query(
        "CREATE TRIGGER refuse_changes BEFORE UPDATE ON operations
             BEGIN SELECT RAISE(ABORT, 'refused'); END",
    )
    .execute(&mut raw)
    .await
    .expect("the log is made to refuse changes");
    raw.close().await.expect("the connection closes");
}

/// Fails unless the operation log at `database` is closed: SQLite removes
/// its `-wal` and `-shm` files when the last connection to it closes.
fn assert_closed(database: &std::path::Path) {
    for suffix in ["-wal", "-shm"] {
        let mut file = database.as_os_str().to_owned();
        file.push(suffix);
        let file = std::path::PathBuf::from(file);
        assert!(
            !file.exists(),
            "{} outlived the daemon: the operation log is still open",
            file.display()
        );
    }
}

#[tokio::test]
async fn a_log_that_cannot_be_recovered_stops_the_start_and_keeps_what_it_held() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    let id = OperationId::from_uuid(Uuid::from_u128(1));
    a_log_that_refuses_changes(&database, id).await;

    let refused = daemon_at(Some(config), Some(data), None)
        .await
        .err()
        .expect("a log that cannot be changed refuses the start");
    assert_eq!(refused.message, msg::CLI_CANNOT_RECOVER_OPERATIONS);

    let log = SqliteStore::open(&database)
        .await
        .expect("the log opens again");
    let detail = log
        .get(id)
        .await
        .expect("the log reads")
        .expect("the backup is still in the log");
    assert_eq!(
        detail.record.status,
        OperationStatus::Running,
        "the log is as it was"
    );
}

/// A log the migrations before the baseline schema left (decision 165) is
/// refused by name, with its last migration, and nothing is written to it.
#[tokio::test]
async fn a_log_from_before_the_baseline_schema_is_refused_as_old() {
    use sqlx::{ConnectOptions, Connection};
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    std::fs::create_dir_all(&data).expect("the data folder");
    let database = data.join("ladderback.db");
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true)
        .connect()
        .await
        .expect("the old log is created");
    sqlx::raw_sql(include_str!(
        "../../../crates/adapters/store/tests/fixtures/at-0029.sql"
    ))
    .execute(&mut raw)
    .await
    .expect("the old log is filled");
    // In WAL mode, as the store keeps every log.
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&mut raw)
        .await
        .expect("the old log is in WAL mode");
    raw.close().await.expect("the old log is closed");
    let before = std::fs::read(&database).expect("the old log is read");

    let refused = daemon_at(Some(config), Some(data), None)
        .await
        .err()
        .expect("an old log refuses the start");
    assert_eq!(refused.message, msg::CLI_STORE_OLD_SCHEMA);
    assert!(
        refused
            .args
            .iter()
            .any(|(name, value)| *name == "version" && *value == MsgArg::Text("29".to_owned())),
        "the refusal names the last migration: {:?}",
        refused.args
    );
    assert_eq!(
        std::fs::read(&database).expect("the old log is read again"),
        before,
        "the old log is left as it was"
    );
}

#[tokio::test]
async fn a_start_refused_once_the_log_is_open_closes_the_log_before_it_returns() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    a_log_that_refuses_changes(&database, OperationId::from_uuid(Uuid::from_u128(1))).await;

    let refused = daemon_at(Some(config), Some(data), None)
        .await
        .err()
        .expect("a log that cannot be changed refuses the start");
    assert_eq!(refused.message, msg::CLI_CANNOT_RECOVER_OPERATIONS);
    assert_closed(&database);
}

#[tokio::test]
async fn a_queued_row_that_cannot_be_read_is_logged_by_its_code_alone() {
    use sqlx::{ConnectOptions, Connection};
    super::captured_log::capture();
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    let id = OperationId::from_uuid(Uuid::from_u128(0x274));
    let earlier = SqliteStore::open(&database).await.expect("the log opens");
    let queued = domain::execution::Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Manual,
        domain::execution::Target {
            plan: None,
            repository: None,
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    );
    earlier
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    earlier.close().await;
    // A value the row can't hold, which the store's own account of the
    // row repeats: it is the row's, so nobody has checked it (I4). It is
    // written past the schema's CHECK, as a later version could.
    let canary = "canary-7f3a-not-a-trigger";
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .connect()
        .await
        .expect("a raw connection to the log");
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut raw)
        .await
        .expect("the CHECKs are ignored");
    sqlx::query("UPDATE operations SET trigger = ?")
        .bind(canary)
        .execute(&mut raw)
        .await
        .expect("the row is changed");
    raw.close().await.expect("the connection closes");

    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("an unreadable queued row doesn't refuse the start");
    drop(daemon);

    let said: Vec<_> = super::captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains("in the operation log can't be read"))
        .collect();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains("a queued operation")
            && line.contains("stays queued")
            && line.contains("store-corrupt")),
        "the row is logged as queued, with the store's code: {said:?}"
    );
    assert!(
        said.iter().all(|(_, line)| !line.contains("running")),
        "a queued row isn't logged as a running one: {said:?}"
    );
    assert!(
        said.iter().all(|(_, line)| !line.contains(canary)),
        "the row's own text never reaches the log: {said:?}"
    );
}

#[tokio::test]
async fn a_running_row_that_cannot_be_read_is_logged_as_ended_by_its_code_alone() {
    use sqlx::{ConnectOptions, Connection};
    super::captured_log::capture();
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let database = data.join("ladderback.db");
    let earlier = SqliteStore::open(&database).await.expect("the log opens");
    earlier
        .save(
            &running_backup(OperationId::from_uuid(Uuid::from_u128(0x275))),
            None,
        )
        .await
        .expect("the running backup is saved");
    earlier.close().await;
    // An ID the row can't hold, which the store's own account of the row
    // repeats: it is the row's, so nobody has checked it (I4).
    let canary = "canary-7f3b-not-an-id";
    let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .connect()
        .await
        .expect("a raw connection to the log");
    sqlx::query("UPDATE operations SET id = ?")
        .bind(canary)
        .execute(&mut raw)
        .await
        .expect("the row is changed");
    raw.close().await.expect("the connection closes");

    let daemon = daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("an unreadable running row doesn't refuse the start");
    drop(daemon);

    let said: Vec<_> = super::captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains("in the operation log can't be read"))
        .collect();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains("a running operation")
            && line.contains("has ended as failed")
            && line.contains("store-corrupt")),
        "the row is logged as ended, with the store's code: {said:?}"
    );
    assert!(
        said.iter().all(|(_, line)| !line.contains("queued")),
        "a running row isn't logged as a queued one: {said:?}"
    );
    assert!(
        said.iter().all(|(_, line)| !line.contains(canary)),
        "the row's own text never reaches the log: {said:?}"
    );
}

#[tokio::test]
async fn a_close_that_ends_within_its_bound_is_waited_for_and_says_nothing() {
    super::captured_log::capture();
    let database = std::path::Path::new("/srv/demo/closes-in-time/ladderback.db");
    let (closed, mut close) = futures::channel::oneshot::channel::<()>();
    let closing = async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let _ = closed.send(());
    };
    close_within(closing, database, std::time::Duration::from_secs(5)).await;
    assert_eq!(
        close.try_recv(),
        Ok(Some(())),
        "the close ran to its end before close_within returned"
    );
    let said: Vec<_> = super::captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains("closes-in-time"))
        .collect();
    assert!(said.is_empty(), "a close in time logs nothing: {said:?}");
}

#[tokio::test]
async fn a_close_that_outlasts_its_bound_is_given_up_with_a_warning() {
    super::captured_log::capture();
    let database = std::path::Path::new("/srv/demo/never-closes/ladderback.db");
    let bound = std::time::Duration::from_millis(50);
    let began = std::time::Instant::now();
    // Bounded itself, so a close_within that waited for ever fails here
    // rather than at the test runner's limit.
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        close_within(std::future::pending(), database, bound),
    )
    .await
    .expect("close_within gives up once its bound has run out");
    assert!(
        began.elapsed() >= bound,
        "it waited the bound out first, not {:?}",
        began.elapsed()
    );
    let said: Vec<_> = super::captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains("/srv/demo/never-closes/ladderback.db"))
        .collect();
    assert!(
        said.len() == 1 && said.iter().all(|(level, _)| *level == log::Level::Warn),
        "one warning names the log it gave up on: {said:?}"
    );
}

#[test]
fn each_recording_is_logged_at_its_level_naming_only_the_alias_the_id_and_the_reason() {
    super::captured_log::capture();
    let id = "c".repeat(64);
    let parsed = || domain::RepositoryId::parse(&id).expect("a repository ID");
    let alias = |alias: &str| RepositoryAlias::parse(alias).expect("an alias");
    let cases = [
        (
            Recording::Recorded {
                alias: alias("logged-recorded"),
                id: parsed(),
            },
            log::Level::Info,
            format!(
                "recorded the repository ID {id} in the entry logged-recorded of the configuration: a job opened it"
            ),
        ),
        (
            Recording::NotRecorded {
                alias: alias("logged-changed"),
                id: parsed(),
                why: domain::planning::NotRecorded::Changed,
            },
            log::Level::Info,
            format!(
                "did not record the repository ID {id} in the entry logged-changed of the configuration (Changed); the next job that opens it tries again"
            ),
        ),
        (
            Recording::NotRecorded {
                alias: alias("logged-same"),
                id: parsed(),
                why: domain::planning::NotRecorded::SameRepository {
                    other: alias("logged-other"),
                },
            },
            log::Level::Warn,
            format!(
                "did not record the repository ID {id} in the entry logged-same of the configuration: the entry logged-other records it, so the two entries are one repository; remove one of them"
            ),
        ),
        (
            Recording::NotInPlace {
                alias: alias("logged-form"),
                id: parsed(),
                why: app::ports::NotInPlace::Form,
            },
            log::Level::Warn,
            format!(
                "did not record the repository ID {id} in the entry logged-form of the configuration: it can't be added without changing something else in the file (Form); the next job that opens it tries again"
            ),
        ),
        (
            Recording::Busy {
                alias: alias("logged-busy"),
                id: parsed(),
            },
            log::Level::Info,
            format!(
                "did not record the repository ID {id} in the entry logged-busy of the configuration: another change to it was being saved; the next job that opens it tries again"
            ),
        ),
        (
            Recording::Failed {
                alias: alias("logged-failed"),
                id: parsed(),
                error: ConfigError::Unavailable {
                    file: "/srv/demo/config.toml".to_owned(),
                    detail: "read-only file system".to_owned(),
                },
            },
            log::Level::Warn,
            format!(
                "did not record the repository ID {id} in the entry logged-failed of the configuration (Unavailable {{ file: \"/srv/demo/config.toml\", detail: \"read-only file system\" }}); the next job that opens it tries again"
            ),
        ),
    ];
    for (recording, level, line) in cases {
        let entry = match &recording {
            Recording::Recorded { alias, .. }
            | Recording::NotRecorded { alias, .. }
            | Recording::NotInPlace { alias, .. }
            | Recording::Busy { alias, .. }
            | Recording::Failed { alias, .. } => format!("entry {} ", alias.as_str()),
        };
        log_recording(recording);
        let said: Vec<_> = super::captured_log::lines()
            .into_iter()
            .chain(
                super::captured_log::infos()
                    .into_iter()
                    .map(|line| (log::Level::Info, line)),
            )
            .filter(|(_, said)| said.contains(&entry))
            .collect();
        assert_eq!(said, vec![(level, line)]);
    }
}

#[tokio::test]
async fn a_data_folder_another_process_holds_is_refused_before_its_log_is_opened() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    // Held as another process holds it, before any log exists.
    let held = DataDirLock::acquire(&data).expect("the folder is free");
    let refused = daemon_at(Some(config), Some(data.clone()), None)
        .await
        .err()
        .expect("a data folder another process holds refuses");
    assert_eq!(refused.message, DATA_DIR_IN_USE[0]);
    assert!(
        !data.join("ladderback.db").exists(),
        "a refused daemon never opens the log"
    );
    drop(held);
}

#[tokio::test]
async fn a_newer_schema_is_a_typed_refusal() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    std::fs::write(config.join("config.toml"), "schema = 999999\n").expect("the config file");
    let refused = daemon_at(Some(config), Some(data), None)
        .await
        .err()
        .expect("a newer schema refuses to start");
    assert_eq!(refused.message, msg::CLI_CONFIG_SCHEMA_NEWER);
    assert!(
        refused
            .args
            .iter()
            .any(|(name, value)| *name == "found" && *value == text(999_999u64))
    );
}

#[tokio::test]
async fn a_second_daemon_on_a_data_folder_is_refused_naming_the_first() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let first = daemon_at(Some(config.clone()), Some(data.clone()), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the first daemon starts");
    let refused = daemon_at(Some(config), Some(data), None)
        .await
        .err()
        .expect("a data folder another daemon holds refuses");
    assert_eq!(refused.message, DATA_DIR_IN_USE[0]);
    assert!(
        refused
            .args
            .iter()
            .any(|(name, value)| *name == "pid" && *value == text(std::process::id())),
        "the refusal names the process that holds the folder"
    );
    drop(first);
}

#[tokio::test]
async fn a_daemon_refused_the_data_folder_has_not_read_the_configuration() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let first = daemon_at(Some(config.clone()), Some(data.clone()), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the first daemon starts");
    // A configuration only a newer build reads: a daemon that read it
    // would refuse it for that, and one that migrated an older file would
    // rewrite what the first daemon runs on.
    std::fs::write(config.join("config.toml"), "schema = 999999\n").expect("the config file");
    let refused = daemon_at(Some(config), Some(data), None)
        .await
        .err()
        .expect("a data folder another daemon holds refuses");
    assert_eq!(refused.message, DATA_DIR_IN_USE[0]);
    drop(first);
}

#[tokio::test]
async fn a_data_folder_is_free_again_once_its_daemon_is_gone() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config, data) = own_folders(scratch.path());
    let first = daemon_at(Some(config.clone()), Some(data.clone()), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the first daemon starts");
    drop(first);
    let second = daemon_at(Some(config), Some(data), None).await;
    assert!(
        second.is_ok(),
        "a folder nobody holds is served: {:?}",
        second.err().map(|refused| refused.message)
    );
}

#[test]
fn a_lock_that_cannot_be_taken_is_refused_by_what_is_known_of_it() {
    let folder = std::path::Path::new("/srv/demo/data");
    // The installed service's manager starts a killed holder again (#1035):
    // the refusal names that manager's stop where there is one.
    let (named_id, unnamed_id) = if cfg!(target_os = "linux") {
        (
            msg::CLI_DATA_DIR_IN_USE_SYSTEMD,
            msg::CLI_DATA_DIR_IN_USE_UNNAMED_SYSTEMD,
        )
    } else if cfg!(target_os = "macos") {
        (
            msg::CLI_DATA_DIR_IN_USE_LAUNCHD,
            msg::CLI_DATA_DIR_IN_USE_UNNAMED_LAUNCHD,
        )
    } else {
        (msg::CLI_DATA_DIR_IN_USE, msg::CLI_DATA_DIR_IN_USE_UNNAMED)
    };
    let named = lock_refused(folder, LockError::InUse { holder: Some(4321) });
    assert_eq!(named.message, named_id);
    assert_eq!(
        named.args,
        vec![("path", text("/srv/demo/data")), ("pid", text(4321))]
    );
    let unnamed = lock_refused(folder, LockError::InUse { holder: None });
    assert_eq!(unnamed.message, unnamed_id);
    assert_eq!(unnamed.args, vec![("path", text("/srv/demo/data"))]);
    let unavailable = lock_refused(
        folder,
        LockError::Unavailable {
            detail: "Permission denied (os error 13)".to_owned(),
        },
    );
    assert_eq!(unavailable.message, msg::CLI_CANNOT_LOCK_DATA_DIR);
    assert_eq!(
        unavailable.args,
        vec![
            ("path", text("/srv/demo/data/ladderback.lock")),
            ("detail", text("Permission denied (os error 13)"))
        ]
    );
}

#[test]
fn a_log_that_cannot_be_recovered_is_refused_with_the_stores_own_words() {
    for error in [
        app::ports::StoreError::Unavailable {
            detail: "disk I/O error".to_owned(),
        },
        app::ports::StoreError::Corrupt {
            detail: "operation 5: unknown status `bogus`".to_owned(),
        },
    ] {
        let (app::ports::StoreError::Unavailable { detail }
        | app::ports::StoreError::Corrupt { detail }) = &error;
        let refused =
            recovery_refused(std::path::Path::new("/srv/demo/data/ladderback.db"), &error);
        assert_eq!(refused.message, msg::CLI_CANNOT_RECOVER_OPERATIONS);
        assert_eq!(
            refused.args,
            vec![
                ("path", text("/srv/demo/data/ladderback.db")),
                ("detail", text(detail))
            ]
        );
    }
}
