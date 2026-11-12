//! The daemon's log, seen from outside: what `ladderback serve` writes to its
//! stderr, with and without `RUST_LOG`.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const LADDERBACK: &str = env!("CARGO_BIN_EXE_ladderback");

/// How long a daemon that was just started gets to say where it listens.
const STARTUP: Duration = Duration::from_secs(30);

/// A running `ladderback serve`: stopped, and waited for, when dropped, however
/// the test ends.
struct Daemon {
    child: Child,
    stderr: Receiver<String>,
    /// The daemon's own config and data folders, removed after it stops.
    _scratch: tempfile::TempDir,
}

impl Daemon {
    /// Starts the daemon on a port of the system's choosing, with `RUST_LOG`
    /// set to `rust_log` or, for `None`, not set at all.
    fn start(rust_log: Option<&str>) -> Self {
        Self::start_with(rust_log, None)
    }

    /// [`Daemon::start`] with `config` written as the daemon's `config.toml`.
    fn start_with(rust_log: Option<&str>, config: Option<&str>) -> Self {
        let scratch = tempfile::tempdir().expect("a scratch folder for the daemon");
        if let Some(config) = config {
            let folder = testkit::daemon::config_dir(scratch.path());
            std::fs::create_dir_all(&folder).expect("the daemon's config folder");
            std::fs::write(folder.join("config.toml"), config).expect("the daemon's config");
        }
        let mut command = Command::new(LADDERBACK);
        // Folders of its own: the daemons of these tests run side by side,
        // and none may open the platform's, the developer's real ones.
        testkit::daemon::isolate(&mut command, scratch.path());
        command
            // Port 0: the system picks a free port, and the daemon says which.
            .args(["serve", "--listen", "127.0.0.1:0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            // Colors would put escape codes inside the lines read here.
            .env("NO_COLOR", "1")
            .env_remove("CLICOLOR_FORCE")
            // The banner is read in English, whatever language the machine
            // or a pseudolocale asks for: `C` makes the program ignore
            // `LANGUAGE`, which is removed too.
            .env("LC_ALL", "C")
            .env_remove("LANGUAGE");
        match rust_log {
            Some(rust_log) => command.env("RUST_LOG", rust_log),
            None => command.env_remove("RUST_LOG"),
        };
        let mut child = spawn_gate::hold(|| command.spawn()).expect("ladderback serve started");
        let stderr = child.stderr.take().expect("a piped stderr");
        // A thread reads the lines, so that waiting for one can have a limit.
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stderr: receiver,
            _scratch: scratch,
        }
    }

    /// The lines on stderr up to and including the one that says where the
    /// daemon listens.
    fn lines_until_it_listens(&self) -> Vec<String> {
        let deadline = Instant::now() + STARTUP;
        let mut lines = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.stderr.recv_timeout(left) {
                Ok(line) => {
                    let listening = line.contains("listening on");
                    lines.push(line);
                    if listening {
                        return lines;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    panic!("no `listening on` line in {STARTUP:?}; stderr so far: {lines:#?}")
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("ladderback exited before it listened; stderr: {lines:#?}")
                }
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // SIGTERM is what systemd and launchd send: the daemon stops serving
        // and exits. `kill` sends it without a crate of its own for that.
        let mut kill = Command::new("kill");
        kill.arg("-TERM").arg(self.child.id().to_string());
        let asked = spawn_gate::hold(|| kill.spawn())
            .and_then(|mut kill| kill.wait())
            .is_ok_and(|status| status.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        while asked && Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // It didn't stop, or couldn't be asked to: it ends now.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What the daemon writes to stderr, up to and including its `listening on`
/// line, when `RUST_LOG` is `rust_log` (`None`: not set). The daemon is
/// stopped before this returns.
fn serve(rust_log: Option<&str>) -> Vec<String> {
    Daemon::start(rust_log).lines_until_it_listens()
}

/// An INFO record saying the daemon is starting, as `env_logger` writes it.
fn is_startup_line(line: &str) -> bool {
    line.contains("INFO") && line.contains("starting")
}

#[test]
fn the_daemon_logs_that_it_is_starting_before_it_listens() {
    let lines = serve(None);
    let startup = lines.iter().position(|line| is_startup_line(line));
    assert!(
        startup.is_some_and(|at| at + 1 < lines.len()),
        "no INFO line about the start before the daemon listened: {lines:#?}"
    );
}

#[test]
fn an_empty_rust_log_leaves_the_default_level_in_place() {
    let lines = serve(Some(""));
    assert!(
        lines.iter().any(|line| is_startup_line(line)),
        "an empty RUST_LOG hid the INFO line: {lines:#?}"
    );
}

#[test]
fn rust_log_info_shows_the_startup_line_and_rust_log_warn_hides_it() {
    // The daemon at `info` shows the line, so its absence at `warn` is down to
    // the filter and not to a logger that was never installed.
    let loud = serve(Some("info"));
    assert!(
        loud.iter().any(|line| is_startup_line(line)),
        "RUST_LOG=info hid the INFO line: {loud:#?}"
    );
    // What the daemon prints for itself, where it listens, is not a record,
    // and no filter touches it: `serve` has it, or has panicked.
    let quiet = serve(Some("warn"));
    assert!(
        !quiet.iter().any(|line| is_startup_line(line)),
        "RUST_LOG=warn let the INFO line through: {quiet:#?}"
    );
}

/// A config with one repository, whose password is `password`.
fn config_with(password: &str) -> String {
    format!("schema = 1\n\n[repositories.demo]\nrepository = \"/srv/demo/repo\"\n{password}\n")
}

#[test]
fn the_daemon_warns_at_load_of_a_password_written_in_its_config_and_never_repeats_it() {
    // Not a real secret.
    let secret = "not-a-real-password";
    let config = config_with(&format!("password = \"{secret}\""));
    let lines = Daemon::start_with(None, Some(&config)).lines_until_it_listens();
    let warned: Vec<_> = lines.iter().filter(|line| line.contains("WARN")).collect();
    assert!(
        warned
            .iter()
            .any(|line| line.contains("demo") && line.contains("password")),
        "no WARN line names the repository and its password: {lines:#?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains(secret)),
        "a line repeats the secret: {lines:#?}"
    );
}

#[test]
fn the_daemon_does_not_warn_of_a_password_it_reads_from_elsewhere() {
    let config = config_with("password-env = \"DEMO_PASSWORD\"");
    let lines = Daemon::start_with(None, Some(&config)).lines_until_it_listens();
    assert!(
        !lines.iter().any(|line| line.contains("WARN")),
        "a WARN line for a password that is no secret in the config: {lines:#?}"
    );
}
