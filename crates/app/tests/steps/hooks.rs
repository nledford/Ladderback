//! Steps for `features/planning/hooks.feature`, for the fake-backed
//! acceptance runner: hooks are scripted on a `FakeHookRunner`, and the
//! operation runs with them attached.

use std::assert_matches;
use std::num::NonZeroU32;
use std::sync::Arc;

use cucumber::{given, then};

use app::fakes::{FakeHookRunner, Scripted};
use app::ports::{HookSetup, HookStatus};
use domain::execution::{
    FailureReason, HookEnd, HookLevel, HookPlace, OperationStatus, WarningKind,
};
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
                redactor: Default::default(),
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

#[given(expr = "a second before hook at the {word} level that exits {int} with on-failure {word}")]
fn second_failing(agent: &mut Agent, level: String, code: i32, on_failure_word: String) {
    let name = format!("{level}-before-2");
    agent.hooks.runner.script(&name, Scripted::fails_with(code));
    list(agent, &level, "before").push(
        HookEntry::new(&name, ["demo"], on_failure(&on_failure_word))
            .expect("the demo hook is valid"),
    );
}

#[given(
    regex = r"^a before hook at the (\w+) level that (a signal ends|runs out of time|can't start), with on-failure error$"
)]
fn ending_without_status(agent: &mut Agent, level: String, ends: String) {
    let status = match ends.as_str() {
        "a signal ends" => HookStatus::Failed { exit_code: None },
        "runs out of time" => HookStatus::TimedOut,
        _ => HookStatus::NotStarted {
            detail: "no such file".to_owned(),
        },
    };
    add(
        agent,
        "before",
        &level,
        Scripted {
            status,
            stdout: String::new(),
            stderr: String::new(),
        },
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
    assert_matches!(
        agent.operation().status(),
        OperationStatus::Failed {
            reason: FailureReason::HookFailed {
                ended: Some(HookEnd::Exited { code: exited }),
                ..
            }
        } if *exited == code
    );
}

#[then("the operation ends as failed because a hook timed out")]
fn failed_by_timeout(agent: &mut Agent) {
    assert_matches!(
        agent.operation().status(),
        OperationStatus::Failed {
            reason: FailureReason::HookFailed {
                ended: Some(HookEnd::TimedOut),
                ..
            }
        }
    );
}

/// The level a scenario names, as the failure records it.
fn level_word(word: &str) -> HookLevel {
    match word {
        "agent" => HookLevel::Agent,
        "repository" => HookLevel::Repository,
        "plan" => HookLevel::Plan,
        other => panic!("unknown hook level {other}"),
    }
}

#[then(
    regex = r"^the operation ends as failed because before-hook (\d+) at the (\w+) level (exited with status (\d+)|was ended by a signal|timed out|could not be started)$"
)]
fn failed_by_named_hook(
    agent: &mut Agent,
    position: u32,
    level: String,
    ended: String,
    code: String,
) {
    let ended = match ended.as_str() {
        "was ended by a signal" => HookEnd::Signalled,
        "timed out" => HookEnd::TimedOut,
        "could not be started" => HookEnd::NotStarted,
        _ => HookEnd::Exited {
            code: code.parse().expect("an exit status"),
        },
    };
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::HookFailed {
                hook: Some(HookPlace {
                    level: level_word(&level),
                    position: NonZeroU32::new(position).expect("a place counted from 1"),
                }),
                ended: Some(ended),
            }
        }
    );
}

#[then(expr = "the operation carries a hook warning from the {word} phase with detail {string}")]
fn hook_warning(agent: &mut Agent, phase: String, detail: String) {
    let during = phase_word(&phase);
    assert!(
        agent
            .operation()
            .warnings()
            .iter()
            .any(|warning| warning.kind == WarningKind::Hook
                && warning.during == during
                && warning.detail == detail),
        "a hook warning from the {phase} phase with detail {detail:?}: {:?}",
        agent.operation().warnings()
    );
}

#[then("the operation carries no hook warning")]
fn no_hook_warning(agent: &mut Agent) {
    assert!(
        agent
            .operation()
            .warnings()
            .iter()
            .all(|warning| warning.kind != WarningKind::Hook),
        "no hook warning: {:?}",
        agent.operation().warnings()
    );
}
