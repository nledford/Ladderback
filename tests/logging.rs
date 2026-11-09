//! The daemon's log, seen from outside: what `steading serve` writes to its
//! stderr, with and without `RUST_LOG`.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const STEADING: &str = env!("CARGO_BIN_EXE_steading");

/// How long a daemon that was just started gets to say where it listens.
const STARTUP: Duration = Duration::from_secs(30);

/// A running `steading serve`: stopped, and waited for, when dropped, however
/// the test ends.
struct Daemon {
    child: Child,
    stderr: Receiver<String>,
}

impl Daemon {
    /// Starts the daemon on a port of the system's choosing, with `RUST_LOG`
    /// set to `rust_log` or, for `None`, not set at all.
    fn start(rust_log: Option<&str>) -> Self {
        let mut command = Command::new(STEADING);
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
        let mut child = command.spawn().expect("steading serve started");
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
                    panic!("steading exited before it listened; stderr: {lines:#?}")
                }
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // SIGTERM is what systemd and launchd send: the daemon stops serving
        // and exits. `kill` sends it without a crate of its own for that.
        let asked = Command::new("kill")
            .arg("-TERM")
            .arg(self.child.id().to_string())
            .status()
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
