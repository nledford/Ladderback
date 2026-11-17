//! Steps for `features/cli/install_restart.feature`: `scripts/install-restart.ts`
//! run by Node as `just install-restart` runs it, through
//! `interop_steps::run` (a scratch home and data folder, English, nothing of
//! the developer's environment), with the service manager's stand-ins alone
//! on the `PATH` (`service_steps`).
//!
//! `just install` is a stand-in that builds nothing. The installed program
//! is the real `ladderback` behind a stand-in that answers `--version` with
//! what the scenario chose: by default the checkout's build, which
//! `LADDERBACK_BUILD_COMMIT` fixes for the script (scripts/build-identity.ts).
//! The daemon's address is a stand-in that answers `GET /api/v1/session`
//! alone, as the daemon answers a caller without a session. The operation
//! log is the real store's, migrated as the daemon migrates it and kept open
//! while the scenario runs, in a data folder whose lock file names the
//! service's process, as the daemon's does.
//!
//! `@needs-node`: the runner skips the feature where
//! `translations_steps::node_runs_typescript` fails.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use app::ports::OperationStore;
use cucumber::{given, then, when};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::{OperationId, PlanId, Timestamp, Uuid};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection};
use store::SqliteStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::interop_steps::run;
use super::service_steps::{NAME, managers};
use super::{Agent, LADDERBACK};

/// The script under test, as the repository holds it.
const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/install-restart.ts");

/// The workspace's version, which the script reads from `Cargo.toml`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit the checkout's build names (`LADDERBACK_BUILD_COMMIT`).
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// The most of a request the address's stand-in reads.
const REQUEST_LIMIT: usize = 8 * 1024;

#[derive(Default)]
pub(super) struct InstallRestartE2e {
    /// The operation log, open as the service's daemon keeps it.
    store: Option<SqliteStore>,
    /// A connection that holds the operation log locked, while it lives.
    locker: Option<SqliteConnection>,
    /// What the address answers to `GET /api/v1/session`.
    session: Arc<Mutex<String>>,
    /// The service's process when the command ran.
    process_before: Option<u32>,
}

/// The checkout's build as `--version` and the script show it.
fn build() -> String {
    format!("{VERSION}+{}", COMMIT.get(..7).expect("a long commit"))
}

/// The answer of a daemon with sign-in on to a caller without a session: no
/// build.
fn signed_out() -> String {
    r#"{"sign-in":"signed-out"}"#.to_owned()
}

fn data_dir(agent: &Agent) -> PathBuf {
    agent.scratch().join("data")
}

fn database(agent: &Agent) -> PathBuf {
    data_dir(agent).join("ladderback.db")
}

fn installed(agent: &Agent) -> PathBuf {
    agent.scratch().join("installed")
}

/// Writes a runnable file. Written while no child starts: a child forked
/// meanwhile would hold the file open for writing, and running it would fail
/// with "text file busy".
fn write_program(path: &Path, text: &str) {
    spawn_gate::hold(|| {
        fs::write(path, text).expect("a stand-in is written");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("a stand-in is made runnable");
    });
}

fn service_process(agent: &Agent) -> u32 {
    managers(agent)
        .process(NAME)
        .expect("the service runs a process")
}

/// The data folder's lock file names `process`, as the daemon that holds the
/// folder writes it.
fn held_by(agent: &Agent, process: u32) {
    fs::write(
        data_dir(agent).join("ladderback.lock"),
        format!("{process}\n"),
    )
    .expect("the lock file");
}

#[given("the service's daemon keeps its operation log, with sign-in on")]
async fn daemon_keeps_its_log(agent: &mut Agent) {
    fs::create_dir_all(data_dir(agent)).expect("the data folder");
    let store = SqliteStore::open(&database(agent))
        .await
        .expect("the operation log opens");
    agent.install_restart.store = Some(store);
    held_by(agent, service_process(agent));

    *agent
        .install_restart
        .session
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = signed_out();
    let address = answer_sessions(Arc::clone(&agent.install_restart.session)).await;

    let folder = installed(agent);
    fs::create_dir_all(&folder).expect("the installed program's folder");
    fs::write(folder.join("version"), format!("ladderback {}\n", build()))
        .expect("the installed program's version");
    write_program(&folder.join("just"), "#!/bin/sh\nexit 0\n");
    // `--version` names `version`, and from its second call on
    // `version-later` when a scenario has written one. Each call's first
    // argument goes to `calls`. `check-keyring` fails as the real program
    // fails for an entry it can't read when a scenario has written
    // `keyring-fails`.
    write_program(
        &folder.join("ladderback"),
        &format!(
            "#!/bin/sh\n\
             d='{}'\n\
             echo \"$1\" >> \"$d/calls\"\n\
             if [ \"$1\" = --version ]; then\n\
             if [ -e \"$d/asked\" ] && [ -e \"$d/version-later\" ]; then exec /bin/cat \"$d/version-later\"; fi\n\
             : > \"$d/asked\"\n\
             exec /bin/cat \"$d/version\"\n\
             fi\n\
             if [ \"$1\" = check-keyring ] && [ -e \"$d/keyring-fails\" ]; then\n\
             echo 'ladderback: nas: could not read the keyring entry ladderback/nas (secret-keyring-unavailable)' >&2\n\
             exit 1\n\
             fi\n\
             exec '{LADDERBACK}' \"$@\"\n",
            folder.display()
        ),
    );
    for (name, value) in [
        (
            "INSTALL_RESTART_JUST",
            folder.join("just").display().to_string(),
        ),
        (
            "INSTALL_RESTART_LADDERBACK",
            folder.join("ladderback").display().to_string(),
        ),
        ("INSTALL_RESTART_ADDRESS", address),
        ("LADDERBACK_BUILD_COMMIT", COMMIT.to_owned()),
    ] {
        agent.interop.env.push((name.to_owned(), value));
    }
}

/// Serves `GET /api/v1/session` on loopback with what `session` holds, and
/// to anything else 401 with sign-in on, as the daemon answers a caller
/// without a session, or 404; the address it listens on.
async fn answer_sessions(session: Arc<Mutex<String>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let address = listener.local_addr().expect("an address").to_string();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let session = Arc::clone(&session);
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0_u8; 1024];
                while !request.windows(4).any(|end| end == b"\r\n\r\n")
                    && request.len() < REQUEST_LIMIT
                {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => {
                            request.extend_from_slice(chunk.get(..read).unwrap_or_default())
                        }
                    }
                }
                let body = session
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                let answer = if request.starts_with(b"GET /api/v1/session ") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else if body == signed_out() {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                };
                let _ = stream.write_all(answer.as_bytes()).await;
            });
        }
    });
    address
}

#[given("the daemon at the service's address has sign-in off")]
fn sign_in_off(agent: &mut Agent) {
    *agent
        .install_restart
        .session
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = format!(
        r#"{{"sign-in":"off","version":"{VERSION}","commit":"{COMMIT}","dirty":false,"release":false}}"#
    );
}

#[given("the service manager keeps the service's process when it restarts it")]
fn keeps_process(agent: &mut Agent) {
    managers(agent).keep_process_on_restart();
}

#[given(expr = "the installed program says it is {string}")]
fn installed_says(agent: &mut Agent, said: String) {
    fs::write(installed(agent).join("version"), format!("{said}\n"))
        .expect("the installed program's version");
}

/// The installed program's stand-in names `said` from its second
/// `--version` on, as when another checkout's `just install` writes over it
/// while install-restart waits.
#[given(
    expr = "another program, saying it is {string}, is installed once install-restart has checked the installed one"
)]
fn installed_over(agent: &mut Agent, said: String) {
    fs::write(installed(agent).join("version-later"), format!("{said}\n"))
        .expect("the later program's version");
}

#[given("the installed program can't read a keyring entry")]
fn keyring_fails(agent: &mut Agent) {
    fs::write(installed(agent).join("keyring-fails"), "")
        .expect("the stand-in is told to fail its keyring check");
}

/// The installed program's commands, by their first argument, in the order
/// they ran.
fn calls(agent: &Agent) -> Vec<String> {
    fs::read_to_string(installed(agent).join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[then("the installed program read the keyring entries before the service was restarted")]
fn keyring_read_before_restart(agent: &mut Agent) {
    let calls = calls(agent);
    let checked = calls.iter().position(|call| call == "check-keyring");
    let restarted = calls.iter().position(|call| call == "install-service");
    assert!(
        checked.is_some() && checked < restarted,
        "the installed program ran {calls:?}"
    );
}

#[then("the installed program never restarted the service")]
fn never_restarted(agent: &mut Agent) {
    let calls = calls(agent);
    assert!(
        !calls.iter().any(|call| call == "install-service"),
        "the installed program ran {calls:?}"
    );
}

#[given(expr = "the operation log shows a backup {word}")]
async fn log_shows_a_backup(agent: &mut Agent, status: String) {
    let queued = Timestamp::from_second(1_700_000_000).expect("a time");
    let mut backup = Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(0x0bac_0001)),
        OperationKind::Backup,
        Trigger::Schedule,
        Target {
            plan: Some(PlanId::try_from("documents".to_owned()).expect("a plan ID")),
            repository: None,
        },
        queued,
    );
    match status.as_str() {
        "queued" => {}
        "running" => backup.start(queued).expect("a queued backup starts"),
        other => panic!("no backup is {other}"),
    }
    agent
        .install_restart
        .store
        .as_ref()
        .expect("the operation log is open")
        .save(&backup, None)
        .await
        .expect("the backup is stored");
}

/// Closes the operation log and removes its files, as if it had never been.
async fn remove_log(agent: &mut Agent) {
    if let Some(store) = agent.install_restart.store.take() {
        store.close().await;
    }
    let file = database(agent);
    for suffix in ["", "-wal", "-shm"] {
        let mut name = file.clone().into_os_string();
        name.push(suffix);
        match fs::remove_file(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("the operation log's file is removed: {error}"),
        }
    }
}

/// A connection of its own to the operation log, made when there is none.
async fn connect(agent: &Agent) -> SqliteConnection {
    SqliteConnectOptions::new()
        .filename(database(agent))
        .create_if_missing(true)
        .connect()
        .await
        .expect("a connection to the operation log")
}

#[given("there is no operation log")]
async fn no_log(agent: &mut Agent) {
    remove_log(agent).await;
}

#[given("the operation log isn't a database")]
async fn log_not_a_database(agent: &mut Agent) {
    remove_log(agent).await;
    fs::write(
        database(agent),
        "Not a database: a text file of the same name, long enough to hold a header.\n",
    )
    .expect("the text file");
}

/// A log with no `operations` table.
#[given("the operation log comes from a version that keeps operations elsewhere")]
async fn log_without_operations(agent: &mut Agent) {
    remove_log(agent).await;
    let mut connection = connect(agent).await;
    sqlx::raw_sql("CREATE TABLE plans (id TEXT PRIMARY KEY NOT NULL)")
        .execute(&mut connection)
        .await
        .expect("a table of another name");
    connection.close().await.expect("the connection closes");
}

/// A log whose `operations` table has no `status` column.
#[given("the operation log comes from a version that keeps no status for an operation")]
async fn operations_without_status(agent: &mut Agent) {
    remove_log(agent).await;
    let mut connection = connect(agent).await;
    sqlx::raw_sql("CREATE TABLE operations (id TEXT PRIMARY KEY NOT NULL)")
        .execute(&mut connection)
        .await
        .expect("an operations table without a status");
    connection.close().await.expect("the connection closes");
}

#[given(expr = "the operation log holds an operation whose status is {string}")]
async fn log_holds_status(agent: &mut Agent, status: String) {
    let mut connection = connect(agent).await;
    sqlx::query(
        "INSERT INTO operations (id, agent_id, kind, trigger, status, queued_at) \
         VALUES ('01890000-0000-7000-8000-000000000001', 'local', 'backup', 'manual', ?, 0)",
    )
    .bind(status)
    .execute(&mut connection)
    .await
    .expect("the operation is stored");
    connection.close().await.expect("the connection closes");
}

#[given("another process holds the operation log locked")]
async fn log_locked(agent: &mut Agent) {
    // The daemon's own connections let a reader in; one in exclusive locking
    // mode that has written keeps every other process out until it closes.
    if let Some(store) = agent.install_restart.store.take() {
        store.close().await;
    }
    let mut connection = connect(agent).await;
    sqlx::raw_sql(
        "PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; \
         UPDATE operations SET warning_count = warning_count; COMMIT;",
    )
    .execute(&mut connection)
    .await
    .expect("the operation log is locked");
    agent.install_restart.locker = Some(connection);
}

#[given("another process holds the data folder")]
fn data_folder_held_elsewhere(agent: &mut Agent) {
    held_by(agent, service_process(agent) + 1000);
}

#[given("the service's process has ended")]
fn process_ended(agent: &mut Agent) {
    managers(agent).end(NAME);
}

#[when("the owner runs install-restart")]
async fn runs(agent: &mut Agent) {
    run_script(agent, &[]).await;
}

#[when(expr = "the owner runs install-restart with {string}")]
async fn runs_with(agent: &mut Agent, args: String) {
    let args: Vec<&str> = args.split_whitespace().collect();
    run_script(agent, &args).await;
}

/// Runs the script with Node, as `just install-restart` does.
async fn run_script(agent: &mut Agent, args: &[&str]) {
    agent.install_restart.process_before = managers(agent).process(NAME);
    let node = node();
    let mut all = vec![SCRIPT];
    all.extend_from_slice(args);
    agent.interop.program = Some(node);
    run(agent, &all).await;
    agent.interop.program = None;
}

/// The `node` on the test's own `PATH`: the one the build's web UI needs.
fn node() -> String {
    let path = std::env::var_os("PATH").expect("a PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join("node"))
        .find(|candidate| candidate.is_file())
        .expect("node is on the PATH")
        .display()
        .to_string()
}

#[then("the service runs a new process")]
fn runs_a_new_process(agent: &mut Agent) {
    let now = managers(agent).process(NAME);
    assert!(now.is_some(), "the service runs no process");
    assert_ne!(
        now, agent.install_restart.process_before,
        "the service runs the process it ran before"
    );
}

#[then("the service runs the same process")]
fn runs_the_same_process(agent: &mut Agent) {
    assert_eq!(
        managers(agent).process(NAME),
        agent.install_restart.process_before,
        "the service was restarted"
    );
}
