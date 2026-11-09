//! Steps for `features/planning/hooks.feature`, for the fake-backed
//! acceptance runner: hooks are scripted on a `FakeHookRunner`, and the
//! operation runs with them attached.

use std::sync::Arc;

use cucumber::{given, then};

use app::fakes::{FakeHookRunner, Scripted};
use app::ports::HookSetup;
use domain::execution::{FailureReason, OperationStatus};
use domain::planning::{HookEntry, Hooks, OnFailure};
use domain::{PlanId, RepositoryAlias};

use super::Agent;

/// The hooks a scenario has configured, level by level, on a shared fake.
pub struct HooksWorld {
    pub runner: Arc<FakeHookRunner>,
    pub setup: HookSetup,
}

impl HooksWorld {
    pub fn new() -> Self {
        Self {
            runner: Arc::new(FakeHookRunner::new()),
            setup: HookSetup {
                agent: Hooks::default(),
                repository: Hooks::default(),
                plan: Hooks::default(),
                plan_id: PlanId::parse("demo-plan").unwrap(),
                repository_alias: RepositoryAlias::parse("nas").unwrap(),
            },
        }
    }
}

impl std::fmt::Debug for HooksWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HooksWorld")
            .field("setup", &self.setup)
            .finish_non_exhaustive()
    }
}

/// The list a phase word means.
const PHASES: &[(&str, &str)] = &[
    ("before", "before"),
    ("after", "after"),
    ("failed", "failed"),
    ("finally", "finally"),
];

fn phase_word(word: &str) -> &'static str {
    PHASES
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, phase)| *phase)
        .unwrap_or_else(|| panic!("unknown hook phase {word}"))
}

fn on_failure(word: &str) -> OnFailure {
    match word {
        "error" => OnFailure::Error,
        "warn" => OnFailure::Warn,
        "ignore" => OnFailure::Ignore,
        other => panic!("unknown on-failure {other}"),
    }
}

/// The level's list for `phase`, in the scenario's setup.
fn list<'a>(agent: &'a mut Agent, level: &str, phase: &str) -> &'a mut Vec<HookEntry> {
    let setup = &mut agent.hooks.setup;
    let hooks = match level {
        "agent" => &mut setup.agent,
        "repository" => &mut setup.repository,
        "plan" => &mut setup.plan,
        other => panic!("unknown hook level {other}"),
    };
    match phase {
        "before" => &mut hooks.run_before,
        "after" => &mut hooks.run_after,
        "failed" => &mut hooks.run_failed,
        "finally" => &mut hooks.run_finally,
        other => panic!("unknown hook phase {other}"),
    }
}

/// Scripts the level's `phase` hook on the fake and adds it to the setup.
/// The program is named `<level>-<phase>`, so recorded calls can be told
/// apart.
fn add(agent: &mut Agent, phase: &str, level: &str, outcome: Scripted, on_failure: OnFailure) {
    let name = format!("{level}-{phase}");
    agent.hooks.runner.script(&name, outcome);
    list(agent, level, phase)
        .push(HookEntry::new(&name, ["demo"], on_failure).expect("the demo hook is valid"));
}

#[given(expr = "an {word} hook at the {word} level that succeeds")]
#[given(expr = "a {word} hook at the {word} level that succeeds")]
fn succeeding(agent: &mut Agent, phase: String, level: String) {
    add(
        agent,
        phase_word(&phase),
        &level,
        Scripted::succeeds(),
        OnFailure::Error,
    );
}

#[given(expr = "an {word} hook at the {word} level that exits {int} with on-failure {word}")]
#[given(expr = "a {word} hook at the {word} level that exits {int} with on-failure {word}")]
fn failing(agent: &mut Agent, phase: String, level: String, code: i32, on_failure_word: String) {
    add(
        agent,
        phase_word(&phase),
        &level,
        Scripted::fails_with(code),
        on_failure(&on_failure_word),
    );
}

#[given(expr = "an {word} hook at the {word} level that times out")]
#[given(expr = "a {word} hook at the {word} level that times out")]
fn timing_out(agent: &mut Agent, phase: String, level: String) {
    add(
        agent,
        phase_word(&phase),
        &level,
        Scripted::times_out(),
        OnFailure::Error,
    );
}

#[then(expr = "the hooks ran in the order {string}")]
fn ran_in_order(agent: &mut Agent, order: String) {
    let ran: Vec<String> = agent
        .hooks
        .runner
        .calls()
        .iter()
        .map(|call| call.entry.command.program().to_owned())
        .collect();
    let want: Vec<String> = order.split(',').map(str::to_owned).collect();
    assert_eq!(ran, want, "the hooks run in §6.1's order");
}

#[then(expr = "the {word} hook at the {word} level ran")]
fn ran(agent: &mut Agent, phase: String, level: String) {
    let name = format!("{level}-{}", phase_word(&phase));
    assert!(
        agent
            .hooks
            .runner
            .calls()
            .iter()
            .any(|call| call.entry.command.program() == name),
        "the {name} hook ran"
    );
}

#[then(expr = "the {word} hook at the {word} level never ran")]
fn never_ran(agent: &mut Agent, phase: String, level: String) {
    let name = format!("{level}-{}", phase_word(&phase));
    assert!(
        agent
            .hooks
            .runner
            .calls()
            .iter()
            .all(|call| call.entry.command.program() != name),
        "the {name} hook never ran"
    );
}

#[then(expr = "the operation ends as failed because a hook failed with exit code {int}")]
fn failed_by_hook(agent: &mut Agent, code: i32) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::HookFailed {
                exit_code: Some(code)
            }
        }
    );
}

#[then("the operation ends as failed because a hook timed out")]
fn failed_by_timeout(agent: &mut Agent) {
    assert!(matches!(
        agent.operation().status(),
        OperationStatus::Failed {
            reason: FailureReason::HookFailed { exit_code: None }
        }
    ));
}
