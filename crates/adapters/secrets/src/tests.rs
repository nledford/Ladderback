//! The app's secrets contract against the real adapter, and what the
//! contract can't state: process groups, limits, literal arguments, special
//! files and the OS keyring.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app::contract::secrets::SecretsHarness;
use app::ports::{Resolved, SecretResolver};
use domain::secrets::{
    CommandLine, FileRead, KeyringEntry, MAX_SECRET_BYTES, Problem, SecretError, SecretRef,
    SecretValue, SecretWarning, SourceName,
};
use nix::unistd::Pid;
use tempfile::TempDir;

use crate::command::{Stop, run_with};
use crate::keyring_store::{Failure, KeyringStore, Memory};
use crate::{FileReader, SystemSecrets};

// ---------------------------------------------------------------- helpers

/// The contract's harness over a scratch directory, the real environment
/// reader and files, real commands and a keyring in memory.
struct RealHarness {
    directory: TempDir,
    environment: Mutex<BTreeMap<String, String>>,
    keyring: Arc<Memory>,
    commands: AtomicUsize,
}

impl RealHarness {
    fn new() -> Self {
        Self {
            directory: TempDir::new().unwrap(),
            environment: Mutex::default(),
            keyring: Arc::default(),
            commands: AtomicUsize::new(0),
        }
    }

    fn path(&self, name: &str) -> String {
        self.directory.path().join(name).display().to_string()
    }

    fn next(&self) -> usize {
        self.commands.fetch_add(1, Ordering::Relaxed)
    }
}

impl SecretsHarness for RealHarness {
    fn resolver(&self) -> Arc<dyn SecretResolver> {
        let environment = self.environment.lock().unwrap().clone();
        Arc::new(
            SystemSecrets::new()
                .with_environment(environment)
                .with_keyring(self.keyring.clone())
                .with_command_timeout(Duration::from_millis(500)),
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
/// commands `limit`.
fn resolver(limit: Duration) -> SystemSecrets {
    SystemSecrets::new()
        .with_environment(Vec::<(String, String)>::new())
        .with_keyring(Arc::new(Memory::default()))
        .with_command_timeout(limit)
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
    let deadline = Instant::now() + Duration::from_secs(10);
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_running(pid) {
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}

// --------------------------------------------------------------- contract

#[tokio::test]
async fn the_system_resolver_meets_the_secrets_contract() {
    app::contract::secrets::run(&RealHarness::new()).await;
}

// --------------------------------------------------------------- commands

#[tokio::test]
async fn a_timed_out_command_leaves_nothing_of_its_group_running() {
    let directory = TempDir::new().unwrap();
    let pidfile = directory.path().join("pid");
    let script = sh(
        r#"sleep 60 & echo $! > "$1"; wait"#,
        [pidfile.display().to_string()],
    );
    let result = resolver(Duration::from_secs(1))
        .resolve(&command(script))
        .await;
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
        let result = resolver(limit)
            .resolve(&command(sh("sleep 60", Vec::<String>::new())))
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
    let started = Instant::now();
    // A minute is far more than this test waits for: the size ends it.
    let result = resolver(Duration::from_secs(60))
        .resolve(&command(script))
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
    assert!(started.elapsed() < Duration::from_secs(10));
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
    let result = resolver(Duration::from_secs(30))
        .resolve(&command(script))
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
        let result = resolver(Duration::from_secs(30))
            .resolve(&command(sh(r#"printf %s "$1""#, [&argument])))
            .await;
        assert_eq!(result, gives(&argument));
    }
    assert!(!marker.exists());
}

#[tokio::test]
async fn a_command_gets_no_input() {
    let started = Instant::now();
    // `cat` with no arguments copies its input until it ends.
    let result = resolver(Duration::from_secs(30))
        .resolve(&command(
            CommandLine::new("cat", Vec::<String>::new()).unwrap(),
        ))
        .await;
    assert_eq!(result, Err(SecretError::missing(program("cat"))));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn a_command_that_is_killed_by_a_signal_has_no_status() {
    let result = resolver(Duration::from_secs(30))
        .resolve(&command(sh("kill -9 $$", Vec::<String>::new())))
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
    let result = resolver(Duration::from_secs(30))
        .resolve(&command(sh(
            r#"head -c 1048576 /dev/zero | tr '\0' x >&2; exit 2"#,
            Vec::<String>::new(),
        )))
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
        let result = resolver(Duration::from_secs(30))
            .resolve(&command(line))
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

#[tokio::test]
async fn a_fifo_is_not_read_and_does_not_block() {
    use nix::sys::stat::Mode;
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("pipe");
    nix::unistd::mkfifo(&path, Mode::S_IRWXU).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        resolver(Duration::from_secs(1)).resolve(&file(&path)),
    )
    .await
    .expect("opening a FIFO blocked");
    assert!(
        matches!(&result, Err(SecretError { problem: Problem::Unreachable { .. }, source })
            if *source == SourceName::File { path: path.display().to_string() }),
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
fn os_keyring_enabled(_setting: Option<&std::ffi::OsStr>) -> bool {
    todo!()
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
                problem: Problem::Missing | Problem::Unreachable { .. },
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
    let started = Instant::now();
    let result = resolver(Duration::from_millis(300))
        .with_file_reader(hung)
        .resolve(&file("/mnt/nas/secret"))
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
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "it waited {:?}",
        started.elapsed()
    );
}

// What stops a command that outlives its limit.

/// A command no signal reaches, as one stuck in the kernel is: the limit ends
/// the call, and what the call waits for after it is bounded too.
#[tokio::test]
async fn a_command_that_will_not_die_does_not_hold_the_resolver_up() {
    let lost = |_: Pid| {};
    let stop = Stop {
        reap_limit: Duration::from_millis(300),
        kill: &lost,
    };
    let started = Instant::now();
    let sleeping = sh("exec sleep 60", Vec::<String>::new());
    let running = run_with(&sleeping, Duration::from_millis(300), &stop);
    let result = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("the call waited for a command that cannot be killed");
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "it waited {:?}",
        started.elapsed()
    );
}

/// A process group is named by the process id of its leader, and that id is
/// free for another process once the leader has been reaped.
#[tokio::test]
async fn the_group_is_signalled_once_and_only_while_its_leader_is_unreaped() {
    use nix::sys::signal::{Signal, kill, killpg};
    let leader_there = Mutex::new(Vec::new());
    let signal = |group: Pid| {
        // Signal 0 only asks whether the process is there, reaped or not.
        leader_there.lock().unwrap().push(kill(group, None).is_ok());
        let _ = killpg(group, Signal::SIGKILL);
    };
    let stop = Stop {
        reap_limit: Duration::from_secs(5),
        kill: &signal,
    };
    let result = run_with(
        &sh("sleep 60", Vec::<String>::new()),
        Duration::from_millis(300),
        &stop,
    )
    .await;
    assert_eq!(
        result,
        Err(SecretError {
            source: program("sh"),
            problem: Problem::TimedOut { seconds: 1 },
        })
    );
    assert_eq!(*leader_there.lock().unwrap(), [true]);
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
