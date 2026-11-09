//! The [`HookRunner`](crate::ports::HookRunner) contract: the same checks
//! run against the in-memory fake and the real process-spawning adapter.
//! A check fails on the first violation, as the other suites do.

use domain::planning::HookEntry;
use domain::{OperationId, PlanId, RepositoryAlias, SnapshotId, Uuid};

use crate::ports::{HookContext, HookPhase, HookRunner, HookStatus, MAX_HOOK_OUTPUT_BYTES};

/// Sets up hooks for the runner under test: in memory for the fake, as demo
/// scripts in a scratch directory for the real one, so the same checks run
/// against both.
#[async_trait::async_trait]
pub trait HooksHarness: Send + Sync {
    /// The runner under test, built with a timeout short enough for the
    /// hangs check.
    fn runner(&self) -> &dyn HookRunner;
    /// A hook that, when it runs, appends `label` to what [`written`](Self::written)
    /// returns.
    fn hook_logging(&self, label: &str) -> HookEntry;

    /// when it runs, appends each of its argv words as `argv: <word>` and
    /// each of its environment variables as `env: NAME=VALUE` to what
    /// [`written`](Self::written) returns.
    fn hook_env(&self, arg: &str) -> HookEntry;
    /// A hook that exits with `code`, first writing `text` to both streams.
    fn hook_exits(&self, code: i32, text: &str) -> HookEntry;
    /// A hook that never ends on its own: the runner's timeout must.
    fn hook_hangs(&self) -> HookEntry;
    /// A hook whose program is not there.
    fn hook_missing(&self) -> HookEntry;
    /// A hook that exits zero, first writing `text` to stdout.
    fn hook_prints(&self, text: &str) -> HookEntry;
    /// The secret the captured output is redacted against.
    fn secret(&self) -> String;
    /// The name of a variable of the daemon's environment that is not on
    /// the worker's allow-list, and so must not reach a hook: the runner of
    /// a test process names one cargo always sets, such as
    /// `CARGO_PKG_NAME`.
    fn leaked_env_var(&self) -> String;
    /// The lines the logging and env hooks have written, in order, once
    /// every hook the suite started has ended.
    async fn written(&self) -> Vec<String>;
}

/// Runs the whole suite against `harness`.
pub async fn run(harness: &dyn HooksHarness) {
    let demo = Demo::new();
    assert!(
        demo.is_some(),
        "the suite's own demo identifiers are valid ones"
    );
    let Some(demo) = demo else { return };

    a_hook_that_succeeds_captures_its_output(harness, &demo).await;
    a_failing_hook_reports_its_exit_code(harness, &demo).await;
    a_hook_runs_as_parsed_without_a_shell(harness, &demo).await;
    a_hook_is_told_the_context_and_nothing_else(harness, &demo).await;
    a_hook_past_its_timeout_is_stopped(harness, &demo).await;
    a_hook_that_cannot_be_started_is_not_started(harness, &demo).await;
    output_past_the_cap_is_cut(harness, &demo).await;
    a_secret_in_the_output_is_redacted(harness, &demo).await;
}

/// The suite's own demo identifiers, parsed once.
struct Demo {
    plan: PlanId,
    alias: RepositoryAlias,
    snapshot: Option<SnapshotId>,
}

impl Demo {
    fn new() -> Option<Self> {
        Some(Self {
            plan: PlanId::parse("demo-plan").ok()?,
            alias: RepositoryAlias::parse("nas").ok()?,
            snapshot: SnapshotId::parse(&"1".repeat(64)).ok(),
        })
    }
}

fn context(demo: &Demo, phase: HookPhase) -> HookContext {
    HookContext {
        phase,
        plan: demo.plan.clone(),
        repository: demo.alias.clone(),
        operation: OperationId::from_uuid(Uuid::from_u128(7)),
        snapshot: demo.snapshot.clone(),
        error: None,
    }
}

async fn run_hook(
    harness: &dyn HooksHarness,
    demo: &Demo,
    entry: &HookEntry,
) -> crate::ports::HookOutcome {
    harness
        .runner()
        .run(entry, &context(demo, HookPhase::Before))
        .await
}

async fn a_hook_that_succeeds_captures_its_output(harness: &dyn HooksHarness, demo: &Demo) {
    let hook = harness.hook_prints("demo output");
    let outcome = run_hook(harness, demo, &hook).await;
    assert!(outcome.succeeded(), "a zero exit is a success: {outcome:?}");
    assert_eq!(outcome.stdout.text, "demo output");
    assert!(!outcome.stdout.truncated);
}

async fn a_failing_hook_reports_its_exit_code(harness: &dyn HooksHarness, demo: &Demo) {
    let hook = harness.hook_exits(3, "boom");
    let outcome = run_hook(harness, demo, &hook).await;
    assert_eq!(outcome.status, HookStatus::Failed { exit_code: Some(3) });
    assert_eq!(outcome.stderr.text, "boom");
}

async fn a_hook_runs_as_parsed_without_a_shell(harness: &dyn HooksHarness, demo: &Demo) {
    let arg = "two words";
    let hook = harness.hook_env(arg);
    let outcome = run_hook(harness, demo, &hook).await;
    assert!(outcome.succeeded(), "the env hook runs: {outcome:?}");
    let written = harness.written().await;
    let argv: Vec<&String> = written.iter().filter(|l| l.starts_with("argv: ")).collect();
    assert_eq!(argv, [format!("argv: {arg}")].iter().collect::<Vec<_>>());
}

async fn a_hook_is_told_the_context_and_nothing_else(harness: &dyn HooksHarness, demo: &Demo) {
    let secret = harness.secret();
    let leaked = format!("env: {}=", harness.leaked_env_var());
    let hook = harness.hook_env("demo");
    let outcome = run_hook(harness, demo, &hook).await;
    assert!(outcome.succeeded(), "the env hook runs: {outcome:?}");
    let written = harness.written().await;
    assert!(
        written.contains(&"env: LADDERBACK_HOOK=before".to_owned()),
        "the hook is told its phase: {written:?}"
    );
    assert!(
        written.contains(&"env: LADDERBACK_PLAN=demo-plan".to_owned()),
        "the hook is told its plan: {written:?}"
    );
    assert!(
        written.contains(&"env: LADDERBACK_REPOSITORY=nas".to_owned()),
        "the hook is told its repository: {written:?}"
    );
    assert!(
        written
            .iter()
            .any(|l| l.starts_with("env: LADDERBACK_OPERATION_ID=")),
        "the hook is told its operation: {written:?}"
    );
    // Every name the hook saw is the worker's allow-list (components.md
    // §4.1) or a context variable: nothing else of the daemon's gets through.
    for line in &written {
        if let Some(name) = line.strip_prefix("env: ").and_then(|l| l.split('=').next()) {
            assert!(
                name.starts_with("LADDERBACK_") || WORKER_ALLOW_LIST.contains(&name),
                "{name} is not allowed to reach a hook"
            );
        }
    }
    assert!(
        written.iter().all(|l| !l.starts_with(&leaked)),
        "no secret reaches a hook: {written:?}"
    );
    assert!(
        written.iter().all(|l| !l.contains(&secret)),
        "no secret value reaches a hook: {written:?}"
    );
}

/// The only variables of the daemon's environment a worker, and so a hook,
/// is passed (components.md §4.1). The proxy variables also count in
/// lowercase.
const WORKER_ALLOW_LIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TZ",
    "LANG",
    "LC_ALL",
    "SSH_AUTH_SOCK",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "RCLONE_CONFIG",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

async fn a_hook_past_its_timeout_is_stopped(harness: &dyn HooksHarness, demo: &Demo) {
    let hook = harness.hook_hangs();
    let outcome = run_hook(harness, demo, &hook).await;
    assert_eq!(outcome.status, HookStatus::TimedOut);
}

async fn a_hook_that_cannot_be_started_is_not_started(harness: &dyn HooksHarness, demo: &Demo) {
    let hook = harness.hook_missing();
    let outcome = run_hook(harness, demo, &hook).await;
    assert!(
        matches!(outcome.status, HookStatus::NotStarted { ref detail } if !detail.is_empty()),
        "a missing program is NotStarted with a detail: {:?}",
        outcome.status
    );
}

async fn output_past_the_cap_is_cut(harness: &dyn HooksHarness, demo: &Demo) {
    let big = "a".repeat(MAX_HOOK_OUTPUT_BYTES + 17);
    let hook = harness.hook_prints(&big);
    let outcome = run_hook(harness, demo, &hook).await;
    assert!(outcome.succeeded());
    assert!(
        outcome.stdout.truncated,
        "past the cap is marked: {outcome:?}"
    );
    assert!(outcome.stdout.text.len() <= MAX_HOOK_OUTPUT_BYTES);
}

async fn a_secret_in_the_output_is_redacted(harness: &dyn HooksHarness, demo: &Demo) {
    let secret = harness.secret();
    let hook = harness.hook_prints(&format!("password is {secret}"));
    let outcome = run_hook(harness, demo, &hook).await;
    assert!(outcome.succeeded());
    assert!(
        !outcome.stdout.text.contains(&secret),
        "a secret is masked before it is kept: {:?}",
        outcome.stdout
    );
}
