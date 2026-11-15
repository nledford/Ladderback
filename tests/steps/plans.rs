//! Steps for `features/planning/plans_over_http.feature`: the client creates,
//! changes and deletes plans through the real daemon's plan routes, and the
//! steps read back the config file it writes and the history of what ran.

use std::time::{Duration, Instant};

use cucumber::{then, when};

use super::events_steps::Events;
use super::operations_steps::{exchange_at, history_page};
use super::repositories_steps::api_answer;
use super::{Agent, blocking};

/// How long a plan scheduled every minute may take to back up: the wait for
/// the next minute, then the backup of a few demo files.
const EVERY_MINUTE: Duration = Duration::from_secs(90);

fn config_file(agent: &Agent) -> std::path::PathBuf {
    Events::config_dir(agent).join("config.toml")
}

/// Sends one request to the daemon, remembering its answer as every client
/// step does. A write takes the configuration's lock and writes a file, so
/// the wait is on a blocking thread: see [`blocking`].
async fn send(agent: &mut Agent, method: &'static str, path: String, body: String) {
    let addr = agent.events.addr();
    let answer = blocking(move || exchange_at(addr, method, &path, &body)).await;
    agent.repositories.api_last = Some(answer);
}

/// The plan `id` in the config file as written, or `None`.
fn plan_in_file(agent: &Agent, id: &str) -> Option<toml::Table> {
    let text = std::fs::read_to_string(config_file(agent)).expect("the config file");
    let file: toml::Table = text.parse().expect("the config file is TOML");
    file.get("plans")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .find(|plan| plan.get("id").and_then(toml::Value::as_str) == Some(id))
        .cloned()
}

#[when(
    expr = "the client creates the plan {string} over the source of {string}, scheduled {string}"
)]
async fn creates(agent: &mut Agent, id: String, _over: String, cron: String) {
    let source = agent.backup.source.clone().expect("the plan's source");
    let body = serde_json::json!({
        "id": id,
        "repository": "demo",
        "sources": [source.to_str().expect("a UTF-8 path")],
        "schedule": {"cron": cron},
    });
    send(agent, "POST", "/api/v1/plans".to_owned(), body.to_string()).await;
}

/// The Plans page changes a plan as it shows it: it reads the plan, changes
/// the one setting and sends the whole plan back.
#[when(expr = "the client changes the schedule of the plan {string} to {string}")]
async fn changes_schedule(agent: &mut Agent, id: String, cron: String) {
    let path = format!("/api/v1/plans/{id}");
    send(agent, "GET", path.clone(), String::new()).await;
    let (status, mut plan) = api_answer(agent);
    assert_eq!(status, 200, "the plan reads: {plan}");
    plan["schedule"]["cron"] = serde_json::json!(cron);
    send(agent, "PUT", path, plan.to_string()).await;
    let (status, answer) = api_answer(agent);
    assert_eq!(status, 200, "the change is taken: {answer}");
}

#[when(expr = "the client deletes the plan {string}")]
async fn deletes(agent: &mut Agent, id: String) {
    send(
        agent,
        "DELETE",
        format!("/api/v1/plans/{id}"),
        String::new(),
    )
    .await;
}

/// Nothing in the scenario starts a backup by hand, so a backup of the plan
/// is one its schedule started.
#[then(expr = "the history comes to list a scheduled backup of the plan {string} that succeeded")]
async fn history_lists_scheduled_backup(agent: &mut Agent, plan: String) {
    let started = Instant::now();
    loop {
        let page = history_page(agent, "limit=50").await;
        let succeeded = page
            .get("operations")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|op| {
                op.get("kind").and_then(serde_json::Value::as_str) == Some("backup")
                    && op.get("plan").and_then(serde_json::Value::as_str) == Some(plan.as_str())
            })
            .any(|op| op.get("status").and_then(serde_json::Value::as_str) == Some("succeeded"));
        if succeeded {
            break;
        }
        assert!(
            started.elapsed() < EVERY_MINUTE,
            "no backup of {plan} succeeded within {} s: {page}",
            EVERY_MINUTE.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[then(expr = "the config file holds the plan {string} scheduled {string}")]
fn file_holds_plan(agent: &mut Agent, id: String, cron: String) {
    let plan =
        plan_in_file(agent, &id).unwrap_or_else(|| panic!("the config file holds no plan {id:?}"));
    let written = plan
        .get("schedule")
        .and_then(|schedule| schedule.get("cron"))
        .and_then(toml::Value::as_str);
    assert_eq!(written, Some(cron.as_str()), "the plan as written: {plan}");
}

#[then(expr = "the config file holds no plan {string}")]
fn file_holds_no_plan(agent: &mut Agent, id: String) {
    assert_eq!(
        plan_in_file(agent, &id),
        None,
        "the config file holds {id:?}"
    );
}

#[then("the daemon's config file reads:")]
fn file_reads(agent: &mut Agent, step: &cucumber::gherkin::Step) {
    let expected = step.docstring.as_deref().expect("the step has a text");
    // As `the daemon's config file is edited by hand to hold:` writes it.
    let expected = format!("{}\n", expected.trim_start_matches('\n'));
    let text = std::fs::read_to_string(config_file(agent)).expect("the config file");
    assert_eq!(text, expected, "the config file as written");
}

#[then(expr = "the refusal names the rule {string}")]
fn refusal_names_rule(agent: &mut Agent, rule: String) {
    let (_, json) = api_answer(agent);
    assert_eq!(
        json["args"]["rule"],
        serde_json::json!({"type": "text", "value": rule}),
        "the refusal: {json}"
    );
}
