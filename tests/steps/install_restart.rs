//! Steps for `features/cli/install_restart.feature`: `scripts/install-restart.ts`
//! run by Node as `just install-restart` runs it, through
//! `interop_steps::run` (a scratch home and data folder, English, nothing of
//! the developer's environment), with the service manager's stand-ins alone
//! on the `PATH` (`service_steps`).
//!
//! `just install` is a stand-in that builds nothing: it copies the new
//! program's stand-in into `bin` under the `--root` it is given, or, given
//! none, over the installed program, as `cargo install` installs into
//! Cargo's bin folder. The installed program, which
//! `INSTALL_RESTART_LADDERBACK` names, starts as a stand-in for the build
//! from before. The new program is the real `ladderback` behind a stand-in
//! that answers `--version` with what the scenario chose: by default the
//! checkout's build, which `LADDERBACK_BUILD_COMMIT` fixes for the script
//! (scripts/build-identity.ts).
//! The daemon's address is a stand-in that answers `GET /api/v1/session`
//! alone, as the daemon answers a caller without a session. The operation
//! log is the real store's, migrated as the daemon migrates it and kept open
//! while the scenario runs, in a data folder whose lock file names the
//! service's process, as the daemon's does.
//!
//! `@needs-node`: the runner skips the feature where
//! `translations_steps::node_runs_typescript` fails.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

use super::interop_steps::{Ran, environment, run};
use super::service_steps::{NAME, managers};
use super::{Agent, LADDERBACK, blocking};

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
    /// What the script showed on the terminal it ran at, when it ran at one.
    terminal: Option<String>,
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

/// The folder of the stand-ins, and of the installed program.
fn installed(agent: &Agent) -> PathBuf {
    agent.scratch().join("installed")
}

/// The installed program, which `INSTALL_RESTART_LADDERBACK` names.
fn installed_program(agent: &Agent) -> PathBuf {
    installed(agent).join("ladderback")
}

/// A stand-in for the `build` of the program in `folder`. `--version` names
/// `version`; each call's first argument and the path it was called by go
/// to `calls`; `check-keyring` fails as the real program fails for an entry
/// it can't read when a scenario has written `keyring-fails`, or once when
/// it has written `keyring-fails-once`. Everything else goes to the real
/// program.
fn stand_in(folder: &Path, build: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # The {build} build.\n\
         d='{}'\n\
         printf '%s\\t%s\\n' \"$1\" \"$0\" >> \"$d/calls\"\n\
         if [ \"$1\" = --version ]; then exec /bin/cat \"$d/version\"; fi\n\
         if [ \"$1\" = check-keyring ] && [ -e \"$d/keyring-fails-once\" ]; then\n\
         /bin/rm -f \"$d/keyring-fails-once\"\n\
         echo 'ladderback: nas: could not read the keyring entry ladderback/nas (secret-keyring-timed-out)' >&2\n\
         exit 1\n\
         fi\n\
         if [ \"$1\" = check-keyring ] && [ -e \"$d/keyring-fails\" ]; then\n\
         echo 'ladderback: nas: could not read the keyring entry ladderback/nas (secret-keyring-unavailable)' >&2\n\
         exit 1\n\
         fi\n\
         exec '{LADDERBACK}' \"$@\"\n",
        folder.display()
    )
}

/// What the installed program holds: the build from before, or the new one.
fn installed_text(agent: &Agent) -> String {
    fs::read_to_string(installed_program(agent)).expect("an installed program")
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
        .expect("the new program's version");
    write_program(&folder.join("new"), &stand_in(&folder, "new"));
    write_program(&installed_program(agent), &stand_in(&folder, "earlier"));
    // `just install [args]`: the new program goes to `bin` under `--root`,
    // or over the installed program, unless a scenario has written
    // `build-fails`. With `stop-while-building`, the build first sends the
    // script SIGTERM, as `timeout` or a `kill` does, and goes on.
    write_program(
        &folder.join("just"),
        &format!(
            "#!/bin/sh\n\
             d='{}'\n\
             if [ -e \"$d/build-fails\" ]; then echo 'error: could not compile `ladderback`' >&2; exit 101; fi\n\
             if [ -e \"$d/stop-while-building\" ]; then kill -TERM \"$PPID\"; fi\n\
             to=\"$d\"\n\
             while [ $# -gt 0 ]; do\n\
             case \"$1\" in --root) to=\"$2/bin\"; shift ;; --root=*) to=\"${{1#--root=}}/bin\" ;; esac\n\
             shift\n\
             done\n\
             /bin/mkdir -p \"$to\" && /bin/cp \"$d/new\" \"$to/ladderback.new\" && /bin/mv \"$to/ladderback.new\" \"$to/ladderback\"\n",
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
            installed_program(agent).display().to_string(),
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

#[given(expr = "the new program says it is {string}")]
fn new_program_says(agent: &mut Agent, said: String) {
    fs::write(installed(agent).join("version"), format!("{said}\n"))
        .expect("the new program's version");
}

#[given("the new program can't read a keyring entry")]
fn keyring_fails(agent: &mut Agent) {
    fs::write(installed(agent).join("keyring-fails"), "")
        .expect("the stand-in is told to fail its keyring check");
}

#[given("the new program can't read a keyring entry until it checks again")]
fn keyring_fails_once(agent: &mut Agent) {
    fs::write(installed(agent).join("keyring-fails-once"), "")
        .expect("the stand-in is told to fail its first keyring check");
}

#[given("the new program's build fails")]
fn build_fails(agent: &mut Agent) {
    fs::write(installed(agent).join("build-fails"), "")
        .expect("the build's stand-in is told to fail");
}

#[given("install-restart is told to stop while it builds")]
fn stop_while_building(agent: &mut Agent) {
    fs::write(installed(agent).join("stop-while-building"), "")
        .expect("the build's stand-in is told to stop the script");
}

/// The commands the programs ran, each by its first argument and the path
/// it was called by, in the order they ran.
fn calls(agent: &Agent) -> Vec<(String, PathBuf)> {
    fs::read_to_string(installed(agent).join("calls"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let (call, by) = line.split_once('\t').unwrap_or((line, ""));
            (call.to_owned(), PathBuf::from(by))
        })
        .collect()
}

/// Where `calls` has the first `call`.
fn first(calls: &[(String, PathBuf)], call: &str) -> Option<usize> {
    calls.iter().position(|(ran, _)| ran == call)
}

#[then("the new program read the keyring entries")]
fn keyring_read(agent: &mut Agent) {
    let calls = calls(agent);
    assert!(
        first(&calls, "check-keyring").is_some(),
        "the programs ran {calls:?}"
    );
}

#[then(expr = "the new program read the keyring entries {int} time(s)")]
fn keyring_read_times(agent: &mut Agent, times: usize) {
    let calls = calls(agent);
    let read = calls
        .iter()
        .filter(|(call, _)| call == "check-keyring")
        .count();
    assert_eq!(read, times, "the programs ran {calls:?}");
}

#[then("the new program read the keyring entries before the service was restarted")]
fn keyring_read_before_restart(agent: &mut Agent) {
    let calls = calls(agent);
    let checked = first(&calls, "check-keyring");
    assert!(
        checked.is_some() && checked < first(&calls, "install-service"),
        "the programs ran {calls:?}"
    );
}

#[then("the new program never restarted the service")]
fn never_restarted(agent: &mut Agent) {
    let calls = calls(agent);
    assert!(
        first(&calls, "install-service").is_none(),
        "the programs ran {calls:?}"
    );
}

/// The new program named its build and read the keyring entries where it
/// was built: in a folder of its own in the installed program's folder,
/// not as the installed program.
#[then("the new program named its build and read the keyring entries before it was put in place")]
fn checked_where_staged(agent: &mut Agent) {
    let calls = calls(agent);
    let folder = installed(agent);
    let program = installed_program(agent);
    for check in ["--version", "check-keyring"] {
        let by = calls
            .iter()
            .find(|(call, _)| call == check)
            .map(|(_, by)| by);
        assert!(
            by.is_some_and(|by| by != &program && by.starts_with(&folder)),
            "{check} should run from a staging folder in {}: the programs ran {calls:?}",
            folder.display()
        );
    }
}

#[then("the service was restarted by the new program in place")]
fn restarted_in_place(agent: &mut Agent) {
    let calls = calls(agent);
    let program = installed_program(agent);
    assert!(
        calls
            .iter()
            .any(|(call, by)| call == "install-service" && by == &program),
        "the programs ran {calls:?}"
    );
}

#[then("the installed program is the new one")]
fn installed_is_new(agent: &mut Agent) {
    assert_eq!(installed_text(agent), stand_in(&installed(agent), "new"));
}

#[then("the installed program is still the one from before")]
fn installed_is_earlier(agent: &mut Agent) {
    assert_eq!(
        installed_text(agent),
        stand_in(&installed(agent), "earlier")
    );
}

/// The installed program's folder holds the stand-ins' files and nothing
/// else: no folder a build went into.
#[then("no staging folder is left")]
fn no_staging_folder(agent: &mut Agent) {
    let folders: Vec<PathBuf> = fs::read_dir(installed(agent))
        .expect("the installed program's folder")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.is_dir())
        .collect();
    assert!(folders.is_empty(), "left behind: {folders:?}");
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
        "INSERT INTO operations (id, machine_id, kind, trigger, status, queued_at) \
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

/// Runs the script as `run_script` does, but with a terminal as its
/// standard input and standard error, as a person runs it, and presses
/// Enter once when it asks for it. What it showed on the terminal is kept
/// apart from its standard output.
#[when("the owner runs install-restart at a terminal and presses Enter when asked")]
async fn runs_at_terminal(agent: &mut Agent) {
    agent.install_restart.process_before = managers(agent).process(NAME);
    let pty = match nix::pty::openpty(None, None) {
        Ok(pty) => pty,
        Err(error) => testkit::skip!("no pseudo-terminal can be opened here: {error}"),
    };
    let mut command = Command::new(node());
    command.arg(SCRIPT);
    environment(agent, &mut command);
    command
        .stdin(Stdio::from(
            pty.slave.try_clone().expect("the terminal's other end"),
        ))
        .stderr(Stdio::from(pty.slave))
        .stdout(Stdio::piped());
    let (ran, shown) = blocking(move || {
        use std::io::{Read, Write};
        use std::time::Duration;
        let mut child = spawn_gate::hold(|| command.spawn()).expect("the script runs");
        // The script and its children now hold the terminal's other end
        // alone, so the reads below end once they have all ended.
        drop(command);
        // Its standard output is read meanwhile, so a full pipe can't hold
        // it, and it is stopped if it outlasts the bound, so a script that
        // never ends fails the step rather than hanging it.
        let mut stdout = child.stdout.take().expect("its standard output");
        let printed = std::thread::spawn(move || {
            let mut printed = Vec::new();
            let _ = stdout.read_to_end(&mut printed);
            String::from_utf8_lossy_owned(printed)
        });
        let pid = nix::unistd::Pid::from_raw(i32::try_from(child.id()).expect("a process ID"));
        let (ended, end) = std::sync::mpsc::channel::<()>();
        let watch = std::thread::spawn(move || {
            let outlasted = end.recv_timeout(Duration::from_secs(60)).is_err();
            if outlasted {
                let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
            }
            outlasted
        });
        let mut screen = fs::File::from(pty.master);
        let (mut shown, mut pressed) = (String::new(), false);
        let mut chunk = [0_u8; 4096];
        loop {
            match screen.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    shown.push_str(&String::from_utf8_lossy(
                        chunk.get(..read).unwrap_or_default(),
                    ));
                }
            }
            if !pressed && shown.contains("press Enter") {
                screen.write_all(b"\n").expect("Enter is pressed");
                pressed = true;
            }
        }
        let status = child.wait().expect("the script ended");
        let _ = ended.send(());
        assert!(
            !watch.join().expect("the watch ends"),
            "the script didn't end within a minute: {shown:?}"
        );
        let ran = Ran {
            success: status.success(),
            code: status.code(),
            stdout: printed.join().expect("its output was read"),
            stderr: shown.clone(),
        };
        (ran, shown)
    })
    .await;
    agent.interop.ran = Some(ran);
    agent.install_restart.terminal = Some(shown);
}

#[then(expr = "the terminal said {string}")]
fn terminal_said(agent: &mut Agent, said: String) {
    let Some(shown) = &agent.install_restart.terminal else {
        testkit::skip!("no pseudo-terminal could be opened");
    };
    assert!(shown.contains(&said), "the terminal showed {shown:?}");
}

/// Runs the script as `run_script` does, with a minute's wait, and sends it
/// SIGINT, as Ctrl-C in its terminal does, once it says it is waiting.
#[when("the owner stops install-restart while it waits")]
async fn stops_while_waiting(agent: &mut Agent) {
    agent.install_restart.process_before = managers(agent).process(NAME);
    let mut command = Command::new(node());
    command.args([SCRIPT, "--wait", "1"]);
    environment(agent, &mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let ran = blocking(move || {
        let mut child = spawn_gate::hold(|| command.spawn()).expect("the script runs");
        let mut said = String::new();
        let mut lines = BufReader::new(child.stdout.take().expect("its output")).lines();
        for line in lines.by_ref() {
            let line = line.expect("a line of its output");
            said.push_str(&line);
            said.push('\n');
            if line.contains("waiting for") {
                let pid = i32::try_from(child.id()).expect("a process ID fits a pid_t");
                nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGINT,
                )
                .expect("the script is sent SIGINT");
                break;
            }
        }
        for line in lines {
            said.push_str(&line.expect("a line of its output"));
            said.push('\n');
        }
        let output = child.wait_with_output().expect("the script ended");
        Ran {
            success: output.status.success(),
            code: output.status.code(),
            stdout: said,
            stderr: String::from_utf8_lossy_owned(output.stderr),
        }
    })
    .await;
    assert!(
        ran.stdout.contains("waiting for"),
        "the script never waited: {}{}",
        ran.stdout,
        ran.stderr
    );
    agent.interop.ran = Some(ran);
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
