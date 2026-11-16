//! Steps for `features/planning/proposals_over_http.feature`: an API token
//! the admin created proposes configuration changes through the real
//! daemon's proposal routes, and the steps read back the config file the
//! proposal must leave as it was (decision 89).

use cucumber::{given, then, when};
use domain::{SignedDuration, Timestamp};

use super::request_checks_steps::{self as web, Request, Response};
use super::{Agent, blocking, sign_in_steps};

/// The config file as the scenario wrote it, and the proposal it made.
#[derive(Debug, Default)]
pub struct ProposalsE2e {
    /// The config file's text, as written before any proposal.
    config: Option<String>,
    /// The ID of the proposal the API token made.
    proposal: Option<String>,
    /// The value a refused proposal's change held, which no refusal holds.
    value: Option<String>,
}

fn config_file(agent: &mut Agent) -> std::path::PathBuf {
    testkit::daemon::config_dir(&sign_in_steps::home(agent)).join("config.toml")
}

/// Sends `body` to `path` with the API token, as a program presents it, and
/// keeps the answer as every request step does.
async fn with_the_token(agent: &mut Agent, method: &str, path: String, body: String) {
    let token = agent.api_tokens.text().to_owned();
    let request = Request {
        method: method.to_owned(),
        path,
        headers: vec![
            format!("Authorization: Bearer {token}"),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body,
    };
    let addr = agent.web.server().addr;
    let response = blocking(move || web::exchange(addr, &request)).await;
    agent.web.response = Some(response);
}

fn answer(response: &Response) -> serde_json::Value {
    serde_json::from_str(&response.body)
        .unwrap_or_else(|error| panic!("the answer is JSON ({error}): {:?}", response.body))
}

/// The API token proposes adding the plan `id` over `source` into
/// `repository`, with its rationale.
async fn propose(agent: &mut Agent, id: &str, source: &str, repository: &str) {
    let body = serde_json::json!({
        "changes": [{
            "kind": "add-plan",
            "plan": { "id": id, "repository": repository, "sources": [source] },
        }],
        "rationale": "Back up the demo home folder every night.",
    });
    with_the_token(
        agent,
        "POST",
        "/api/v1/proposals".to_owned(),
        body.to_string(),
    )
    .await;
}

#[given(expr = "the daemon's config file holds only the repository {string}")]
fn holds_only_the_repository(agent: &mut Agent, alias: String) {
    let text = format!(
        "schema = 1\n\n[repositories.{alias}]\nrepository = \"/srv/demo-repo\"\npassword-env = \"DEMO_PASSWORD\"\n"
    );
    let file = config_file(agent);
    std::fs::create_dir_all(file.parent().expect("the config folder"))
        .expect("the daemon's config folder");
    std::fs::write(&file, &text).expect("the daemon's config file");
    agent.proposals.config = Some(text);
}

#[when(expr = "the API token proposes adding the plan {string} over {string}")]
async fn proposes(agent: &mut Agent, id: String, source: String) {
    propose(agent, &id, &source, "demo").await;
}

#[given(expr = "the API token has proposed adding the plan {string} over {string}")]
async fn has_proposed(agent: &mut Agent, id: String, source: String) {
    propose(agent, &id, &source, "demo").await;
    let response = agent.web.response();
    assert_eq!(response.status, 201, "body {:?}", response.body);
    let made = answer(response);
    let id = made["id"].as_str().expect("the proposal's ID").to_owned();
    agent.proposals.proposal = Some(id);
}

#[when(expr = "the API token proposes adding the plan {string} into the repository {string}")]
async fn proposes_into(agent: &mut Agent, id: String, repository: String) {
    propose(agent, &id, "/home/demo", &repository).await;
    agent.proposals.value = Some(repository);
}

#[when("the API token withdraws its proposal")]
async fn withdraws(agent: &mut Agent) {
    let id = agent
        .proposals
        .proposal
        .clone()
        .expect("a proposal was made");
    let path = format!("/api/v1/proposals/{id}/withdraw");
    with_the_token(agent, "POST", path, String::new()).await;
}

#[then(
    expr = "the proposal is answered as pending, proposed by {string}, to expire {int} hours after it was made"
)]
fn answered_pending(agent: &mut Agent, label: String, hours: i64) {
    let response = agent.web.response();
    assert_eq!(response.status, 201, "body {:?}", response.body);
    let made = answer(response);
    assert_eq!(made["status"], "pending", "{made}");
    assert_eq!(made["proposed-by"], label.as_str(), "{made}");
    let at = |key: &str| -> Timestamp {
        made[key]
            .as_str()
            .and_then(|text| text.parse().ok())
            .unwrap_or_else(|| panic!("{key} is a time: {made}"))
    };
    assert_eq!(
        at("expires-at").duration_since(at("created-at")),
        SignedDuration::from_hours(hours),
        "{made}"
    );
}

#[then(expr = "the daemon's config file still holds only the repository {string}")]
fn still_holds_only(agent: &mut Agent, alias: String) {
    let text = std::fs::read_to_string(config_file(agent)).expect("the config file");
    let written = agent
        .proposals
        .config
        .as_deref()
        .expect("the file was written");
    assert_eq!(text, written, "the config file changed");
    let file: toml::Table = text.parse().expect("the config file is TOML");
    let repositories: Vec<&String> = file
        .get("repositories")
        .and_then(toml::Value::as_table)
        .map(|table| table.keys().collect())
        .unwrap_or_default();
    assert_eq!(repositories, [&alias]);
    assert!(file.get("plans").is_none(), "the file has plans: {text}");
}

#[then("the API token's proposals list it as withdrawn")]
async fn lists_withdrawn(agent: &mut Agent) {
    let withdrawn = agent.web.response();
    assert_eq!(withdrawn.status, 200, "body {:?}", withdrawn.body);
    assert_eq!(answer(withdrawn)["status"], "withdrawn");
    let id = agent
        .proposals
        .proposal
        .clone()
        .expect("a proposal was made");
    with_the_token(agent, "GET", "/api/v1/proposals".to_owned(), String::new()).await;
    let response = agent.web.response();
    assert_eq!(response.status, 200, "body {:?}", response.body);
    let listed = answer(response);
    let statuses: Vec<(&str, &str)> = listed["proposals"]
        .as_array()
        .expect("a list of proposals")
        .iter()
        .map(|one| {
            (
                one["id"].as_str().unwrap_or_default(),
                one["status"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(statuses, [(id.as_str(), "withdrawn")], "{listed}");
}

#[then(expr = "the refusal names change {int} and its setting {string}")]
fn names_the_change(agent: &mut Agent, change: u64, field: String) {
    let response = agent.web.response();
    let refusal = answer(response);
    assert_eq!(
        (&refusal["args"]["change"], &refusal["args"]["field"]),
        (
            &serde_json::json!({ "type": "count", "value": change }),
            &serde_json::json!({ "type": "text", "value": field }),
        ),
        "{refusal}"
    );
    let value = agent
        .proposals
        .value
        .as_deref()
        .expect("the change's value");
    assert!(
        !response.body.contains(value),
        "the refusal holds the value {value:?}: {}",
        response.body
    );
}
