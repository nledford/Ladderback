//! Steps for the `@e2e` scenarios of `features/planning/hooks.feature`:
//! real hook processes, as demo scripts in the scratch dir, run by the real
//! `ProcessHookRunner` around the operation.

use std::assert_matches;

use app::ports::{HookRunner, HookSetup};
use cucumber::{given, then};
use domain::execution::{FailureReason, OperationStatus};
use domain::planning::{HookEntry, OnFailure};
use domain::{PlanId, RepositoryAlias};
use hooks::ProcessHookRunner;
use std::sync::Arc;
use std::time::Duration;

use super::Agent;

/// The hooks an e2e scenario has configured.
pub struct HooksE2e {
    pub runner: Arc<dyn HookRunner>,
    pub setup: HookSetup,
}

impl HooksE2e {
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessHookRunner::new()),
            setup: HookSetup {
                agent: Default::default(),
                repository: Default::default(),
                plan: Default::default(),
                plan_id: PlanId::parse("demo-plan").unwrap(),
                repository_alias: RepositoryAlias::parse("nas").unwrap(),
            },
        }
    }
}

impl Default for HooksE2e {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HooksE2e {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HooksE2e")
            .field("setup", &self.setup)
            .finish_non_exhaustive()
    }
}

/// Writes an executable demo script `name.sh` into the scratch dir and
/// returns an entry running it.
fn script(agent: &mut Agent, name: &str, body: &str) -> HookEntry {
    let path = agent.scratch().join(format!("{name}.sh"));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the script is written");
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path)
            .expect("the script is there")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("the script is executable");
    }
    HookEntry::new(&path.display().to_string(), ["demo"], OnFailure::Error)
        .expect("the demo hook is valid")
}

#[given("a before hook at the plan level that never ends, with on-failure error")]
fn never_ends(agent: &mut Agent) {
    agent.hooks.runner = Arc::new(ProcessHookRunner::with_timeout(Duration::from_millis(500)));
    let hook = script(agent, "hangs", "sleep 30");
    agent.hooks.setup.plan.run_before.push(hook);
}

#[given("a before hook at the plan level that writes its environment to the scratch dir")]
fn writes_env(agent: &mut Agent) {
    let env_file = agent.scratch().join("hook-env").display().to_string();
    let hook = script(agent, "env-dump", &format!("env > {env_file}"));
    agent.hooks.setup.plan.run_before.push(hook);
}

#[then("the operation ends as failed because a hook timed out")]
fn failed_by_timeout(agent: &mut Agent) {
    assert_matches!(
        agent.operation().status(),
        OperationStatus::Failed {
            reason: FailureReason::HookFailed { exit_code: None }
        }
    );
}

/// The variables a hook may be given on top of its `LADDERBACK_*` context:
/// the worker's allow-list (components.md §4.1, also pinned by the
/// `HookRunner` contract).
const ALLOWED: &[&str] = &[
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

#[then(
    "the hook's environment names LADDERBACK_HOOK and LADDERBACK_PLAN and no variable the worker's allow-list forbids"
)]
fn env_named(agent: &mut Agent) {
    let read = std::fs::read_to_string(agent.scratch().join("hook-env"))
        .expect("the hook wrote its environment");
    assert!(
        read.contains("LADDERBACK_HOOK=before"),
        "the hook is told its phase: {read}"
    );
    assert!(
        read.contains("LADDERBACK_PLAN=demo-plan"),
        "the hook is told its plan: {read}"
    );
    for name in read.lines().filter_map(|l| l.split('=').next()) {
        assert!(
            name.starts_with("LADDERBACK_")
                || ALLOWED.contains(&name)
                // Set by the POSIX shell the demo script runs under.
                || matches!(name, "PWD" | "SHLVL" | "_"),
            "{name} is not allowed to reach a hook"
        );
    }
}
