//! What the stop of a hook that outlives its timeout does, and does not do,
//! with the signals sent for real and remembered. The contract the runner
//! meets as a port is in `tests/contract.rs`.

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use app::ports::{HookContext, HookPhase, HookStatus};
use domain::planning::{HookEntry, OnFailure};
use domain::{OperationId, PlanId, RepositoryAlias};
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use tempfile::TempDir;

use crate::{ProcessHookRunner, Stop};

/// A stop's signals, sent for real and remembered, each with whether the
/// leader of the group was there (not reaped) when it went.
#[derive(Default)]
struct Signals(Mutex<Vec<(Signal, bool)>>);

impl Signals {
    fn send(&self, group: Pid, signal: Signal) {
        // Signal 0 only asks whether the process is there, reaped or not.
        let there = kill(group, None).is_ok();
        self.0.lock().unwrap().push((signal, there));
        let _ = killpg(group, signal);
    }

    fn sent(&self) -> Vec<(Signal, bool)> {
        self.0.lock().unwrap().clone()
    }
}

/// Kills what is left of a process a test started, whatever the test found.
struct Reaper(i32);

impl Drop for Reaper {
    fn drop(&mut self) {
        let _ = kill(Pid::from_raw(self.0), Signal::SIGKILL);
    }
}

fn context() -> HookContext {
    HookContext {
        phase: HookPhase::Before,
        plan: PlanId::parse("demo-plan").unwrap(),
        repository: RepositoryAlias::parse("nas").unwrap(),
        operation: OperationId::from_uuid(uuid::Uuid::from_u128(7)),
        snapshot: None,
        error: None,
    }
}

/// `sh -c script sh log`: the log is `$1` of the script.
fn sh(script: &str, log: &Path) -> HookEntry {
    HookEntry::new(
        "sh",
        ["-c", script, "sh", &log.display().to_string()],
        OnFailure::Error,
    )
    .unwrap()
}

fn lines(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn pid_in(lines: &[String]) -> i32 {
    lines
        .iter()
        .find_map(|line| line.strip_prefix("pid ")?.parse().ok())
        .expect("the script wrote its pid")
}

/// What one run of a hook, stopped at its timeout, did.
struct Trapped {
    status: HookStatus,
    lines: Vec<String>,
    elapsed: Duration,
    sent: Vec<(Signal, bool)>,
}

impl Trapped {
    fn wrote(&self, line: &str) -> bool {
        self.lines.iter().any(|written| written == line)
    }
}

/// Runs `script` (`$1` is its log) until `timeout` and stops it with the real
/// signals and a SIGKILL `grace` after the SIGTERM. A hook that is still
/// starting when the timeout fires is ended by the TERM before it has trapped
/// anything, which reads as a clean stop with nothing written: only a run
/// whose script said `ready` once its trap was in place proves anything, so
/// the others are tried again.
async fn run_trapping(script: &str, timeout: Duration, grace: Duration) -> Trapped {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let entry = sh(script, &log);
    let runner = ProcessHookRunner::with_timeout(timeout);
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
        let (status, _) = runner.spawn_and_wait_with(&entry, &context(), &stop).await;
        let elapsed = started.elapsed();
        written = lines(&log);
        if written.iter().any(|line| line == "ready") {
            return Trapped {
                status,
                lines: written,
                elapsed,
                sent: signals.sent(),
            };
        }
    }
    panic!("the hook never reached its trap: {written:?}");
}

/// What a hook says on its way out, it can say: a handler that writes to
/// stdout or stderr (a hook that logs that it is closing) must find the pipes
/// open, or the SIGPIPE ends it before it is done.
#[tokio::test]
async fn a_hook_can_still_write_to_its_output_when_it_gets_sigterm() {
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
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(
        trapped.wrote("done"),
        "the handler was cut short: {:?}",
        trapped.lines
    );
}

/// A group is named by the process id of its leader, and that id is free for
/// another process once the leader has been reaped: the SIGTERM went while
/// the leader was there, and nothing went after it.
#[tokio::test]
async fn a_hook_that_ends_on_sigterm_is_signalled_once_and_not_after_the_reap() {
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
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(trapped.wrote("term"), "{:?}", trapped.lines);
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
    assert!(
        trapped.elapsed < limit + Duration::from_secs(10),
        "the call waited {:?}",
        trapped.elapsed
    );
}

/// A hook no signal reaches, as one stuck in the kernel is: the timeout ends
/// the call, and what the call waits for after it is bounded too.
#[tokio::test]
async fn a_hook_that_will_not_die_does_not_hold_the_runner_up() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let entry = sh(r#"echo "pid $$" >> "$1"; exec sleep 20"#, &log);
    let runner = ProcessHookRunner::with_timeout(Duration::from_millis(300));
    let lost = |_: Pid, _: Signal| {};
    let stop = Stop {
        grace: Duration::from_millis(300),
        reap_limit: Duration::from_millis(300),
        signal: &lost,
    };
    let started = Instant::now();
    let context = context();
    let running = runner.spawn_and_wait_with(&entry, &context, &stop);
    let outcome = tokio::time::timeout(Duration::from_secs(5), running).await;
    let _reaper = Reaper(pid_in(&lines(&log)));
    let (status, _) = outcome.expect("the call waited for a hook that cannot be killed");
    assert_eq!(status, HookStatus::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "it waited {:?}",
        started.elapsed()
    );
}
