//! Steps for `features/execution/*.feature`, shared by the fake-backed
//! acceptance runner and the `@e2e` runner. Each runner's `Agent` world has
//! a `job`, the finished `operation`, and the two methods used below.

use std::time::Duration;

use app::ports::FakeEnd;
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::execution::{FailureReason, OperationStatus, Warning, WarningKind};

use super::Agent;

#[given(expr = "the engine will complete the operation in {int} steps of {int} minute(s) each")]
fn steps_of_minutes(agent: &mut Agent, steps: u32, minutes: u64) {
    agent.job.steps = steps;
    agent.job.step_delay = Duration::from_secs(minutes * 60);
}

#[given(expr = "the engine will report {int} unreadable files")]
fn unreadable_files(agent: &mut Agent, count: usize) {
    agent.job.warnings = (1..=count)
        .map(|n| Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some(format!("/home/demo/locked-{n}.txt")),
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        })
        .collect();
}

#[given(expr = "the engine will report the failure {string}")]
fn reported_failure(agent: &mut Agent, detail: String) {
    agent.job.end = FakeEnd::Fail { detail };
}

#[given(expr = "the engine will stop after {int} step(s) with exit code {int}")]
fn stops(agent: &mut Agent, after_steps: u32, exit_code: u8) {
    agent.job.steps = after_steps + 1;
    agent.job.end = FakeEnd::Exit {
        after_steps,
        exit_code,
    };
}

#[given("the engine can't be started")]
fn cannot_start(agent: &mut Agent) {
    agent.make_engine_unavailable();
}

#[when("the agent runs the operation")]
async fn run(agent: &mut Agent) {
    agent.run_operation().await;
}

#[then("the operation ends as succeeded")]
fn succeeded(agent: &mut Agent) {
    assert_eq!(agent.operation().status(), &OperationStatus::Succeeded);
}

#[then("the operation ends as succeeded with warnings")]
fn succeeded_with_warnings(agent: &mut Agent) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::SucceededWithWarnings
    );
}

#[then(expr = "the operation took {int} minutes")]
fn took(agent: &mut Agent, minutes: i64) {
    assert_eq!(
        agent.operation().duration(),
        Some(SignedDuration::from_mins(minutes))
    );
}

#[then(expr = "the operation lists {int} warning(s)")]
fn lists_warnings(agent: &mut Agent, count: usize) {
    assert_eq!(agent.operation().warnings().len(), count);
}

#[then(expr = "the operation ends as failed because the engine reported {string}")]
fn failed_reported(agent: &mut Agent, detail: String) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::EngineFailed { detail }
        }
    );
}

#[then("the operation ends as failed because the engine could not start")]
fn failed_to_start(agent: &mut Agent) {
    assert!(
        matches!(
            agent.operation().status(),
            OperationStatus::Failed {
                reason: FailureReason::EngineUnavailable { .. }
            }
        ),
        "{:?}",
        agent.operation().status()
    );
}

#[then(expr = "the operation ends as failed because the engine exited with code {int}")]
fn failed_exited(agent: &mut Agent, code: i32) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::EngineExited {
                exit_code: Some(code)
            }
        }
    );
}

#[then("the operation never started")]
fn never_started(agent: &mut Agent) {
    assert_eq!(agent.operation().started_at(), None);
}

#[given(expr = "the operation has {int} minute(s) before its deadline")]
fn minutes_before_deadline(agent: &mut Agent, minutes: i64) {
    agent.job.deadline_budget = Some(SignedDuration::from_mins(minutes));
}

#[then("the operation ends as cancelled")]
fn cancelled(agent: &mut Agent) {
    assert_eq!(agent.operation().status(), &OperationStatus::Cancelled);
}

#[then("the operation ends as failed because its deadline passed")]
fn failed_deadline(agent: &mut Agent) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    );
}

/// Whether a scenario carries `tag` on itself, its rule or its feature.
pub fn tagged(
    feature: &cucumber::gherkin::Feature,
    rule: Option<&cucumber::gherkin::Rule>,
    scenario: &cucumber::gherkin::Scenario,
    tag: &str,
) -> bool {
    let has = |tags: &[String]| tags.iter().any(|t| t == tag);
    has(&feature.tags) || rule.is_some_and(|r| has(&r.tags)) || has(&scenario.tags)
}
