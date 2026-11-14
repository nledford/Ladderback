//! A process a test owns ends, with the process groups its children lead,
//! when its owner is dropped and when the scenario watch stops the run at its
//! limit (#499): what a run started outlives neither, and nothing it started
//! keeps the runner's output pipe open. The test binary plays every part
//! itself, started again with [`ROLE`] naming the part: the runner, a daemon,
//! or a worker that leads a process group of its own, as a daemon's do.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use testkit::children::Owned;
use testkit::scenario_watch::ScenarioWatch;

/// The part the test binary plays when started again: `runner`, `daemon` or
/// `worker`.
const ROLE: &str = "TESTKIT_CHILDREN_ROLE";

/// The runner's scenario limit: short, as the test waits it out.
const LIMIT: Duration = Duration::from_millis(100);

/// How long the parts may take to start, each a process of the test binary,
/// on a loaded machine.
const START: Duration = Duration::from_secs(5);

/// How long a kill may take to end a process and close the pipes it held: it
/// takes milliseconds; the rest allows for a loaded machine.
const KILLED: Duration = Duration::from_secs(1);

/// How long after its scenario began the run must have ended everything it
/// started: the limit, two of the watch's looks (every 250 ms) and the kill.
const ENDED: Duration = Duration::from_millis(100 + 500 + 1_000);

#[test]
fn a_run_stopped_at_its_limit_leaves_no_process_it_started() {
    const TEST: &str = "a_run_stopped_at_its_limit_leaves_no_process_it_started";
    play_role(TEST);
    let mut runner = part(TEST, "runner");
    runner.stdout(Stdio::piped());
    let mut runner = spawn_gate::hold(|| runner.spawn()).expect("the runner starts");
    let runner_pid = pid(runner.id());
    let mut out = Output::read(runner.stdout.take().expect("the runner's stdout"));
    let started = Instant::now();
    let worker = out.pid("worker", started + START);
    let daemon = out.pid("daemon", started + START);
    let _reap = Reap(vec![worker, daemon, runner_pid]);
    let began = Instant::now();
    assert!(
        out.closed_by(began + ENDED),
        "the runner's stdout was still open {ENDED:?} after its scenario began: a process \
         the run started holds it ({:?})",
        out.seen
    );
    let ended = loop {
        if let Some(status) = runner.try_wait().expect("the runner's status") {
            break status;
        }
        assert!(
            Instant::now() < began + ENDED,
            "the runner still ran {ENDED:?} after its scenario began"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(ended.code(), Some(1), "the watch stops the run: {ended}");
    let by = Instant::now() + KILLED;
    assert!(gone_by(daemon, by), "the daemon {daemon} outlived the run");
    assert!(
        gone_by(worker, by),
        "the daemon's worker {worker} outlived the run"
    );
}

#[test]
fn a_dropped_owner_ends_its_process_and_the_groups_its_children_lead() {
    const TEST: &str = "a_dropped_owner_ends_its_process_and_the_groups_its_children_lead";
    play_role(TEST);
    let mut command = part(TEST, "daemon");
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let owned = Owned::spawn(&mut command).expect("the daemon starts");
    let daemon = pid(owned.id());
    let mut out = Output::read(owned.take_stdout().expect("the daemon's stdout"));
    let worker = out.pid("worker", Instant::now() + START);
    let _reap = Reap(vec![worker, daemon]);
    drop(owned);
    let by = Instant::now() + KILLED;
    assert!(
        out.closed_by(by),
        "the daemon's stdout was still open {KILLED:?} after its owner was dropped: its \
         worker holds it"
    );
    assert!(
        gone_by(daemon, by),
        "the daemon {daemon} outlived its owner"
    );
    assert!(
        gone_by(worker, by),
        "the daemon's worker {worker} outlived its owner"
    );
}

/// Plays the part [`ROLE`] names, if any, and then never returns.
fn play_role(test: &str) {
    match std::env::var(ROLE).as_deref() {
        Ok("runner") => runner(test),
        Ok("daemon") => daemon(test),
        Ok("worker") => idle(),
        _ => {}
    }
}

/// The test binary, to be started again to run only `test`, playing `role`.
fn part(test: &str, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("the test binary"));
    command
        .args([test, "--exact", "--nocapture"])
        .env(ROLE, role)
        .stdin(Stdio::null());
    command
}

/// A runner: starts a daemon it owns, which shares its stdout, says the
/// daemon's ID there once the daemon is ready, then begins a scenario that
/// never ends.
fn runner(test: &str) -> ! {
    let watch = ScenarioWatch::start("test runner", LIMIT);
    let mut command = part(test, "daemon");
    command.stderr(Stdio::piped());
    let daemon = Owned::spawn(&mut command).expect("the daemon starts");
    let mut ready = BufReader::new(daemon.take_stderr().expect("the daemon's stderr"));
    let mut line = String::new();
    ready
        .read_line(&mut line)
        .expect("the daemon says it is ready");
    println!("daemon {}", daemon.id());
    watch.began(&daemon, "\"never ends\"".to_owned());
    idle()
}

/// A daemon: starts a worker that leads a process group of its own and
/// shares its stdout, says the worker's ID there, then that it is ready on
/// stderr, and waits for the worker.
fn daemon(test: &str) -> ! {
    let mut command = part(test, "worker");
    command.stderr(Stdio::null()).process_group(0);
    let mut worker = spawn_gate::hold(|| command.spawn()).expect("the worker starts");
    println!("worker {}", worker.id());
    eprintln!("ready");
    // The worker never ends of itself.
    let _ = worker.wait();
    idle()
}

fn idle() -> ! {
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn pid(id: u32) -> Pid {
    Pid::from_raw(i32::try_from(id).expect("a process ID"))
}

/// Whether `pid` is gone by `by`: neither running nor waiting to be reaped.
fn gone_by(pid: Pid, by: Instant) -> bool {
    loop {
        if kill(pid, None) == Err(Errno::ESRCH) {
            return true;
        }
        if Instant::now() >= by {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// The lines of a pipe, read on a thread of their own until every writer has
/// closed it.
struct Output {
    lines: mpsc::Receiver<Option<String>>,
    seen: Vec<String>,
}

impl Output {
    fn read(from: impl Read + Send + 'static) -> Self {
        let (sent, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(from).lines() {
                let Ok(line) = line else { break };
                if sent.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = sent.send(None);
        });
        Self {
            lines,
            seen: Vec::new(),
        }
    }

    /// The process ID a line `<name> <ID>` says, waiting for it until `by`.
    fn pid(&mut self, name: &str, by: Instant) -> Pid {
        loop {
            let said = self.seen.iter().find_map(|line| {
                let id = line.strip_prefix(name)?.strip_prefix(' ')?;
                id.trim().parse::<u32>().ok()
            });
            if let Some(id) = said {
                return pid(id);
            }
            let wait = by.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(wait) {
                Ok(Some(line)) => self.seen.push(line),
                Ok(None) | Err(_) => {
                    panic!("no line `{name} <ID>` came: {:?}", self.seen);
                }
            }
        }
    }

    /// Whether every writer has closed the pipe by `by`.
    fn closed_by(&mut self, by: Instant) -> bool {
        loop {
            let wait = by.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(wait) {
                Ok(Some(line)) => self.seen.push(line),
                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => return true,
                Err(mpsc::RecvTimeoutError::Timeout) => return false,
            }
        }
    }
}

/// Kills what a failed test leaves running, each process with the group it
/// leads, so that a failure doesn't leave them to the machine. A test that
/// passes has seen them gone, and their IDs may be another's by then.
struct Reap(Vec<Pid>);

impl Drop for Reap {
    fn drop(&mut self) {
        if !thread::panicking() {
            return;
        }
        for &pid in &self.0 {
            let _ = killpg(pid, Signal::SIGKILL);
            let _ = kill(pid, Signal::SIGKILL);
        }
    }
}
