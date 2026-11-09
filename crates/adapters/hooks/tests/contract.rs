//! The real hook runner, processes in a scratch directory, meets the same
//! contract as `app`'s fake. Each hook is a demo shell script that appends
//! to one log file in the scratch dir, which `written` reads back once the
//! runner says the hook ended.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use app::contract::hooks::HooksHarness;
use app::ports::HookRunner;
use app::ports::{HookContext, HookPhase, HookStatus};
use async_trait::async_trait;
use domain::planning::{HookEntry, OnFailure};
use domain::{OperationId, PlanId, RepositoryAlias};
use hooks::ProcessHookRunner;

struct ProcessHooksHarness {
    runner: ProcessHookRunner,
    scratch: tempfile::TempDir,
    log: PathBuf,
    counter: AtomicUsize,
}

impl ProcessHooksHarness {
    fn new() -> Self {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let log = scratch.path().join("log");
        Self {
            runner: ProcessHookRunner::with_timeout(Duration::from_millis(500))
                .with_secret("hunter2-demo"),
            log,
            scratch,
            counter: AtomicUsize::new(0),
        }
    }

    /// A fresh executable script named `name` that runs `body`, with the log
    /// file in `$LOG`, and an entry for it with argument `arg`, if any. The
    /// body's own quoting covers the texts it prints.
    fn script(&self, name: &str, arg: Option<&str>, body: &str) -> HookEntry {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let path = self.scratch.path().join(format!("{name}-{n}.sh"));
        std::fs::write(
            &path,
            format!("#!/bin/sh\nLOG={}\n{body}\n", quote(&self.log)),
        )
        .expect("the script is written");
        make_executable(&path);
        let args = arg.into_iter().map(str::to_owned).collect::<Vec<_>>();
        HookEntry::new(&path.display().to_string(), args, OnFailure::Error)
            .expect("the demo hook is valid")
    }
}

/// Single-quotes a path for the script's assignment.
fn quote(path: &Path) -> String {
    format!("'{}'", path.display())
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("the script is there")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions).expect("the script is executable");
}

#[async_trait]
impl HooksHarness for ProcessHooksHarness {
    fn runner(&self) -> &dyn HookRunner {
        &self.runner
    }

    fn hook_logging(&self, label: &str) -> HookEntry {
        self.script("log", None, &format!("echo 'log: {label}' >> \"$LOG\""))
    }

    fn hook_env(&self, arg: &str) -> HookEntry {
        self.script(
            "env",
            Some(arg),
            concat!(
                r#"for a in "$@"; do printf 'argv: %s\n' "$a" >> "$LOG"; done"#,
                "\n",
                r#"env | sed 's/^/env: /' >> "$LOG""#,
            ),
        )
    }

    fn hook_exits(&self, code: i32, text: &str) -> HookEntry {
        self.script(
            "exits",
            None,
            &format!("printf '%s\n' {text:?} >&2; printf '%s\n' {text:?}; exit {code}"),
        )
    }

    fn hook_hangs(&self) -> HookEntry {
        self.script("hangs", None, "sleep 30")
    }

    fn hook_missing(&self) -> HookEntry {
        HookEntry::new("no-such-demo-hook-program", ["demo"], OnFailure::Error)
            .expect("the demo hook is valid")
    }

    fn hook_prints(&self, text: &str) -> HookEntry {
        self.script("prints", None, &format!("printf '%s\\n' {text:?}"))
    }

    fn secret(&self) -> String {
        "hunter2-demo".to_owned()
    }

    fn leaked_env_var(&self) -> String {
        // Cargo sets this in every integration test, and it is not on the
        // worker's allow-list.
        "CARGO_PKG_NAME".to_owned()
    }

    async fn written(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[tokio::test]
async fn process_hooks_meet_the_hooks_contract() {
    app::contract::hooks::run(&ProcessHooksHarness::new()).await;
}

/// The stop sequence a hook that ignores SIGTERM is killed by: TERM first
/// (the hook's trap fires), then SIGKILL after the 5 s grace. A plain
/// `sleep` hook dies on the first signal, so it cannot tell the two apart.
/// The trap only holds once the shell has reached it: under load the
/// process can still be starting when the timeout fires, and the group's
/// TERM kills it before it ever traps, which reads as a clean stop with no
/// marker. So the script says when its trap is in place, the runner gets a
/// timeout of its own, and only a `ready` attempt counts; the markers, not
/// the clock, say what happened.
#[tokio::test]
async fn a_hook_that_ignores_term_is_term_then_kill_stopped() {
    let harness = ProcessHooksHarness::new();
    let hook = harness.script(
        "ignores-term",
        None,
        concat!(
            r#"echo started >> "$LOG""#,
            "\n",
            r#"trap 'echo term >> "$LOG"' TERM"#,
            "\n",
            r#"echo ready >> "$LOG""#,
            "\n",
            "while :; do sleep 1 & wait $!; done",
        ),
    );
    let context = HookContext {
        phase: HookPhase::Before,
        plan: PlanId::parse("demo-plan").expect("the demo plan parses"),
        repository: RepositoryAlias::parse("nas").expect("the demo alias parses"),
        operation: OperationId::from_uuid(uuid::Uuid::from_u128(7)),
        snapshot: None,
        error: None,
    };
    // A whole process to start, on a runner that may be loaded: the
    // harness's half second is not always enough for the shell to reach
    // its trap.
    let runner = ProcessHookRunner::with_timeout(Duration::from_secs(5));
    for attempt in 0..3 {
        std::fs::write(&harness.log, "").expect("the log is cleared");
        let started = std::time::Instant::now();
        let outcome = runner.run(&hook, &context).await;
        let elapsed = started.elapsed();
        assert_eq!(outcome.status, HookStatus::TimedOut);
        let lines = harness.written().await;
        if lines.iter().any(|line| line == "ready") {
            // The trap was in place before the timeout could fire, so the
            // marker says what really reached the hook.
            assert!(
                lines.iter().any(|line| line == "term"),
                "SIGTERM reached the group before the kill: {lines:?}"
            );
            assert!(
                elapsed >= Duration::from_secs(5) && elapsed < Duration::from_secs(30),
                "TERM, then the 5 s grace, then KILL: {elapsed:?}"
            );
            return;
        }
        // Still starting when the timeout fired; nothing was proven, so
        // nothing is asserted. Try again while attempts remain.
        assert!(attempt < 2, "the hook never reached its trap: {lines:?}");
    }
}
