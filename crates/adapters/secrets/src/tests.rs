//! The app's secrets contract against the real adapter, and what the
//! contract can't state: process groups, limits, literal arguments, special
//! files and the OS keyring.
//!
//! A test whose code under test fails to stop or cut short a command must
//! fail, not hang the run: each call that waits on a limit, a stop or a pipe
//! has a bound of its own (`within`), a few times what the call takes.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app::contract::secrets::SecretsHarness;
use app::ports::{Resolved, SecretResolver};
use domain::secrets::{
    CommandLine, FileRead, KeyringEntry, MAX_SECRET_BYTES, Problem, SecretError, SecretRef,
    SecretValue, SecretWarning, SourceName,
};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use tempfile::TempDir;

use crate::command::{Stop, run_with};
use crate::keyring_store::{Failure, KeyringStore, Memory};
use crate::{FileReader, SystemSecrets};

// ---------------------------------------------------------------- helpers

/// The contract's harness over a scratch directory, the real environment
/// reader and files, real commands and a keyring in memory.
struct RealHarness {
    /// Holds the scratch directory until the harness is dropped.
    _directory: TempDir,
    /// The scratch directory resolved to a plain path: a `$TMPDIR` holding `..` would
    /// keep the credentials directory from being trusted (security.md §10.2).
    root: PathBuf,
    environment: Mutex<BTreeMap<String, String>>,
    keyring: Arc<Memory>,
    commands: AtomicUsize,
}

impl RealHarness {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        Self {
            _directory: directory,
            root,
            environment: Mutex::default(),
            keyring: Arc::default(),
            commands: AtomicUsize::new(0),
        }
    }

    fn path(&self, name: &str) -> String {
        self.root.join(name).display().to_string()
    }

    fn next(&self) -> usize {
        self.commands.fetch_add(1, Ordering::Relaxed)
    }
}

impl SecretsHarness for RealHarness {
    fn resolver(&self) -> Arc<dyn SecretResolver> {
        let environment = self.environment.lock().unwrap().clone();
        // The keyring last: a mutant that makes a builder method give a new
        // resolver must not leave it reading the session's own store.
        Arc::new(
            SystemSecrets::new()
                .with_environment(environment)
                .with_command_timeout(Duration::from_millis(500))
                .with_keyring(self.keyring.clone()),
        )
    }

    fn set_env(&self, name: &str, value: &str) {
        self.environment
            .lock()
            .unwrap()
            .insert(name.to_owned(), value.to_owned());
    }

    fn write_file(&self, name: &str, contents: &[u8], mode: u32) -> String {
        let path = self.path(name);
        if let Some(directory) = Path::new(&path).parent() {
            fs::create_dir_all(directory).unwrap();
        }
        // A read-only file of that name goes first: a new one replaces it.
        let _ = fs::remove_file(&path);
        fs::write(&path, contents).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn link(&self, name: &str, target: &str) -> String {
        let path = self.path(name);
        if let Some(directory) = Path::new(&path).parent() {
            fs::create_dir_all(directory).unwrap();
        }
        let _ = fs::remove_file(&path);
        std::os::unix::fs::symlink(target, &path).unwrap();
        path
    }

    fn missing_file(&self) -> String {
        self.path("nothing-here")
    }

    fn directory(&self) -> String {
        let path = self.path("a-directory");
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn set_keyring_entry(&self, name: &str, secret: &str) -> KeyringEntry {
        let name = format!("{name}-{}", self.next());
        self.keyring
            .set("ladderback-demo", &name, secret.as_bytes());
        KeyringEntry::parse(&format!("ladderback-demo/{name}")).unwrap()
    }

    fn empty_keyring_entry(&self) -> KeyringEntry {
        KeyringEntry::parse("ladderback-demo/nothing-here").unwrap()
    }

    fn printing(&self, stdout: &[u8]) -> CommandLine {
        let path = self.write_file(&format!("printed-{}", self.next()), stdout, 0o600);
        CommandLine::new("cat", [path]).unwrap()
    }

    fn failing(&self, status: i32, stdout: &str, stderr: &str) -> CommandLine {
        sh(
            r#"printf %s "$1"; printf %s "$2" >&2; exit "$3""#,
            [stdout, stderr, &status.to_string()],
        )
    }

    fn hanging(&self) -> CommandLine {
        sh("sleep 60", Vec::<String>::new())
    }

    fn unstartable(&self) -> CommandLine {
        CommandLine::new(self.path("no-such-program"), Vec::<String>::new()).unwrap()
    }
}

/// `sh -c script sh args…`: the arguments are `$1`, `$2`, … of the script.
fn sh<S: AsRef<str>>(script: &str, args: impl IntoIterator<Item = S>) -> CommandLine {
    let args = ["-c", script, "sh"]
        .into_iter()
        .map(str::to_owned)
        .chain(args.into_iter().map(|arg| arg.as_ref().to_owned()));
    CommandLine::new("sh", args).unwrap()
}

/// A resolver that reads no variables, keeps its keyring in memory and gives
/// commands `limit`. The keyring is set last, as in `RealHarness`.
fn resolver(limit: Duration) -> SystemSecrets {
    SystemSecrets::new()
        .with_environment(Vec::<(String, String)>::new())
        .with_command_timeout(limit)
        .with_keyring(Arc::new(Memory::default()))
}

fn command(command: CommandLine) -> SecretRef {
    SecretRef::Command(command)
}

fn file(path: impl AsRef<Path>) -> SecretRef {
    SecretRef::File {
        path: path.as_ref().display().to_string(),
        read: FileRead::Whole,
    }
}

fn program(name: &str) -> SourceName {
    SourceName::Command {
        program: name.to_owned(),
    }
}

fn gives(value: &str) -> Result<Resolved, SecretError> {
    Ok(Resolved {
        value: SecretValue::new(value),
        warnings: Vec::new(),
    })
}

/// A zombie nobody reaped (an orphan of a container's first process) is
/// dead, and `kill(pid, 0)` would say it lives.
#[cfg(target_os = "linux")]
fn is_running(pid: i32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .and_then(|(_, rest)| rest.trim_start().chars().next())
            .is_some_and(|state| state != 'Z' && state != 'X')
    })
}

#[cfg(not(target_os = "linux"))]
fn is_running(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid), None).is_ok()
}

/// Kills what is left of a process a test started, whatever the test found.
struct Reaper(i32);

impl Drop for Reaper {
    fn drop(&mut self) {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let _ = kill(Pid::from_raw(self.0), Signal::SIGKILL);
    }
}

/// The process id a script wrote to `pidfile`, once it has.
async fn recorded_pid(pidfile: &Path) -> i32 {
    let deadline = Instant::now() + SLACK;
    loop {
        if let Ok(text) = fs::read_to_string(pidfile)
            && text.ends_with('\n')
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "no pid in {}", pidfile.display());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn is_gone_soon(pid: i32) -> bool {
    let deadline = Instant::now() + SLACK;
    while is_running(pid) {
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}

/// What a call may take past the limit, grace or delay it waits for, on a
/// slow runner. A test that waits for a signal that never went fails this
/// soon after, not at a limit of the code's: a mutant that sends nothing
/// leaves a dozen tests waiting, and a mutation run gives the whole test
/// binary three times its usual time.
const SLACK: Duration = Duration::from_secs(2);

/// Awaits `call` for at most `bound` and fails the test when it is still
/// running then.
async fn within<T>(bound: Duration, call: impl Future<Output = T>) -> T {
    tokio::time::timeout(bound, call)
        .await
        .unwrap_or_else(|_| panic!("the call was still running after {bound:?}"))
}

// --------------------------------------------------------------- contract

#[tokio::test]
async fn the_system_resolver_meets_the_secrets_contract() {
    // The harness's commands have half a second, and the contract times one
    // out; the slack is for that call and the many quick ones.
    within(
        Duration::from_millis(500) + 2 * SLACK,
        app::contract::secrets::run(&RealHarness::new()),
    )
    .await;
}

// --------------------------------------------------------------- commands

#[tokio::test]
async fn a_timed_out_command_takes_a_helper_of_its_group_with_it() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = sh(
        r#"sleep 60 & echo $! > "$1"; wait"#,
        [pidfile.display().to_string()],
    );
    let limit = Duration::from_secs(1);
    let result = within(limit + SLACK, resolver(limit).resolve(&command(script))).await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
    let pid = recorded_pid(&pidfile).await;
    let _reaper = Reaper(pid);
    assert!(is_gone_soon(pid).await, "process {pid} is still running");
}

#[tokio::test]
async fn the_time_limit_is_reported_in_whole_seconds_rounded_up() {
    for (limit, seconds) in [
        (Duration::from_millis(300), 1),
        (Duration::from_millis(1_100), 2),
    ] {
        let result = within(
            limit + SLACK,
            resolver(limit).resolve(&command(sh("sleep 60", Vec::<String>::new()))),
        )
        .await;
        assert_eq!(
            result,
            Err(SecretError {
                source: program("sh"),
                problem: Problem::TimedOut { seconds },
            }),
            "{limit:?}"
        );
    }
}

#[tokio::test]
async fn a_command_that_prints_without_end_is_stopped_at_the_limit_and_killed() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = sh(
        r#"echo $$ > "$1"; exec yes"#,
        [pidfile.display().to_string()],
    );
    // A minute is far more than this test waits for: the size ends it.
    let result = within(
        SLACK,
        resolver(Duration::from_secs(60)).resolve(&command(script)),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TooLarge {
                limit: MAX_SECRET_BYTES
            },
        })
    );
    let pid = recorded_pid(&pidfile).await;
    let _reaper = Reaper(pid);
    assert!(is_gone_soon(pid).await, "process {pid} is still running");
}

#[tokio::test]
async fn a_helper_that_leaves_a_daemon_holding_its_output_still_gives_its_secret() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    // Like gpg-agent: the helper is done, and something it started keeps the
    // output open.
    let script = sh(
        r#"sleep 60 & echo $! > "$1"; echo hunter2"#,
        [pidfile.display().to_string()],
    );
    let started = Instant::now();
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(script)),
    )
    .await;
    let pid = recorded_pid(&pidfile).await;
    let _reaper = Reaper(pid);
    assert_eq!(result, gives("hunter2"));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    // A command that worked is not cleaned up after: its daemon lives.
    assert!(is_running(pid));
}

/// The limit is the command's. One that has exited in time is not timed out
/// because a daemon of its holds the output open past the limit, and its
/// group is not signalled: its leader is reaped, and the id may be another
/// process's.
#[tokio::test]
async fn a_command_that_exited_in_time_is_not_timed_out_by_a_daemon_of_its() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let signalled = Mutex::new(0_u32);
    let signal = |_: Pid, _: Signal| *signalled.lock().unwrap() += 1;
    let stop = Stop {
        grace: Duration::from_secs(5),
        reap_limit: Duration::from_secs(5),
        signal: &signal,
    };
    let script = sh(
        r#"sleep 60 & echo $! > "$1"; echo hunter2"#,
        [pidfile.display().to_string()],
    );
    let result = within(
        Duration::from_secs(5),
        run_with(&script, Duration::from_millis(300), &stop),
    )
    .await;
    let pid = recorded_pid(&pidfile).await;
    let _reaper = Reaper(pid);
    assert_eq!(result, Ok(SecretValue::new("hunter2")));
    assert_eq!(*signalled.lock().unwrap(), 0);
}

#[tokio::test]
async fn dropping_the_resolve_future_kills_the_command_s_group() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = command(sh(
        r#"sleep 60 & echo $! > "$1"; wait"#,
        [pidfile.display().to_string()],
    ));
    let secrets = resolver(Duration::from_secs(60));
    let pid = {
        let resolving = secrets.resolve(&script);
        tokio::pin!(resolving);
        tokio::select! {
            result = &mut resolving => panic!("the command ended: {result:?}"),
            pid = recorded_pid(&pidfile) => pid,
        }
        // `resolving` is dropped here, with the command running.
    };
    let _reaper = Reaper(pid);
    assert!(is_gone_soon(pid).await, "process {pid} is still running");
}

#[tokio::test]
async fn arguments_are_text_and_no_shell_reads_them() {
    let directory = TempDir::new().unwrap();
    let marker = directory.path().join("pwned");
    let marker_text = marker.display().to_string();
    for argument in [
        format!("$(touch {marker_text})"),
        format!("`touch {marker_text}`"),
        format!("; touch {marker_text}"),
        "*".to_owned(),
        "$HOME".to_owned(),
        "a b".to_owned(),
    ] {
        let result = within(
            Duration::from_secs(5),
            resolver(Duration::from_secs(30))
                .resolve(&command(sh(r#"printf %s "$1""#, [&argument]))),
        )
        .await;
        assert_eq!(result, gives(&argument));
    }
    assert!(!marker.exists());
}

#[tokio::test]
async fn a_command_gets_no_input() {
    let started = Instant::now();
    // `cat` with no arguments copies its input until it ends.
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(
            CommandLine::new("cat", Vec::<String>::new()).unwrap(),
        )),
    )
    .await;
    assert_eq!(result, Err(SecretError::missing(program("cat"))));
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// A password command inherits no descriptor that another start in the
/// daemon has open: it starts through the spawn gate, so it waits while a
/// pipe that is not yet close-on-exec is open, as a worker's, a hook's or
/// another command's pipes are for a moment where the platform has no
/// `pipe2` (#234). A command that held a worker's pipe could read the job
/// it is sent, secrets and all, or keep it from the end of its input.
#[tokio::test]
async fn a_command_inherits_no_pipe_another_start_has_open() {
    let pipe = testkit::inheritable::InheritablePipe::open();
    let found = sh(
        &format!("{}\necho none", testkit::inheritable::INHERITED),
        pipe.numbers(),
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(found)),
    )
    .await;
    let Ok(Resolved { value, .. }) = result else {
        panic!("the command failed: {result:?}");
    };
    assert_eq!(value.expose(), "none", "the command inherited a descriptor");
}

/// A password command waits for the spawn gate off the runtime's threads:
/// a start whose `exec` stalls holds the gate, and a resolve that waited for
/// it on a runtime thread would stop every other task of that thread
/// meanwhile. Here the gate is let go by a task of the same one-thread
/// runtime, so it can be only while the resolve waits elsewhere.
#[tokio::test]
async fn a_command_waits_for_the_gate_off_the_runtime() {
    let (release, stalled) = testkit::inheritable::stalled_start(Duration::from_secs(2));
    let resolver = resolver(Duration::from_secs(30));
    let found = command(sh("echo found", Vec::<String>::new()));
    let let_go = async {
        tokio::task::yield_now().await;
        let _ = release.send(());
    };
    let (result, ()) = tokio::join!(resolver.resolve(&found), let_go);
    let Ok(Resolved { value, .. }) = result else {
        panic!("the command failed: {result:?}");
    };
    assert_eq!(value.expose(), "found");
    assert!(
        stalled.join().unwrap(),
        "the resolve waited for the gate on the runtime's thread"
    );
}

#[tokio::test]
async fn a_command_that_is_killed_by_a_signal_has_no_status() {
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(sh("kill -9 $$", Vec::<String>::new()))),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::CommandFailed {
                status: None,
                detail: String::new(),
            },
        })
    );
}

#[tokio::test]
async fn a_command_that_complains_at_length_is_cut_short_and_not_waited_for_in_vain() {
    // More than a pipe holds: nobody reading stderr would block it forever.
    let result = within(
        Duration::from_secs(10),
        resolver(Duration::from_secs(30)).resolve(&command(sh(
            r#"head -c 1048576 /dev/zero | tr '\0' x >&2; exit 2"#,
            Vec::<String>::new(),
        ))),
    )
    .await;
    let Err(SecretError {
        problem: Problem::CommandFailed { status, detail },
        ..
    }) = result
    else {
        panic!("{result:?}");
    };
    assert_eq!(status, Some(2));
    assert!(detail.starts_with("xxxx"), "{detail}");
    assert!(detail.len() < 600, "{} bytes", detail.len());
}

/// The detail is the first line of stderr with something on it, looked for in
/// the first 8 KiB the command wrote, which is all that is kept of it: a line
/// that starts within them is found, and one that starts after them is not.
#[tokio::test]
async fn the_detail_is_looked_for_in_the_first_8_kib_of_stderr() {
    let directory = TempDir::new().unwrap();
    let stderr_file = directory.path().join("stderr");
    for (blank_lines, detail) in [(4 * 1024, "access denied"), (9 * 1024, "")] {
        let stderr = format!("{}access denied\n", "\n".repeat(blank_lines));
        fs::write(&stderr_file, stderr).unwrap();
        let result = within(
            Duration::from_secs(5),
            resolver(Duration::from_secs(30)).resolve(&command(sh(
                r#"cat "$1" >&2; exit 1"#,
                [stderr_file.display().to_string()],
            ))),
        )
        .await;
        assert_eq!(
            result,
            Err(SecretError {
                source: program("sh"),
                problem: Problem::CommandFailed {
                    status: Some(1),
                    detail: detail.to_owned(),
                },
            }),
            "after {blank_lines} blank lines"
        );
    }
}

/// A helper that rejects its secret and quotes it on stderr, more of it than
/// is kept: the kept stderr ends inside the secret, or holds less than the
/// whole of it, and nothing of it reaches the detail (I4).
#[tokio::test]
async fn a_secret_quoted_past_the_kept_stderr_leaves_nothing_of_itself_in_the_error() {
    let directory = TempDir::new().unwrap();
    let long: String = ('a'..='z').cycle().take(10_000).collect();
    let short: String = ('a'..='z').rev().cycle().take(600).collect();
    for (secret, stderr) in [
        // Longer than the kept stderr.
        (long.clone(), long.clone()),
        // Quoted again and again, so the cut goes through one.
        (short.clone(), format!("rejected {}", short.repeat(14))),
    ] {
        let stdout_file = directory.path().join("stdout");
        let stderr_file = directory.path().join("stderr");
        fs::write(&stdout_file, format!("{secret}\n")).unwrap();
        fs::write(&stderr_file, &stderr).unwrap();
        let result = within(
            Duration::from_secs(5),
            resolver(Duration::from_secs(30)).resolve(&command(sh(
                r#"cat "$1"; cat "$2" >&2; exit 1"#,
                [
                    stdout_file.display().to_string(),
                    stderr_file.display().to_string(),
                ],
            ))),
        )
        .await;
        let Err(SecretError {
            problem: Problem::CommandFailed { detail, .. },
            ..
        }) = result
        else {
            panic!("{result:?}");
        };
        let start = secret.get(..8).unwrap();
        assert!(
            !detail.contains(start),
            "the secret's start is in {detail:?}"
        );
    }
}

/// A helper that rejects its secret, quotes it on stderr and exits, while a
/// process it started holds stderr open (stdout it lets go, so all of the
/// secret is known): stderr is read for the grace and then left, so what
/// was read can end partway through the secret (a write larger than the
/// pipe takes at once reaches the reader in parts; the helper here writes
/// the start and leaves the rest unsaid, which is where such a split leaves
/// the reader). Nothing of it reaches the detail (I4).
#[tokio::test]
async fn a_secret_cut_off_in_stderr_a_daemon_holds_open_leaves_nothing_of_itself_in_the_error() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = sh(
        concat!(
            r#"sleep 60 >/dev/null & echo $! > "$1"; "#,
            "echo hunter2-canary; printf 'rejected hunter2-' >&2; exit 1"
        ),
        [pidfile.display().to_string()],
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(script)),
    )
    .await;
    let _reaper = Reaper(recorded_pid(&pidfile).await);
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::CommandFailed {
                status: Some(1),
                detail: "rejected ***".to_owned(),
            },
        })
    );
}

/// A helper can cut its own stderr short: it stops partway through the
/// secret it quotes and exits on its own, with nothing holding its pipes and
/// no newline at the end, as one that a signal Ladderback didn't send ended.
/// Its stderr is masked as a cut text all the same, whoever stopped it, and
/// nothing of the secret reaches the detail (I4, #232). A stderr that ends
/// nowhere near the secret is kept as it was said.
#[tokio::test]
async fn stderr_a_helper_cut_short_itself_leaves_nothing_of_the_secret_in_the_error() {
    for (said, detail) in [
        ("rejected hello", "rejected hello"),
        ("rejected hunter2-", "rejected ***"),
    ] {
        let script = sh(r#"echo hunter2-canary; printf %s "$1" >&2; exit 1"#, [said]);
        let result = within(
            Duration::from_secs(5),
            resolver(Duration::from_secs(30)).resolve(&command(script)),
        )
        .await;
        assert_eq!(
            result,
            Err(SecretError {
                source: program("sh"),
                problem: Problem::CommandFailed {
                    status: Some(1),
                    detail: detail.to_owned(),
                },
            }),
            "{said:?}"
        );
    }
}

/// A helper that prints the start of its secret and fails, quoting all of it
/// on stderr, while a process it started holds stdout open: stdout is read
/// for the grace and then left, so what was read is only part of the secret,
/// and masking the detail against that part would show the rest. Nothing of
/// what the helper said is quoted then, and the error keeps its status
/// (I4, #233).
#[tokio::test]
async fn a_secret_cut_off_in_stdout_a_daemon_holds_open_leaves_nothing_of_itself_in_the_error() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = sh(
        concat!(
            r#"sleep 60 2>/dev/null & echo $! > "$1"; "#,
            "printf hunter2-ca; echo 'rejected hunter2-canary' >&2; exit 1"
        ),
        [pidfile.display().to_string()],
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(script)),
    )
    .await;
    let _reaper = Reaper(recorded_pid(&pidfile).await);
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::CommandFailed {
                status: Some(1),
                detail: String::new(),
            },
        })
    );
}

/// A process the helper started may hold stdout open a moment after the
/// helper exits, and let go of it within the grace: stdout is then read to
/// its end, so it holds all of the secret, and the detail is masked against
/// it as for any failed helper, not left empty (#233).
#[tokio::test]
async fn stdout_a_daemon_lets_go_of_within_the_grace_is_read_whole_for_the_error() {
    let script = sh(
        concat!(
            "sleep 0.3 2>/dev/null & ",
            "echo hunter2-canary; echo 'rejected hunter2-canary' >&2; exit 1"
        ),
        Vec::<String>::new(),
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(script)),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::CommandFailed {
                status: Some(1),
                detail: "rejected ***".to_owned(),
            },
        })
    );
}

/// A helper that prints past the cap, cutting its secret there, and fails,
/// quoting all of the secret on stderr: what it printed is too large, and
/// that is the error, which quotes nothing it said (I4, #233).
#[tokio::test]
async fn a_secret_cut_off_at_the_stdout_cap_leaves_nothing_of_itself_in_the_error() {
    let script = sh(
        concat!(
            "printf '%65530s' ''; printf hunter2-canary; ",
            "echo 'rejected hunter2-canary' >&2; exit 1"
        ),
        Vec::<String>::new(),
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(script)),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TooLarge {
                limit: MAX_SECRET_BYTES
            },
        })
    );
}

/// A helper killed partway through printing its secret, or exiting there,
/// that quotes all of the secret on stderr: its stdout was read to its end
/// but holds only part of the secret, and the detail masked against that
/// part would show the rest. Nothing it said is quoted, and the error keeps
/// its status (I4, #237).
#[tokio::test]
async fn a_helper_that_ends_partway_through_printing_its_secret_quotes_nothing_it_said() {
    for (end, status) in [("kill -9 $$", None), ("exit 1", Some(1))] {
        let script = sh(
            &format!("printf hunter2-ca; echo 'rejected hunter2-canary' >&2; {end}"),
            Vec::<String>::new(),
        );
        let result = within(
            Duration::from_secs(5),
            resolver(Duration::from_secs(30)).resolve(&command(script)),
        )
        .await;
        assert_eq!(
            result,
            Err(SecretError {
                source: program("sh"),
                problem: Problem::CommandFailed {
                    status,
                    detail: String::new(),
                },
            }),
            "{end}"
        );
    }
}

/// A helper that succeeds while something it started holds its stdout open,
/// before the line that is its secret has ended: what was read when the
/// grace ran out may be only part of the secret, which is refused rather
/// than used (#237).
#[tokio::test]
async fn a_helper_whose_daemon_holds_its_output_before_its_secret_s_line_ended_gives_none() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = sh(
        r#"sleep 60 & echo $! > "$1"; printf hunter2-ca"#,
        [pidfile.display().to_string()],
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_secs(30)).resolve(&command(script)),
    )
    .await;
    let _reaper = Reaper(recorded_pid(&pidfile).await);
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::OutputCut,
        })
    );
}

/// A helper that prints part of its secret, quotes all of it on stderr and
/// then outlives its limit: it is timed out, which gives neither what it
/// printed nor what it said (I4, #237).
#[tokio::test]
async fn a_helper_timed_out_after_printing_part_of_its_secret_gives_none_of_it() {
    let script = sh(
        "printf hunter2-ca; echo 'rejected hunter2-canary' >&2; sleep 60",
        Vec::<String>::new(),
    );
    let result = within(
        Duration::from_secs(5),
        resolver(Duration::from_millis(300)).resolve(&command(script)),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
}

#[tokio::test]
async fn the_error_of_a_failed_command_holds_nothing_it_printed_as_its_secret() {
    let echo_both = CommandLine::new(
        "sh",
        [
            "-c",
            r#"echo "$1"; echo "rejected value $1" >&2; exit 1"#,
            "sh",
            "CANARY-hunter2",
        ],
    )
    .unwrap();
    let echoes = |stdout: &str, stderr: &str| {
        sh(
            r#"printf %s "$1"; printf %s "$2" >&2; exit 1"#,
            [stdout, stderr],
        )
    };
    let commands = [
        echo_both,
        // The trimmed form of what it printed.
        echoes("  CANARY-hunter2  \n", "rejected value CANARY-hunter2"),
        // One line of several.
        echoes(
            "CANARY-first-line\nCANARY-second-line\n",
            "rejected value CANARY-second-line",
        ),
        echoes(
            "CANARY-first-line\nCANARY-second-line\n",
            "rejected value CANARY-first-line",
        ),
        // The first line is what the command would have given, and a short
        // one is no needle of its own when the output spans lines.
        echoes(
            "CANARY\nCANARY-the-second-line-is-long\n",
            "rejected value CANARY",
        ),
    ];
    for line in commands {
        let result = within(
            Duration::from_secs(5),
            resolver(Duration::from_secs(30)).resolve(&command(line)),
        )
        .await;
        let Err(error) = result else {
            panic!("the command failed, and this succeeded");
        };
        let text = format!("{error:?}");
        assert!(!text.contains("CANARY"), "{text}");
        assert_eq!(error.source, program("sh"));
        let Problem::CommandFailed { status, detail } = error.problem else {
            panic!("{text}");
        };
        // Still of use: the status and what the command said, with the value
        // shown as masked.
        assert_eq!(status, Some(1), "{text}");
        assert!(detail.starts_with("rejected value ***"), "{text}");
    }
}

// ------------------------------------------------------------------ files

/// The read runs on a thread of its own: an open that blocks can't be cut
/// short, and a test that waited for it, or a runtime that shut down after
/// it, would hang with it.
#[test]
fn a_fifo_is_not_read_and_does_not_block() {
    use nix::sys::stat::Mode;
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("pipe");
    nix::unistd::mkfifo(&path, Mode::S_IRWXU).unwrap();
    let path = path.display().to_string();
    let (sent, received) = std::sync::mpsc::channel();
    let opened = path.clone();
    std::thread::spawn(move || {
        let _ = sent.send(crate::file::read(&opened, FileRead::Whole, None));
    });
    let result = received
        .recv_timeout(Duration::from_secs(5))
        .expect("opening a FIFO blocked");
    assert!(
        matches!(&result, Err(SecretError { problem: Problem::Unreachable { .. }, source })
            if *source == SourceName::File { path }),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_device_is_not_read_to_its_end() {
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        resolver(Duration::from_secs(1)).resolve(&file("/dev/zero")),
    )
    .await
    .expect("read a device");
    assert!(
        matches!(
            result,
            Err(SecretError {
                problem: Problem::Unreachable { .. },
                ..
            })
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_path_through_a_file_is_missing() {
    let directory = TempDir::new().unwrap();
    let plain = directory.path().join("plain");
    fs::write(&plain, "x").unwrap();
    let below = plain.join("below");
    assert_eq!(
        resolver(Duration::from_secs(1))
            .resolve(&file(&below))
            .await,
        Err(SecretError::missing(SourceName::File {
            path: below.display().to_string()
        }))
    );
}

#[tokio::test]
async fn a_file_that_may_not_be_read_is_unreachable() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("locked");
    fs::write(&path, "hunter2").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let result = resolver(Duration::from_secs(1)).resolve(&file(&path)).await;
    if result.is_ok() {
        testkit::skip!("running as root, which reads any file");
    }
    assert!(
        matches!(
            &result,
            Err(SecretError { problem: Problem::Unreachable { detail }, .. }) if !detail.is_empty()
        ),
        "{result:?}"
    );
}

// ------------------------------------------------------------ environment

#[tokio::test]
async fn the_given_environment_replaces_the_process_environment() {
    // PATH is set in any process a test runs in.
    assert!(std::env::var_os("PATH").is_some());
    let injected = SystemSecrets::new().with_environment([("DEMO_ONLY_HERE", "1")]);
    assert_eq!(
        injected
            .resolve(&SecretRef::Env("DEMO_ONLY_HERE".to_owned()))
            .await,
        gives("1")
    );
    assert_eq!(
        injected.resolve(&SecretRef::Env("PATH".to_owned())).await,
        Err(SecretError::missing(SourceName::Env {
            name: "PATH".to_owned()
        }))
    );
}

#[tokio::test]
async fn the_process_environment_is_read_when_none_is_given() {
    let secrets = SystemSecrets::new();
    let expected = std::env::var("PATH").unwrap();
    assert_eq!(
        secrets.resolve(&SecretRef::Env("PATH".to_owned())).await,
        gives(&expected)
    );
    assert_eq!(
        secrets
            .resolve(&SecretRef::Env("DEMO_ONLY_HERE".to_owned()))
            .await,
        Err(SecretError::missing(SourceName::Env {
            name: "DEMO_ONLY_HERE".to_owned()
        }))
    );
}

#[tokio::test]
async fn a_name_no_variable_can_have_is_missing() {
    let injected = SystemSecrets::new().with_environment([("A=B", "1"), ("", "2")]);
    for secrets in [SystemSecrets::new(), injected] {
        for name in ["", "A=B", "A\0B", "="] {
            assert_eq!(
                secrets.resolve(&SecretRef::Env(name.to_owned())).await,
                Err(SecretError::missing(SourceName::Env {
                    name: name.to_owned()
                })),
                "{name:?}"
            );
        }
    }
}

#[test]
fn the_debug_text_shows_no_variable() {
    let secrets = SystemSecrets::new().with_environment([("DEMO_TOKEN", "canary-value")]);
    let text = format!("{secrets:?}");
    assert!(text.contains("SystemSecrets"), "{text}");
    assert!(!text.contains("canary-value"), "{text}");
    assert!(!text.contains("DEMO_TOKEN"), "{text}");
}

// ---------------------------------------------------------------- keyring

struct Locked;

impl KeyringStore for Locked {
    fn get(&self, _service: &str, _name: &str) -> Result<String, Failure> {
        Err(Failure::Unreachable("the keychain is locked".to_owned()))
    }
}

#[tokio::test]
async fn a_keyring_that_cannot_be_reached_is_unreachable_and_says_why() {
    let entry = KeyringEntry::parse("ladderback-demo/nas").unwrap();
    let result = resolver(Duration::from_secs(1))
        .with_keyring(Arc::new(Locked))
        .resolve(&SecretRef::Keyring(entry))
        .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: SourceName::Keyring {
                entry: "ladderback-demo/nas".to_owned()
            },
            problem: Problem::Unreachable {
                detail: "the keychain is locked".to_owned()
            },
        })
    );
}

struct NoService;

impl KeyringStore for NoService {
    fn get(&self, _service: &str, _name: &str) -> Result<String, Failure> {
        Err(Failure::NoService("no session bus".to_owned()))
    }
}

#[tokio::test]
async fn a_keyring_with_no_service_to_ask_says_so_and_why() {
    let entry = KeyringEntry::parse("ladderback-demo/nas").unwrap();
    let result = resolver(Duration::from_secs(1))
        .with_keyring(Arc::new(NoService))
        .resolve(&SecretRef::Keyring(entry))
        .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: SourceName::Keyring {
                entry: "ladderback-demo/nas".to_owned()
            },
            problem: Problem::KeyringUnavailable {
                detail: "no session bus".to_owned()
            },
        })
    );
}

/// A keyring that answers late, as one does that waits for the user to
/// unlock it.
struct Stuck(Duration);

impl KeyringStore for Stuck {
    fn get(&self, _service: &str, _name: &str) -> Result<String, Failure> {
        std::thread::sleep(self.0);
        Err(Failure::NoEntry)
    }
}

#[tokio::test]
async fn a_keyring_that_does_not_answer_in_time_is_timed_out() {
    let entry = KeyringEntry::parse("ladderback-demo/nas").unwrap();
    let started = Instant::now();
    let result = resolver(Duration::from_millis(300))
        .with_keyring(Arc::new(Stuck(Duration::from_secs(3))))
        .resolve(&SecretRef::Keyring(entry))
        .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: SourceName::Keyring {
                entry: "ladderback-demo/nas".to_owned()
            },
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "it waited {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_keyring_secret_is_text_and_within_the_limit() {
    let memory = Arc::new(Memory::default());
    memory.set("ladderback-demo", "binary", &[b'a', 0xff]);
    memory.set(
        "ladderback-demo",
        "big",
        "x".repeat(MAX_SECRET_BYTES + 1).as_bytes(),
    );
    memory.set(
        "ladderback-demo",
        "limit",
        "x".repeat(MAX_SECRET_BYTES).as_bytes(),
    );
    let secrets = resolver(Duration::from_secs(1)).with_keyring(memory);
    let read = |name: &str| {
        let entry = KeyringEntry::parse(&format!("ladderback-demo/{name}")).unwrap();
        let secrets = &secrets;
        async move { secrets.resolve(&SecretRef::Keyring(entry)).await }
    };
    let source = |name: &str| SourceName::Keyring {
        entry: format!("ladderback-demo/{name}"),
    };
    assert_eq!(
        read("binary").await,
        Err(SecretError {
            source: source("binary"),
            problem: Problem::NotUnicode
        })
    );
    assert_eq!(
        read("big").await,
        Err(SecretError {
            source: source("big"),
            problem: Problem::TooLarge {
                limit: MAX_SECRET_BYTES
            }
        })
    );
    assert_eq!(read("limit").await, gives(&"x".repeat(MAX_SECRET_BYTES)));
}

/// Whether the OS keyring may be used: only when it is asked for, because it
/// is the session's own store (an unlocked keychain, a Secret Service) and
/// keeps what a test puts in it.
fn os_keyring_enabled(setting: Option<&std::ffi::OsStr>) -> bool {
    setting.is_some_and(|value| value == "1")
}

#[test]
fn the_os_keyring_is_used_only_when_it_is_asked_for() {
    use std::ffi::OsStr;
    assert!(!os_keyring_enabled(None));
    for setting in ["", "0", "true", "yes", "11", " 1"] {
        assert!(
            !os_keyring_enabled(Some(OsStr::new(setting))),
            "{setting:?}"
        );
    }
    assert!(os_keyring_enabled(Some(OsStr::new("1"))));
}

/// What the OS store's errors become. A value that is not text comes back
/// with its bytes, which are a secret: they are dropped (I4). A keychain that
/// refuses is one that can't be reached.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn an_os_keyring_error_says_why_and_holds_no_secret() {
    use crate::keyring_store::{OsStore, failure};
    for store in [OsStore::Keychain, OsStore::SecretService] {
        assert_eq!(failure(keyring::Error::NoEntry, store), Failure::NoEntry);
        assert_eq!(
            failure(keyring::Error::BadEncoding(b"hunter2\xff".to_vec()), store),
            Failure::NotUnicode
        );
    }
    let refused = || keyring::Error::NoStorageAccess("the keychain is locked".into());
    let said = refused().to_string();
    assert!(said.contains("the keychain is locked"), "{said}");
    assert_eq!(
        failure(refused(), OsStore::Keychain),
        Failure::Unreachable(said)
    );
}

/// The Secret Service's failures to answer at all, as the `keyring` crate
/// reports them on Linux, with the `dbus-secret-service` error it boxes: no
/// session bus or no daemon on it (a D-Bus error, `PlatformFailure`), and a
/// locked or missing collection or a dismissed unlock prompt
/// (`NoStorageAccess`). Each is a store that isn't there to ask, in the
/// platform's words, whatever the machine running the test has.
#[cfg(target_os = "linux")]
#[test]
fn a_secret_service_that_cannot_be_asked_is_no_keyring_service() {
    use crate::keyring_store::{OsStore, failure};
    use dbus_secret_service::Error;
    let bus = |name: &str, message: &str| Error::Dbus(dbus::Error::new_custom(name, message));
    let cases = [
        keyring::Error::PlatformFailure(Box::new(bus(
            "org.freedesktop.DBus.Error.NotSupported",
            "Unable to autolaunch a dbus-daemon without a $DISPLAY for X11",
        ))),
        keyring::Error::PlatformFailure(Box::new(bus(
            "org.freedesktop.DBus.Error.FileNotFound",
            "Failed to connect to socket /run/user/1000/bus: No such file or directory",
        ))),
        keyring::Error::PlatformFailure(Box::new(bus(
            "org.freedesktop.DBus.Error.ServiceUnknown",
            "The name org.freedesktop.secrets was not provided by any .service files",
        ))),
        keyring::Error::PlatformFailure(Box::new(bus(
            "org.freedesktop.Secret.Error.IsLocked",
            "Cannot get secret of a locked object",
        ))),
        keyring::Error::PlatformFailure(Box::new(Error::Unavailable)),
        keyring::Error::NoStorageAccess(Box::new(Error::Locked)),
        keyring::Error::NoStorageAccess(Box::new(Error::NoResult)),
        keyring::Error::NoStorageAccess(Box::new(Error::Prompt)),
    ];
    for error in cases {
        let said = error.to_string();
        assert_eq!(
            failure(error, OsStore::SecretService),
            Failure::NoService(said)
        );
    }
}

/// A Secret Service that answered and still gave no secret is unreadable,
/// not missing: a reply it couldn't decrypt or parse, a bad object path, a
/// secret that is not text, and a D-Bus call that failed once the service
/// took it (no reply, access denied, the item gone).
#[cfg(target_os = "linux")]
#[test]
fn a_secret_service_that_answers_and_fails_is_unreadable() {
    use crate::keyring_store::{OsStore, failure};
    use dbus_secret_service::Error;
    let bus = |name: &str, message: &str| Error::Dbus(dbus::Error::new_custom(name, message));
    let cases = [
        Error::Crypto("Unpad Error".into()),
        Error::Parse,
        Error::Path("invalid object path".into()),
        Error::UnsupportedSecretFormat,
        bus(
            "org.freedesktop.DBus.Error.NoReply",
            "Did not receive a reply",
        ),
        bus(
            "org.freedesktop.DBus.Error.AccessDenied",
            "Rejected send message",
        ),
        bus("org.freedesktop.Secret.Error.NoSuchObject", "No such item"),
        bus(
            "org.freedesktop.DBus.Error.UnknownObject",
            "No such object path",
        ),
    ];
    for error in cases {
        let error = keyring::Error::PlatformFailure(Box::new(error));
        let said = error.to_string();
        assert_eq!(
            failure(error, OsStore::SecretService),
            Failure::Unreachable(said)
        );
    }
}

/// What the Secret Service says about an entry, rather than about itself, is
/// not a missing service; nor is a failure whose cause is not the Secret
/// Service's own error type, which says nothing of what went wrong.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_secret_service_that_refuses_an_entry_is_unreachable() {
    use crate::keyring_store::{OsStore, failure};
    let cases: [fn() -> keyring::Error; 3] = [
        || keyring::Error::Invalid("service".into(), "cannot be empty".into()),
        || {
            keyring::Error::PlatformFailure(
                "DBus error: The name org.freedesktop.secrets was not provided by any .service files"
                    .into(),
            )
        },
        || keyring::Error::NoStorageAccess("Secret Service: object locked".into()),
    ];
    for error in cases {
        let said = error().to_string();
        assert_eq!(
            failure(error(), OsStore::SecretService),
            Failure::Unreachable(said)
        );
    }
}

/// The D-Bus errors that say there is no bus or no Secret Service on it, or
/// that its collection is locked, by name, and those of a call the service
/// took and failed.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_bus_error_names_a_missing_bus_or_service() {
    use crate::keyring_store::bus_or_service_missing;
    for missing in [
        "org.freedesktop.DBus.Error.NoServer",
        "org.freedesktop.DBus.Error.FileNotFound",
        "org.freedesktop.DBus.Error.NotSupported",
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
        "org.freedesktop.Secret.Error.IsLocked",
    ] {
        assert!(bus_or_service_missing(missing), "{missing}");
    }
    for answered in [
        "org.freedesktop.DBus.Error.NoReply",
        "org.freedesktop.DBus.Error.TimedOut",
        "org.freedesktop.DBus.Error.AccessDenied",
        "org.freedesktop.DBus.Error.UnknownObject",
        "org.freedesktop.DBus.Error.UnknownMethod",
        "org.freedesktop.DBus.Error.InvalidArgs",
        "org.freedesktop.DBus.Error.Failed",
        "org.freedesktop.Secret.Error.NoSuchObject",
        "ServiceUnknown",
        "org.example.Error.ServiceUnknown",
    ] {
        assert!(!bus_or_service_missing(answered), "{answered}");
    }
}

/// The real store of the session. It needs one (an unlocked keychain, a
/// Secret Service), so it runs only when `LADDERBACK_TEST_OS_KEYRING=1`, and
/// touches nothing before that is checked.
#[tokio::test]
async fn the_os_keyring_keeps_and_gives_back_a_secret() {
    if !os_keyring_enabled(std::env::var_os("LADDERBACK_TEST_OS_KEYRING").as_deref()) {
        testkit::skip!("LADDERBACK_TEST_OS_KEYRING is not 1");
    }
    let name = format!("probe-{}", std::process::id());
    let entry = KeyringEntry::parse(&format!("ladderback-test/{name}")).unwrap();
    let secret = SecretRef::Keyring(entry);
    let secrets = SystemSecrets::new();

    // Whatever the machine has, an absent entry is an answer, never a panic.
    let before = secrets.resolve(&secret).await;
    assert!(
        matches!(
            &before,
            Err(SecretError {
                problem: Problem::Missing
                    | Problem::Unreachable { .. }
                    | Problem::KeyringUnavailable { .. },
                ..
            })
        ),
        "{before:?}"
    );

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let entry = keyring::Entry::new("ladderback-test", &name).unwrap();
        entry.set_password("hunter2").unwrap();
        let read = secrets.resolve(&secret).await;
        entry.delete_credential().unwrap();
        assert_eq!(read, gives("hunter2"));
        assert!(matches!(
            secrets.resolve(&secret).await,
            Err(SecretError {
                problem: Problem::Missing,
                ..
            })
        ));
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    testkit::skip!("this platform has no OS keyring");
}

// A file read has the time limit a keyring read has.

#[tokio::test]
async fn a_file_that_cannot_be_read_in_time_is_timed_out() {
    let (release, waiting) = std::sync::mpsc::channel::<()>();
    let waiting = Mutex::new(waiting);
    // A mount that has hung: the read doesn't come back until it is let go.
    let hung: FileReader = Arc::new(move |path, _, _| {
        let _ = waiting.lock().unwrap().recv();
        Err(SecretError::missing(SourceName::File {
            path: path.to_owned(),
        }))
    });
    let result = within(
        Duration::from_secs(2),
        resolver(Duration::from_millis(300))
            .with_file_reader(hung)
            .resolve(&file("/mnt/nas/secret")),
    )
    .await;
    release.send(()).unwrap();
    assert_eq!(
        result,
        Err(SecretError {
            source: SourceName::File {
                path: "/mnt/nas/secret".to_owned()
            },
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
}

// What stops a command that outlives its limit.

/// A command no signal reaches, as one stuck in the kernel is: the limit ends
/// the call, and what the call waits for after it is bounded too.
#[tokio::test]
async fn a_command_that_will_not_die_does_not_hold_the_resolver_up() {
    let lost = |_: Pid, _: Signal| {};
    let stop = Stop {
        grace: Duration::from_millis(300),
        reap_limit: Duration::from_millis(300),
        signal: &lost,
    };
    let sleeping = sh("exec sleep 60", Vec::<String>::new());
    let result = within(
        Duration::from_secs(5),
        run_with(&sleeping, Duration::from_millis(300), &stop),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
}

/// What a command that traps TERM did before and while it was stopped.
struct Trapped {
    result: Result<SecretValue, SecretError>,
    /// What the script wrote to its log, line by line.
    lines: Vec<String>,
    /// From the call to its return.
    elapsed: Duration,
    /// The signals the stop sent to the group, in order, each with whether
    /// the leader was there (not reaped) when it went.
    sent: Vec<(Signal, bool)>,
}

impl Trapped {
    fn wrote(&self, line: &str) -> bool {
        self.lines.iter().any(|written| written == line)
    }
}

/// A stop's signals, sent for real and remembered, each with whether the
/// leader of the group was there (not reaped) when it went.
#[derive(Default)]
struct Signals(Mutex<Vec<(Signal, bool)>>);

impl Signals {
    fn send(&self, group: Pid, signal: Signal) {
        use nix::sys::signal::{kill, killpg};
        // Signal 0 only asks whether the process is there, reaped or not.
        let there = kill(group, None).is_ok();
        self.0.lock().unwrap().push((signal, there));
        let _ = killpg(group, signal);
    }

    fn sent(&self) -> Vec<(Signal, bool)> {
        self.0.lock().unwrap().clone()
    }
}

/// Runs `script` (`$1` is its log) until `limit` and stops it with the real
/// signals and a SIGKILL `grace` after the SIGTERM; a run still going after
/// `bound` fails the test. A command that is still starting when the limit
/// fires is ended by the TERM before it has trapped anything, which reads as
/// a clean stop with nothing written: only a run whose script said `ready`
/// once its trap was in place proves anything, so the others are tried again.
async fn run_trapping(script: &str, limit: Duration, grace: Duration, bound: Duration) -> Trapped {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let command = sh(script, [log.display().to_string()]);
    let mut written = Vec::new();
    for _ in 0..3 {
        fs::write(&log, "").unwrap();
        let signals = Signals::default();
        let stop = Stop {
            grace,
            reap_limit: Duration::from_secs(5),
            signal: &|group, signal| signals.send(group, signal),
        };
        let started = Instant::now();
        let result = within(bound, run_with(&command, limit, &stop)).await;
        let elapsed = started.elapsed();
        written = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        if written.iter().any(|line| line == "ready") {
            return Trapped {
                result,
                lines: written,
                elapsed,
                sent: signals.sent(),
            };
        }
    }
    panic!("the command never reached its trap: {written:?}");
}

fn timed_out() -> Result<SecretValue, SecretError> {
    Err(SecretError {
        source: program("sh"),
        problem: Problem::TimedOut { seconds: 1 },
    })
}

/// The polite signal comes first: a command that handles it ends cleanly, as
/// a helper that holds a lock or a session does, and a limit does not take
/// that from it. Nothing is sent after it, and the call does not wait out a
/// grace the command did not need.
///
/// A group is named by the process id of its leader, and that id is free for
/// another process once the leader has been reaped: the signal went while the
/// leader was there, and no other went after it.
#[tokio::test]
async fn a_timed_out_command_gets_sigterm_before_anything_else() {
    let limit = Duration::from_secs(1);
    let trapped = run_trapping(
        concat!(
            r#"trap 'echo term >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        limit,
        Duration::from_secs(20),
        // Far less than the grace: the call does not wait it out.
        limit + SLACK,
    )
    .await;
    assert_eq!(trapped.result, timed_out());
    assert!(
        trapped.wrote("term"),
        "SIGTERM did not reach the command: {:?}",
        trapped.lines
    );
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
}

/// What a command says on its way out, it can say: a handler that writes to
/// stdout or stderr (a helper that logs that it is closing its session) must
/// find the pipes open, or the SIGPIPE ends it before it is done.
#[tokio::test]
async fn a_command_can_still_write_to_its_output_when_it_gets_sigterm() {
    let trapped = run_trapping(
        concat!(
            r#"trap 'echo closing; echo closing >&2; echo done >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        Duration::from_secs(1),
        Duration::from_secs(20),
        Duration::from_secs(1) + SLACK,
    )
    .await;
    assert_eq!(trapped.result, timed_out());
    assert!(
        trapped.wrote("done"),
        "the handler was cut short: {:?}",
        trapped.lines
    );
}

/// A pipe holds 64 KiB or so. A command that says more than that on its way
/// out is not left blocked on a pipe nobody reads until the SIGKILL: what a
/// command being stopped writes is read on and dropped, so its handler gets to
/// the end. The handler writes with the shell's own `printf`, 300 writes of
/// 1000 bytes to each stream: what `head` or `yes` do with a full pipe differs
/// between coreutils, and the test must not depend on it.
#[tokio::test]
async fn a_command_can_say_more_than_a_pipe_holds_when_it_gets_sigterm() {
    let trapped = run_trapping(
        concat!(
            r#"trap 'n=0; while [ $n -lt 300 ]; do printf "%1000s" ""; printf "%1000s" "" >&2; n=$((n+1)); done; echo done >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_secs(1) + SLACK,
    )
    .await;
    assert_eq!(trapped.result, timed_out());
    assert!(
        trapped.wrote("done"),
        "the handler was blocked on its output: {:?}",
        trapped.lines
    );
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
}

/// A command that ignores SIGTERM is killed once its grace is over: not
/// before, so the polite signal had its time, and not much later, so the
/// call is bounded. Each signal went once, while the leader was there.
#[tokio::test]
async fn a_command_that_ignores_sigterm_is_killed_when_the_grace_is_over() {
    let limit = Duration::from_secs(1);
    let grace = Duration::from_secs(1);
    let trapped = run_trapping(
        concat!(
            r#"trap 'echo term >> "$1"' TERM"#,
            "\n",
            r#"echo "pid $$" >> "$1""#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        limit,
        grace,
        // The SIGKILL ends it at once.
        limit + grace + SLACK,
    )
    .await;
    assert_eq!(trapped.result, timed_out());
    assert!(
        trapped.wrote("term"),
        "SIGTERM did not reach the command: {:?}",
        trapped.lines
    );
    assert_eq!(
        trapped.sent,
        [(Signal::SIGTERM, true), (Signal::SIGKILL, true)]
    );
    assert!(
        trapped.elapsed >= limit + grace,
        "the grace was not given: {:?}",
        trapped.elapsed
    );
    let pid = trapped
        .lines
        .iter()
        .find_map(|line| line.strip_prefix("pid ")?.parse().ok())
        .expect("the script wrote its pid");
    let _reaper = Reaper(pid);
    // Reaped, not only killed: gone from the process table, so the call has
    // waited for it (a zombie still answers signal 0).
    assert!(
        nix::sys::signal::kill(Pid::from_raw(pid), None).is_err(),
        "{pid} was not reaped when the call returned"
    );
    assert!(is_gone_soon(pid).await, "{pid} is still running");
}

/// Waits until the log at `log` has the line `line`, for at most `bound`.
async fn logged(log: &Path, line: &str, bound: Duration) {
    let deadline = Instant::now() + bound;
    loop {
        if fs::read_to_string(log).is_ok_and(|text| text.lines().any(|written| written == line)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no {line:?} in {}",
            log.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A call that is given up on while the command has its grace is not a reason
/// to leave the command running: the drop sends the SIGKILL. The call is
/// dropped once the command has said that it got the SIGTERM; one that ended
/// before it had trapped anything proves nothing and is tried again.
#[tokio::test]
async fn dropping_the_call_during_the_grace_kills_the_command() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let command = sh(
        concat!(
            r#"trap 'echo term >> "$1"' TERM"#,
            "\n",
            r#"echo "pid $$" >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        [log.display().to_string()],
    );
    for attempt in 0..3 {
        fs::write(&log, "").unwrap();
        let signals = Signals::default();
        let stop = Stop {
            grace: Duration::from_secs(60),
            reap_limit: Duration::from_secs(5),
            signal: &|group, signal| signals.send(group, signal),
        };
        {
            let limit = Duration::from_secs(1);
            let running = run_with(&command, limit, &stop);
            tokio::pin!(running);
            tokio::select! {
                result = &mut running => {
                    assert!(attempt < 2, "the command never reached its trap: {result:?}");
                    continue;
                }
                () = logged(&log, "term", limit + SLACK) => {}
            }
            // `running` is dropped here, in the middle of the grace.
        }
        let pid = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("pid ")?.parse().ok())
            .expect("the script wrote its pid");
        let _reaper = Reaper(pid);
        assert_eq!(
            signals.sent(),
            [(Signal::SIGTERM, true), (Signal::SIGKILL, true)]
        );
        assert!(is_gone_soon(pid).await, "{pid} is still running");
        return;
    }
}

/// A command that prints without end is killed at once, with no grace, even
/// when it would ignore a SIGTERM: it is not asked to stop what it is doing
/// wrong.
#[tokio::test]
async fn a_command_that_prints_without_end_gets_no_grace() {
    let signals = Signals::default();
    let stop = Stop {
        grace: Duration::from_secs(60),
        reap_limit: Duration::from_secs(5),
        signal: &|group, signal| signals.send(group, signal),
    };
    // `yes` takes the ignored SIGTERM of the shell that executes it.
    let result = within(
        SLACK,
        run_with(
            &sh("trap '' TERM; exec yes", Vec::<String>::new()),
            Duration::from_secs(60),
            &stop,
        ),
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TooLarge {
                limit: MAX_SECRET_BYTES
            },
        })
    );
    assert_eq!(signals.sent(), [(Signal::SIGKILL, true)]);
}

/// What the grace can't do: a command that ends on the SIGTERM is reaped, and
/// the group is not signalled after that, as its id may be another process's
/// by then. A descendant that ignores the SIGTERM too is left running.
#[tokio::test]
async fn a_descendant_that_ignores_sigterm_outlives_a_command_that_ended_on_it() {
    let trapped = run_trapping(
        concat!(
            "trap '' TERM\n",
            "sleep 300 &\n",
            r#"echo "descendant $!" >> "$1""#,
            "\n",
            r#"trap 'echo term >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        Duration::from_secs(1),
        Duration::from_secs(20),
        Duration::from_secs(1) + SLACK,
    )
    .await;
    let descendant = trapped
        .lines
        .iter()
        .find_map(|line| line.strip_prefix("descendant ")?.parse().ok())
        .expect("the script wrote its descendant's pid");
    let _reaper = Reaper(descendant);
    assert_eq!(trapped.result, timed_out());
    assert!(trapped.wrote("term"), "{:?}", trapped.lines);
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
    assert!(is_running(descendant), "{descendant} was killed");
}

/// The call returns once the killed command is reaped, not when the SIGKILL
/// is sent: here the SIGKILL takes a while to arrive, as it does for a
/// command that is busy in the kernel, and the call waits for it. (The
/// SIGTERM is not delivered at all, so the command is still there when the
/// grace is over.)
#[tokio::test]
async fn the_call_waits_for_a_killed_command_to_be_reaped() {
    let delay = Duration::from_millis(400);
    let (limit, grace) = (Duration::from_millis(300), Duration::from_millis(300));
    let late = |group: Pid, signal: Signal| {
        if signal == Signal::SIGKILL {
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let _ = nix::sys::signal::killpg(group, Signal::SIGKILL);
            });
        }
    };
    let stop = Stop {
        grace,
        reap_limit: Duration::from_secs(5),
        signal: &late,
    };
    let started = Instant::now();
    let result = within(
        limit + grace + delay + SLACK,
        run_with(&sh("sleep 60", Vec::<String>::new()), limit, &stop),
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(result, timed_out());
    assert!(
        elapsed >= limit + grace + delay,
        "the call did not wait for the command: {elapsed:?}"
    );
}

/// A command that ended by itself is reaped: nothing is signalled, as the
/// process id of its group may be another process's by now.
#[tokio::test]
async fn a_command_that_ends_by_itself_is_never_signalled() {
    let signalled = Mutex::new(0_u32);
    let signal = |_: Pid, _: Signal| *signalled.lock().unwrap() += 1;
    let stop = Stop {
        grace: Duration::from_secs(5),
        reap_limit: Duration::from_secs(5),
        signal: &signal,
    };
    let result = within(
        Duration::from_secs(5),
        run_with(
            &sh("echo hunter2", Vec::<String>::new()),
            Duration::from_secs(5),
            &stop,
        ),
    )
    .await;
    assert_eq!(result, Ok(SecretValue::new("hunter2")));
    let failed = within(
        Duration::from_secs(5),
        run_with(
            &sh("exit 3", Vec::<String>::new()),
            Duration::from_secs(5),
            &stop,
        ),
    )
    .await;
    assert!(failed.is_err());
    assert_eq!(*signalled.lock().unwrap(), 0);
}

// Where a file really is decides whose permissions it has.

#[tokio::test]
async fn a_link_out_of_the_credentials_directory_is_warned_about() {
    use std::os::unix::fs::symlink;
    let scratch = TempDir::new().unwrap();
    let credentials = scratch.path().join("credentials");
    let outside = scratch.path().join("outside");
    fs::create_dir_all(&credentials).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let secret = outside.join("secret");
    fs::write(&secret, "hunter2\n").unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
    // A relative link to the file, and a link to the directory it is in.
    symlink("../outside/secret", credentials.join("token")).unwrap();
    symlink("../outside", credentials.join("folder")).unwrap();
    let secrets = resolver(Duration::from_secs(5))
        .with_environment([("CREDENTIALS_DIRECTORY", credentials.display().to_string())]);
    for name in ["token", "folder/secret"] {
        let path = credentials.join(name).display().to_string();
        let resolved = secrets.resolve(&file(&path)).await.unwrap();
        assert_eq!(
            resolved.warnings,
            [SecretWarning::FileReadableByOthers { path }],
            "{name}"
        );
    }
}
