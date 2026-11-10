//! What the stop of a hook that outlives its timeout does, and does not do,
//! with the signals sent for real and remembered. The contract the runner
//! meets as a port is in `tests/contract.rs`.

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use app::ports::{HookContext, HookPhase, HookRunner, HookStatus};
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
    let sent = Mutex::new(Vec::new());
    let lost = |_: Pid, signal: Signal| sent.lock().unwrap().push(signal);
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
    // Each signal once, though the hook was still there when the call ended
    // and was dropped.
    assert_eq!(*sent.lock().unwrap(), [Signal::SIGTERM, Signal::SIGKILL]);
}

/// A hook that ignores SIGTERM is killed once its grace is over: not before,
/// so the polite signal had its time, and not much later, so the call is
/// bounded. Each signal went once, while the leader was there, and the hook
/// is reaped, not only killed, when the call returns.
#[tokio::test]
async fn a_hook_that_ignores_sigterm_is_killed_when_the_grace_is_over() {
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
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(trapped.wrote("term"), "{:?}", trapped.lines);
    assert_eq!(
        trapped.sent,
        [(Signal::SIGTERM, true), (Signal::SIGKILL, true)]
    );
    assert!(
        trapped.elapsed >= limit + grace,
        "the grace was not given: {:?}",
        trapped.elapsed
    );
    // The reap after the SIGKILL is bounded by five seconds; the rest is slack.
    assert!(
        trapped.elapsed < limit + grace + Duration::from_secs(15),
        "the call waited {:?}",
        trapped.elapsed
    );
    let pid = pid_in(&trapped.lines);
    let _reaper = Reaper(pid);
    // Reaped: gone from the process table (a zombie still answers signal 0).
    assert!(
        kill(Pid::from_raw(pid), None).is_err(),
        "{pid} was not reaped when the call returned"
    );
}

/// Waits until the log at `log` has the line `line`.
async fn logged(log: &Path, line: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if lines(log).iter().any(|written| written == line) {
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

/// A call that is given up on while the hook has its grace is not a reason to
/// leave the hook running: the drop sends the SIGKILL. The call is dropped
/// once the hook has said that it got the SIGTERM; one that ended before it
/// had trapped anything proves nothing and is tried again.
#[tokio::test]
async fn dropping_the_call_during_the_grace_kills_the_hook() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let entry = sh(
        concat!(
            r#"echo "pid $$" >> "$1""#,
            "\n",
            r#"trap 'echo term >> "$1"' TERM"#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        &log,
    );
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(1));
    let context = context();
    for attempt in 0..3 {
        fs::write(&log, "").unwrap();
        let signals = Signals::default();
        let stop = Stop {
            grace: Duration::from_secs(60),
            reap_limit: Duration::from_secs(5),
            signal: &|group, signal| signals.send(group, signal),
        };
        {
            let running = runner.spawn_and_wait_with(&entry, &context, &stop);
            tokio::pin!(running);
            tokio::select! {
                outcome = &mut running => {
                    assert!(attempt < 2, "the hook never reached its trap: {:?}", outcome.0);
                    continue;
                }
                () = logged(&log, "term") => {}
            }
            // `running` is dropped here, in the middle of the grace.
        }
        let pid = pid_in(&lines(&log));
        let _reaper = Reaper(pid);
        assert_eq!(
            signals.sent(),
            [(Signal::SIGTERM, true), (Signal::SIGKILL, true)]
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while kill(Pid::from_raw(pid), None).is_ok() {
            assert!(Instant::now() < deadline, "{pid} is still running");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        return;
    }
}

/// A hook that ended by itself is reaped: nothing is signalled, as the
/// process id of its group may be another process's by now.
#[tokio::test]
async fn a_hook_that_ends_by_itself_is_never_signalled() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(5));
    let signals = Signals::default();
    let stop = Stop {
        grace: Duration::from_secs(5),
        reap_limit: Duration::from_secs(5),
        signal: &|group, signal| signals.send(group, signal),
    };
    let context = context();
    for (script, status) in [
        ("exit 0", HookStatus::Succeeded),
        ("exit 3", HookStatus::Failed { exit_code: Some(3) }),
    ] {
        let (got, _) = runner
            .spawn_and_wait_with(&sh(script, &log), &context, &stop)
            .await;
        assert_eq!(got, status);
    }
    assert_eq!(signals.sent(), []);
}

/// A hook that ends is reaped when it ends, though a process it started
/// still holds its stdout and stderr open: the call returns soon after the
/// exit, not at the timeout, and signals nothing, as the hook was reaped.
#[tokio::test]
async fn a_hook_that_ends_is_reaped_while_a_descendant_holds_its_output() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let timeout = Duration::from_secs(10);
    let runner = ProcessHookRunner::with_timeout(timeout);
    let signals = Signals::default();
    let stop = Stop {
        grace: Duration::from_secs(5),
        reap_limit: Duration::from_secs(5),
        signal: &|group, signal| signals.send(group, signal),
    };
    let entry = sh(
        concat!("sleep 300 &\n", r#"echo "pid $!" >> "$1""#, "\nexit 0"),
        &log,
    );
    let started = Instant::now();
    let (status, _) = runner.spawn_and_wait_with(&entry, &context(), &stop).await;
    let elapsed = started.elapsed();
    let _reaper = Reaper(pid_in(&lines(&log)));
    assert_eq!(status, HookStatus::Succeeded);
    assert!(
        elapsed < timeout / 2,
        "the call waited for the timeout: {elapsed:?}"
    );
    assert_eq!(signals.sent(), []);
}

/// What the hook wrote before it exited is kept, though a descendant holds
/// the pipes open and nothing ends them: the call reads what is in them and
/// does not wait for an end that does not come.
#[tokio::test]
async fn what_a_hook_wrote_before_it_ended_is_kept_while_a_descendant_holds_its_output() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    let entry = sh(
        concat!(
            "sleep 300 &\n",
            r#"echo "pid $!" >> "$1""#,
            "\necho out; echo err >&2\nexit 3"
        ),
        &log,
    );
    let outcome = runner.run(&entry, &context()).await;
    let _reaper = Reaper(pid_in(&lines(&log)));
    assert_eq!(outcome.status, HookStatus::Failed { exit_code: Some(3) });
    assert_eq!(outcome.stdout.text, "out\n");
    assert_eq!(outcome.stderr.text, "err\n");
    assert!(!outcome.stdout.truncated && !outcome.stderr.truncated);
}

/// What a descendant says once the hook has been reaped is kept, if it says
/// it in the second the pipes are still read: the descendant waits until the
/// hook's pid is gone (`kill -0` still finds a hook that has exited and is
/// not reaped) and a moment more, so its line is written after the reap and
/// only the read that follows the exit can have it.
#[tokio::test]
async fn what_a_descendant_says_after_the_hook_was_reaped_is_kept() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    let entry = sh(
        concat!(
            "hook=$$\n",
            "(while kill -0 $hook 2>/dev/null; do :; done; sleep 0.3; echo late) &\n",
            r#"echo "pid $!" >> "$1""#,
            "\nexit 0"
        ),
        &log,
    );
    let outcome = runner.run(&entry, &context()).await;
    let _reaper = Reaper(pid_in(&lines(&log)));
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert_eq!(outcome.stdout.text, "late\n");
}

/// The pipes are let go when the call returns, though a descendant holds
/// their write ends: what it writes then fails, and nothing reads it. The
/// one that handles SIGPIPE gets a write error; the one that doesn't is
/// ended by the signal, which a child of the runner has at its default.
#[tokio::test]
async fn a_descendant_cannot_write_to_a_hook_s_output_after_the_call_returned() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    // Both wait for the call to return (the test makes `$1.go`) and write.
    let wait = r#"while [ ! -e "$1.go" ]; do sleep 1 & wait $!; done"#;
    let entry = sh(
        &format!(
            concat!(
                "(trap '' PIPE; {wait}\n",
                "echo late 2>/dev/null || echo failed >> \"$1\") &\n",
                r#"echo "pid $!" >> "$1""#,
                "\n(( {wait}\n",
                "echo late ); echo \"status $?\" >> \"$1\") &\n",
                "exit 0"
            ),
            wait = wait
        ),
        &log,
    );
    let outcome = runner.run(&entry, &context()).await;
    let _reaper = Reaper(pid_in(&lines(&log)));
    assert_eq!(outcome.status, HookStatus::Succeeded);
    fs::write(scratch.path().join("log.go"), "").unwrap();
    logged(&log, "failed").await;
    logged(&log, "status 141").await;
}

/// The call returns once the killed hook is reaped, not when the SIGKILL is
/// sent: here the SIGKILL takes a while to arrive, as it does for a hook that
/// is busy in the kernel, and the call waits for it. (The SIGTERM is not
/// delivered at all, so the hook is still there when the grace is over.)
#[tokio::test]
async fn the_call_waits_for_a_killed_hook_to_be_reaped() {
    let delay = Duration::from_millis(400);
    let (timeout, grace) = (Duration::from_millis(300), Duration::from_millis(300));
    let late = |group: Pid, signal: Signal| {
        if signal == Signal::SIGKILL {
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let _ = killpg(group, Signal::SIGKILL);
            });
        }
    };
    let stop = Stop {
        grace,
        reap_limit: Duration::from_secs(5),
        signal: &late,
    };
    let scratch = TempDir::new().unwrap();
    let entry = sh("exec sleep 60", &scratch.path().join("log"));
    let runner = ProcessHookRunner::with_timeout(timeout);
    let started = Instant::now();
    let (status, _) = runner.spawn_and_wait_with(&entry, &context(), &stop).await;
    let elapsed = started.elapsed();
    assert_eq!(status, HookStatus::TimedOut);
    assert!(
        elapsed >= timeout + grace + delay,
        "the call did not wait for the hook: {elapsed:?}"
    );
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

/// A pipe holds 64 KiB or so. A hook that says more than that on its way out
/// is not left blocked on a pipe nobody reads until the SIGKILL: the output
/// of a hook that is being stopped is read on and dropped, so its handler
/// gets to the end. The handler writes with the shell's own `printf`, 300
/// writes of 1000 bytes: what `head` or `yes` do with a full pipe differs
/// between coreutils, and the test must not depend on it.
#[tokio::test]
async fn a_hook_can_say_more_than_a_pipe_holds_when_it_gets_sigterm() {
    let trapped = run_trapping(
        concat!(
            r#"trap 'n=0; while [ $n -lt 300 ]; do printf "%1000s" ""; n=$((n+1)); done; echo done >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        Duration::from_secs(1),
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(
        trapped.wrote("done"),
        "the handler was blocked on its output: {:?}",
        trapped.lines
    );
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
}

/// What the grace can't do: a hook that ends on the SIGTERM is reaped, and
/// the group is not signalled after that, as its id may be another process's
/// by then. A descendant that ignores the SIGTERM too is left running.
#[tokio::test]
async fn a_descendant_that_ignores_sigterm_outlives_a_hook_that_ended_on_it() {
    let trapped = run_trapping(
        concat!(
            "trap '' TERM\n",
            "sleep 300 &\n",
            r#"echo "pid $!" >> "$1""#,
            "\n",
            r#"trap 'echo term >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        Duration::from_secs(1),
        Duration::from_secs(20),
    )
    .await;
    let descendant = pid_in(&trapped.lines);
    let _reaper = Reaper(descendant);
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(trapped.wrote("term"), "{:?}", trapped.lines);
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
    assert!(
        kill(Pid::from_raw(descendant), None).is_ok(),
        "{descendant} was killed"
    );
}
