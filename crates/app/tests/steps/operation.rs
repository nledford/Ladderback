//! Steps for `features/execution/*.feature`, shared by the fake-backed
//! acceptance runner and the `@e2e` runner. Each runner's `Agent` world has
//! a `job`, the `finished` run and the `operation` it left, the operation
//! `store` the run wrote, and the methods used below.

use std::time::Duration;

use app::history::GetOperation;
use app::ports::{FakeEnd, OperationDetail, OperationRecord};
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

/// An unreadable file's warning with `length` characters of text in `field`
/// (`item`, `during` or `detail`), and the usual short text in the others.
fn unreadable_file_with_long(field: &str, length: usize) -> Warning {
    let long = "x".repeat(length);
    let mut warning = Warning {
        kind: WarningKind::SourceUnreadable,
        item: Some("/home/demo/locked.txt".to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    };
    match field {
        "item" => warning.item = Some(long),
        "during" => warning.during = long,
        "detail" => warning.detail = long,
        other => panic!("a warning has no {other}"),
    }
    warning
}

#[given(expr = "the engine will report an unreadable file whose {word} is {int} characters long")]
fn unreadable_file_with_long_text(agent: &mut Agent, field: String, length: usize) {
    agent.job.warnings = vec![unreadable_file_with_long(&field, length)];
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

/// What the history holds for the operation that just ran.
async fn history_of_the_operation(agent: &mut Agent) -> OperationDetail {
    let store = agent.store().await;
    let id = agent.operation().id();
    GetOperation::new(store)
        .get(id)
        .await
        .expect("the history answers")
        .expect("the operation is in the history")
}

#[then("the history shows the operation as it ended")]
async fn history_shows_the_operation(agent: &mut Agent) {
    // A write the log refused is the reason its record can differ from the
    // operation, so it is named first.
    assert!(
        agent.finished().recording_errors.is_empty(),
        "the log refused a write: {:?}",
        agent.finished().recording_errors
    );
    let detail = history_of_the_operation(agent).await;
    let finished = agent.finished();
    assert_eq!(
        detail.record,
        OperationRecord::of(&finished.operation, finished.result.as_ref())
    );
    assert_eq!(detail.warnings, finished.operation.warnings());
}

#[then(expr = "the history lists {int} warning(s) for the operation")]
async fn history_lists_warnings(agent: &mut Agent, count: usize) {
    assert_eq!(history_of_the_operation(agent).await.warnings.len(), count);
}

#[then(expr = "the history counts {int} warning(s) for the operation")]
async fn history_counts_warnings(agent: &mut Agent, count: u64) {
    let detail = history_of_the_operation(agent).await;
    assert_eq!(detail.record.warning_count, count);
}

/// `field` (`item`, `during` or `detail`) of `warning`.
fn text_of<'a>(warning: &'a Warning, field: &str) -> Option<&'a str> {
    match field {
        "item" => warning.item.as_deref(),
        "during" => Some(&warning.during),
        "detail" => Some(&warning.detail),
        other => panic!("a warning has no {other}"),
    }
}

/// The warning the engine reported, and the one the history holds for it.
async fn reported_and_kept_warning(agent: &mut Agent) -> (Warning, Warning) {
    let kept = history_of_the_operation(agent).await.warnings;
    let [kept] = <[Warning; 1]>::try_from(kept).expect("the history holds one warning");
    let reported = agent
        .job
        .warnings
        .first()
        .cloned()
        .expect("the engine reported a warning");
    (reported, kept)
}

#[then(expr = "the history keeps the warning's {word} cut to {int} bytes, ending in {string}")]
async fn history_keeps_text_cut(agent: &mut Agent, field: String, bytes: usize, mark: String) {
    let (reported, kept) = reported_and_kept_warning(agent).await;
    let kept = text_of(&kept, &field).expect("the warning has that text");
    let reported = text_of(&reported, &field).expect("the warning has that text");
    assert_eq!(kept.len(), bytes);
    let start = kept
        .strip_suffix(&mark)
        .unwrap_or_else(|| panic!("the {field} doesn't end in {mark:?}"));
    assert!(
        reported.starts_with(start),
        "what is kept of the {field} is the start of what the engine reported"
    );
}

#[then(expr = "the history keeps any of the warning's text of {int} bytes or less whole")]
async fn history_keeps_short_text_whole(agent: &mut Agent, cap: usize) {
    let (reported, kept) = reported_and_kept_warning(agent).await;
    assert_eq!(kept.kind, reported.kind);
    for field in ["item", "during", "detail"] {
        let reported = text_of(&reported, field);
        if reported.is_none_or(|text| text.len() <= cap) {
            assert_eq!(text_of(&kept, field), reported, "{field}");
        }
    }
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

/// Whether the `--tags` expression given on the command line, if any, picks
/// a scenario, counting its feature's and rule's tags as cucumber-rs does.
///
/// The runners apply it, and `--name`, on top of their own filter. Left to
/// cucumber-rs, either option replaces that filter, and a runner is handed
/// scenarios whose steps only the other runner defines.
pub fn picked_by_cli_tags(
    tags: Option<&cucumber::gherkin::tagexpr::TagOperation>,
    feature: &cucumber::gherkin::Feature,
    rule: Option<&cucumber::gherkin::Rule>,
    scenario: &cucumber::gherkin::Scenario,
) -> bool {
    use cucumber::tag::Ext as _;
    tags.is_none_or(|tags| {
        tags.eval(
            feature
                .tags
                .iter()
                .chain(rule.iter().flat_map(|r| &r.tags))
                .chain(&scenario.tags),
        )
    })
}
