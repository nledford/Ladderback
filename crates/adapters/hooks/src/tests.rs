//! What the stop of a hook that outlives its timeout does, and does not do,
//! with the signals sent for real and remembered. The contract the runner
//! meets as a port is in `tests/contract.rs`.
//!
//! A runner that never stops a hook must fail the test that waits for it, not
//! hang the run: each wait on a stop has a bound of its own (`within`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use app::ports::{HookContext, HookPhase, HookRunner, HookStatus, MAX_HOOK_OUTPUT_BYTES};
use domain::planning::{HookEntry, OnFailure};
use domain::{OperationId, PlanId, RepositoryAlias};
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use tempfile::TempDir;

use crate::{ProcessHookRunner, READ_CHUNK, Stop, read_capped, signal_group};

/// A stop's signals, sent for real, by the runner's own `signal_group`, and
/// remembered, each with whether the leader of the group was there (not
/// reaped) when it went.
#[derive(Default)]
struct Signals(Mutex<Vec<(Signal, bool)>>);

impl Signals {
    fn send(&self, group: Pid, signal: Signal) {
        // Signal 0 only asks whether the process is there, reaped or not.
        let there = kill(group, None).is_ok();
        self.0.lock().unwrap().push((signal, there));
        signal_group(group, signal);
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

/// The longest a call may take to stop a hook that ends on its SIGTERM, with
/// a timeout of a second, which takes about that: the signal, the hook's own
/// exit and slack for a loaded machine. No longer, so that a mutant that
/// fails every test at its bound still ends inside cargo-mutants' limit on a
/// runner with few cores.
const ENDS_ON_TERM: Duration = Duration::from_secs(6);

/// A loop a hook starts in the background: it writes a line to `$1.ticks`
/// every 50 ms for as long as `$1`, the log, is there, so one that a test
/// leaves running by mistake ends with the test's scratch directory. The
/// script moves on once the loop has ticked: it runs.
const TICKER: &str = concat!(
    ": > \"$1.ticks\"\n",
    "(while [ -e \"$1\" ]; do echo tick >> \"$1.ticks\"; sleep 0.05; done) &\n",
    "until [ -s \"$1.ticks\" ]; do sleep 0.01; done\n",
);

/// Awaits `call` for at most `bound` and fails the test when it is still
/// running then: a hook that is never stopped is a test that fails, not one
/// that waits for something else to give up on it.
async fn within<T>(bound: Duration, call: impl Future<Output = T>) -> T {
    tokio::time::timeout(bound, call)
        .await
        .unwrap_or_else(|_| panic!("the call was still running after {bound:?}"))
}

/// Whether the ticks of the hook with the log `log` have stopped: their
/// count no longer grows. What a stop ends is gone moments after the call
/// returns, not at its return, so this looks for a few seconds. The loop has
/// ticked, or there is nothing to have outlived the stop.
async fn ticks_stop(log: &Path) -> bool {
    let ticks = PathBuf::from(format!("{}.ticks", log.display()));
    let count = || lines(&ticks).len();
    assert!(count() > 0, "the hook's loop never ticked");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let before = count();
        tokio::time::sleep(Duration::from_millis(150)).await;
        if count() == before {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
    }
}

/// What one run of a hook, stopped at its timeout, did.
struct Trapped {
    status: HookStatus,
    lines: Vec<String>,
    elapsed: Duration,
    sent: Vec<(Signal, bool)>,
    /// Holds the hook's files until the test is over.
    scratch: TempDir,
}

impl Trapped {
    fn wrote(&self, line: &str) -> bool {
        self.lines.iter().any(|written| written == line)
    }

    /// The log the hook wrote to, `$1` of its script.
    fn log(&self) -> PathBuf {
        self.scratch.path().join("log")
    }
}

/// Runs `script` (`$1` is its log) until `timeout` and stops it with the real
/// signals and a SIGKILL `grace` after the SIGTERM. A hook that is still
/// starting when the timeout fires is ended by the TERM before it has trapped
/// anything, which reads as a clean stop with nothing written: only a run
/// whose script said `ready` once its trap was in place proves anything, so
/// the others are tried again. A call still running after `bound` fails the
/// test.
async fn run_trapping(
    script: &str,
    timeout: Duration,
    grace: Duration,
    bound: Duration,
) -> Trapped {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let entry = sh(script, &log);
    let context = context();
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
        let call = runner.spawn_and_wait_with(&entry, &context, &stop);
        let (status, _) = within(bound, call).await;
        let elapsed = started.elapsed();
        written = lines(&log);
        if written.iter().any(|line| line == "ready") {
            return Trapped {
                status,
                lines: written,
                elapsed,
                sent: signals.sent(),
                scratch,
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
        ENDS_ON_TERM,
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
    let trapped = run_trapping(
        concat!(
            r#"trap 'echo term >> "$1"; exit 0' TERM"#,
            "\n",
            r#"echo ready >> "$1""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
        Duration::from_secs(1),
        Duration::from_secs(20),
        ENDS_ON_TERM,
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(trapped.wrote("term"), "{:?}", trapped.lines);
    assert_eq!(trapped.sent, [(Signal::SIGTERM, true)]);
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
        // The reap after the SIGKILL is bounded by five seconds; one more is slack.
        limit + grace + Duration::from_secs(6),
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
    let deadline = Instant::now() + Duration::from_secs(5);
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

/// The call lets go of the hook's pipes only while no start is in flight:
/// a start (another hook's, a worker's) can copy every descriptor of the
/// daemon into a child that has not yet run `exec`, close-on-exec or not,
/// and holds the spawn gate until it has. A copy of the read end that
/// outlived the call would let a descendant's late write go through (#234).
/// Here a start holds the gate before the hook may end (it waits for a file
/// the test makes only then); the call returns only once it has let go.
#[tokio::test]
async fn the_call_lets_go_of_the_pipes_only_while_no_start_is_in_flight() {
    const HELD: Duration = Duration::from_secs(1);
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let go = scratch.path().join("log.go");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    let entry = sh(
        r#"echo started >> "$1"; while [ ! -e "$1.go" ]; do sleep 0.01; done"#,
        &log,
    );
    let context = context();
    let start = async {
        logged(&log, "started").await;
        let start = testkit::inheritable::start_in_flight(HELD);
        std::fs::write(&go, "").unwrap();
        start
    };
    let (outcome, start) = tokio::join!(
        within(Duration::from_secs(5), runner.run(&entry, &context)),
        start
    );
    let returned = Instant::now();
    let let_go = start.join().unwrap();
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert!(
        returned >= let_go,
        "the call let go of the pipes while a start held the gate"
    );
}

/// A hook inherits no descriptor that another start in the daemon has open:
/// it starts through the spawn gate, so it waits while a pipe that is not
/// yet close-on-exec is open, as a worker's, another hook's or a password
/// command's pipes are for a moment where the platform has no `pipe2`
/// (#234). A hook that held another's pipe would keep its reader from the
/// end, or its writer from the SIGPIPE.
#[tokio::test]
async fn a_hook_inherits_no_pipe_another_start_has_open() {
    let pipe = testkit::inheritable::InheritablePipe::open();
    let [read, write] = pipe.numbers();
    let entry = HookEntry::new(
        "sh",
        ["-c", testkit::inheritable::INHERITED, "sh", &read, &write],
        OnFailure::Error,
    )
    .unwrap();
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    let outcome = within(Duration::from_secs(5), runner.run(&entry, &context())).await;
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert_eq!(
        outcome.stdout.text, "",
        "the hook inherited these descriptors"
    );
}

/// A hook waits for the spawn gate off the runtime's threads: a start whose
/// `exec` stalls (a script on a hung mount, a first scan of a new program)
/// holds the gate, and a call that waited for it on a runtime thread would
/// stop every other task of that thread meanwhile. Here the gate is let go
/// by a task of the same runtime, which has one thread only, so it can be
/// only while the call waits elsewhere.
#[tokio::test]
async fn a_hook_waits_for_the_gate_off_the_runtime() {
    let (release, stalled) = testkit::inheritable::stalled_start(Duration::from_secs(2));
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    let entry = HookEntry::new("sh", ["-c", "exit 0"], OnFailure::Error).unwrap();
    let context = context();
    let let_go = async {
        tokio::task::yield_now().await;
        let _ = release.send(());
    };
    let (outcome, ()) = tokio::join!(runner.run(&entry, &context), let_go);
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert!(
        stalled.join().unwrap(),
        "the call waited for the gate on the runtime's thread"
    );
}

/// The call waits for the gate to let go of the hook's pipes off the
/// runtime's threads too. Here a stalled start holds the gate before the
/// hook may end, and a task of the same one-thread runtime lets it go a
/// while after: it can only once the call waits elsewhere. (Should the hook
/// take longer than that to end, the test passes without showing anything.)
#[tokio::test]
async fn the_call_waits_to_let_go_of_the_pipes_off_the_runtime() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let go = scratch.path().join("log.go");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10));
    let entry = sh(
        r#"echo started >> "$1"; while [ ! -e "$1.go" ]; do sleep 0.01; done"#,
        &log,
    );
    let context = context();
    let let_go = async {
        logged(&log, "started").await;
        let (release, stalled) = testkit::inheritable::stalled_start(Duration::from_secs(2));
        std::fs::write(&go, "").unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = release.send(());
        stalled
    };
    let (outcome, stalled) = tokio::join!(runner.run(&entry, &context), let_go);
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert!(
        stalled.join().unwrap(),
        "the call waited to let go of the pipes on the runtime's thread"
    );
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
    let context = context();
    let started = Instant::now();
    let call = runner.spawn_and_wait_with(&entry, &context, &stop);
    let (status, _) = within(Duration::from_secs(5), call).await;
    let elapsed = started.elapsed();
    assert_eq!(status, HookStatus::TimedOut);
    assert!(
        elapsed >= timeout + grace + delay,
        "the call did not wait for the hook: {elapsed:?}"
    );
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
        ENDS_ON_TERM,
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
        ENDS_ON_TERM,
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

/// A hook's process group holds what the hook started, and a stop is for the
/// group, not the hook alone: the SIGTERM that ends a hook at its timeout
/// ends what it started too. That is watched by what the loop writes
/// (`TICKER`), not by a pid, so the test does not depend on who reaps an
/// orphan.
#[tokio::test]
async fn what_a_hook_started_is_stopped_with_it_at_its_timeout() {
    let trapped = run_trapping(
        &format!("{TICKER}echo ready >> \"$1\"\nwait"),
        Duration::from_secs(1),
        Duration::from_secs(20),
        ENDS_ON_TERM,
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(
        ticks_stop(&trapped.log()).await,
        "what the hook started is still running"
    );
}

/// The SIGKILL, once the grace is over, is for the group too: the hook and
/// what it started all ignore the SIGTERM, and are all gone after it.
#[tokio::test]
async fn what_ignores_sigterm_is_killed_with_the_hook_when_the_grace_is_over() {
    let limit = Duration::from_secs(1);
    let grace = Duration::from_secs(1);
    let trapped = run_trapping(
        &format!("trap '' TERM\n{TICKER}echo ready >> \"$1\"\nwhile :; do sleep 1 & wait $!; done"),
        limit,
        grace,
        limit + grace + Duration::from_secs(6),
    )
    .await;
    assert_eq!(trapped.status, HookStatus::TimedOut);
    assert!(
        ticks_stop(&trapped.log()).await,
        "what the hook started is still running"
    );
}

/// A call that is given up on while the hook runs ends the hook's group, and
/// what it started with it: `kill_on_drop` reaches the hook alone.
#[tokio::test]
async fn dropping_the_call_stops_what_the_hook_started() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    // The loop of `TICKER` runs for as long as the log is there.
    fs::write(&log, "").unwrap();
    let entry = sh(&format!("{TICKER}echo ready >> \"$1\"\nwait"), &log);
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(60));
    let context = context();
    let stop = Stop {
        grace: Duration::from_secs(60),
        reap_limit: Duration::from_secs(5),
        signal: &signal_group,
    };
    {
        let running = runner.spawn_and_wait_with(&entry, &context, &stop);
        tokio::pin!(running);
        tokio::select! {
            outcome = &mut running => panic!("the hook ended by itself: {:?}", outcome.0),
            () = logged(&log, "ready") => {}
        }
        // `running` is dropped here, with the hook running.
    }
    assert!(
        ticks_stop(&log).await,
        "what the hook started is still running"
    );
}

/// A runner built with a timeout stops a hook at it, not at the default of
/// five minutes: this hook would run for a minute.
#[tokio::test]
async fn a_runner_built_with_a_timeout_stops_the_hook_at_it() {
    let scratch = TempDir::new().unwrap();
    let entry = sh("exec sleep 60", &scratch.path().join("log"));
    let timeout = Duration::from_millis(300);
    let runner = ProcessHookRunner::with_timeout(timeout);
    let started = Instant::now();
    let outcome = within(Duration::from_secs(5), runner.run(&entry, &context())).await;
    assert_eq!(outcome.status, HookStatus::TimedOut);
    assert!(
        started.elapsed() >= timeout,
        "the hook was stopped before its timeout: {:?}",
        started.elapsed()
    );
}

/// What `read_capped` keeps of `input` when it keeps at most `keep` bytes,
/// whether it marks the stream cut, and how much of `input` it left unread.
async fn read_capped_from(mut input: &[u8], keep: usize) -> (Vec<u8>, bool, usize) {
    let mut kept = Vec::new();
    let mut truncated = false;
    read_capped(&mut input, &mut kept, keep, &mut truncated)
        .await
        .unwrap();
    (kept, truncated, input.len())
}

/// A stream that just fills the cap lost nothing, whatever the reads it came
/// in: the last of them ends exactly at the cap.
#[tokio::test]
async fn a_stream_that_just_fills_the_cap_is_kept_whole_and_not_marked_cut() {
    let keep = 2 * READ_CHUNK + 5;
    let (kept, truncated, _) = read_capped_from(&vec![b'a'; keep], keep).await;
    assert_eq!(kept.len(), keep);
    assert!(!truncated);
}

/// A read that crosses the cap keeps what is under it, drops the rest and
/// marks the stream cut.
#[tokio::test]
async fn a_read_that_crosses_the_cap_keeps_what_is_under_it_and_marks_the_stream_cut() {
    let (kept, truncated, unread) = read_capped_from(b"abcdefghijk", 10).await;
    assert_eq!(kept, b"abcdefghij");
    assert!(truncated);
    assert_eq!(unread, 0);
}

/// What comes after the cap is full is read to the end and dropped, so the
/// writer is never blocked, and the stream is marked cut.
#[tokio::test]
async fn what_comes_after_the_cap_is_full_is_read_and_dropped() {
    let (kept, truncated, unread) = read_capped_from(&vec![b'a'; 3 * READ_CHUNK + 7], 10).await;
    assert_eq!(kept, [b'a'; 10]);
    assert!(truncated);
    assert_eq!(unread, 0);
}

/// Of each stream a hook writes, the cap is what is kept, and a stream is
/// marked cut only when it lost something: one of exactly the cap is whole,
/// and one byte more is cut.
#[tokio::test]
async fn the_output_of_a_hook_of_exactly_the_cap_is_whole_and_one_byte_more_is_cut() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(30));
    for (written, cut) in [
        (MAX_HOOK_OUTPUT_BYTES, false),
        (MAX_HOOK_OUTPUT_BYTES + 1, true),
    ] {
        let entry = sh(
            &format!("printf '%{written}s' ''\nprintf '%{written}s' '' >&2"),
            &log,
        );
        let outcome = within(Duration::from_secs(25), runner.run(&entry, &context())).await;
        assert_eq!(outcome.status, HookStatus::Succeeded);
        for (name, stream) in [("stdout", &outcome.stdout), ("stderr", &outcome.stderr)] {
            assert_eq!(
                stream.text.len(),
                MAX_HOOK_OUTPUT_BYTES,
                "{name} of {written} bytes"
            );
            assert_eq!(stream.truncated, cut, "{name} of {written} bytes");
        }
    }
}

/// Output is masked from where it begins a secret, cut at the cap or not:
/// what was read doesn't say who stopped it (I4, #232). An end that begins
/// no secret is kept, and `truncated` is the cap's alone.
#[test]
fn output_is_masked_where_it_ends_like_a_secret_begins_whoever_cut_it() {
    let redactor =
        domain::secrets::Redactor::new(&[domain::secrets::SecretValue::new("hunter2-demo")]);
    let captured = crate::Captured::default();
    for truncated in [false, true] {
        let masked = captured.stream(b"done h", truncated, &redactor);
        assert_eq!(masked.text, "done ***");
        assert_eq!(masked.truncated, truncated);
        assert_eq!(
            captured.stream(b"done x", truncated, &redactor).text,
            "done x"
        );
    }
}

/// The secret one write was carrying when the read of a pipe stopped. A
/// write larger than the pipe takes at once reaches the reader in parts, and
/// the kernel picks where: the hooks below write the start of the secret and
/// leave the rest unsaid, which is where such a split leaves the reader.
const SPLIT_SECRET: &str = "hunter2-canary";

/// A hook that ends while a process it started holds its pipes open is read
/// for the grace and then left: what was read can end partway through a
/// secret, and it is masked as a cut text (I4).
#[tokio::test]
async fn output_a_descendant_holds_open_past_the_grace_is_masked_as_cut() {
    let scratch = TempDir::new().unwrap();
    let log = scratch.path().join("log");
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10)).with_secret(SPLIT_SECRET);
    let entry = sh(
        concat!(
            "sleep 300 &\n",
            r#"echo "pid $!" >> "$1""#,
            "\nprintf 'said hunter2-'; printf 'said hunter2-' >&2\nexit 0"
        ),
        &log,
    );
    let outcome = within(Duration::from_secs(8), runner.run(&entry, &context())).await;
    let _reaper = Reaper(pid_in(&lines(&log)));
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert_eq!(outcome.stdout.text, "said ***");
    assert_eq!(outcome.stderr.text, "said ***");
}

/// A hook stopped at its timeout is read no further: what was read can end
/// partway through a secret, and it is masked as a cut text (I4). A hook
/// that a loaded machine hasn't let write before the timeout proves nothing,
/// so a run with a stream still empty is tried again.
#[tokio::test]
async fn output_of_a_hook_stopped_at_its_timeout_is_masked_as_cut() {
    let scratch = TempDir::new().unwrap();
    let entry = sh(
        "printf 'said hunter2-'; printf 'said hunter2-' >&2\nexec sleep 60",
        &scratch.path().join("log"),
    );
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(1)).with_secret(SPLIT_SECRET);
    for _ in 0..3 {
        let outcome = within(ENDS_ON_TERM, runner.run(&entry, &context())).await;
        assert_eq!(outcome.status, HookStatus::TimedOut);
        let texts = [&outcome.stdout.text, &outcome.stderr.text];
        for text in texts {
            assert!(
                text.is_empty() || text == "said ***",
                "the start of the secret is in {text:?}"
            );
        }
        if texts.iter().all(|text| !text.is_empty()) {
            return;
        }
    }
    panic!("the hook's output was never read before its timeout");
}

/// A hook can cut its own output short: it stops partway through a secret
/// and exits on its own, with no timeout, no failed read and no newline at
/// the end, as one that a signal Ladderback didn't send ended. What was read
/// is masked as a cut text all the same, whoever stopped the stream, and
/// nothing of the secret shows, though the hook failed (I4, #232). Output
/// that ends nowhere near a secret is kept as it was said.
#[tokio::test]
async fn output_a_hook_cut_short_itself_leaves_nothing_of_a_secret() {
    let scratch = TempDir::new().unwrap();
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(10)).with_secret(SPLIT_SECRET);
    let plain = sh(
        "printf 'said hello\\n'; printf 'said hello' >&2",
        &scratch.path().join("plain"),
    );
    let outcome = within(Duration::from_secs(5), runner.run(&plain, &context())).await;
    assert_eq!(outcome.status, HookStatus::Succeeded);
    assert_eq!(outcome.stdout.text, "said hello\n");
    assert_eq!(outcome.stderr.text, "said hello");
    let cut = sh(
        "printf 'said hunter2-'; printf 'said hunter2-' >&2\nexit 3",
        &scratch.path().join("cut"),
    );
    let outcome = within(Duration::from_secs(5), runner.run(&cut, &context())).await;
    assert_eq!(outcome.status, HookStatus::Failed { exit_code: Some(3) });
    assert_eq!(outcome.stdout.text, "said ***");
    assert_eq!(outcome.stderr.text, "said ***");
}
