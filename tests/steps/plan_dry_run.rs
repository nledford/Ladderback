//! Steps for `features/automation/plan_dry_run.feature`: an MCP server
//! connected to the real daemon dry-runs a draft plan with the tool
//! `dry_run_plan`, over a demo folder of the scenario's own whose files'
//! names are canaries, within the dry-run allowlist the admin set over HTTP
//! (decision 89, point 13, §4.6).

use std::path::PathBuf;

use cucumber::{given, then, when};
use serde_json::{Value, json};

use super::request_checks_steps::Response;
use super::{Agent, api_tokens_steps, mcp_steps};

const ALLOWLIST_PATH: &str = "/api/v1/settings/dry-run-allowlist";

/// The scenario's demo folder, which the allowlist holds.
fn demo_folder(agent: &Agent) -> PathBuf {
    agent.scratch().join("dry-run-demo")
}

#[given(
    expr = "a demo folder holding the files {string} of {int} bytes and {string} of {int} bytes"
)]
fn demo_folder_holding(
    agent: &mut Agent,
    one: String,
    one_size: usize,
    other: String,
    other_size: usize,
) {
    let folder = demo_folder(agent);
    std::fs::create_dir_all(&folder).expect("the demo folder");
    std::fs::write(folder.join(one), vec![b'a'; one_size]).expect("a demo file");
    std::fs::write(folder.join(other), vec![b'b'; other_size]).expect("a demo file");
}

/// The allowlist the admin's session reads.
async fn allowlist(agent: &Agent) -> Value {
    let request = api_tokens_steps::in_session(agent, "GET", ALLOWLIST_PATH, String::new());
    let response = api_tokens_steps::exchange(agent, request).await;
    assert_eq!(response.status, 200, "body {:?}", response.body);
    serde_json::from_str(&response.body).expect("the allowlist is JSON")
}

#[given("the admin has set the dry-run allowlist to the demo folder over HTTP")]
async fn admin_has_set(agent: &mut Agent) {
    let folder = demo_folder(agent).to_string_lossy().into_owned();
    let body = json!({ "folders": [folder] }).to_string();
    let request = api_tokens_steps::in_session(agent, "PUT", ALLOWLIST_PATH, body);
    let response = api_tokens_steps::exchange(agent, request).await;
    assert_eq!(response.status, 200, "body {:?}", response.body);
}

/// Calls `dry_run_plan` with the plan `id` over `source` into the
/// repository `demo`, its table given as TOML text with `globs`.
async fn dry_run(agent: &mut Agent, id: &str, source: &str, globs: &[&str]) {
    dry_run_with(agent, id, source, globs, "").await;
}

/// As [`dry_run`], with the TOML lines `more` added to the table.
async fn dry_run_with(agent: &mut Agent, id: &str, source: &str, globs: &[&str], more: &str) {
    // A JSON string is a TOML basic string, its escapes and all.
    let source = serde_json::to_string(source).unwrap();
    let globs = serde_json::to_string(globs).unwrap();
    let plan = format!(
        "id = \"{id}\"\nrepository = \"demo\"\nsources = [{source}]\nglobs = {globs}\n{more}"
    );
    mcp_steps::call(agent, "dry_run_plan".to_owned(), json!({ "plan": plan })).await;
}

#[when(expr = "the MCP client dry-runs the plan {string} over the demo folder, excluding {string}")]
async fn dry_runs_excluding(agent: &mut Agent, id: String, glob: String) {
    let folder = demo_folder(agent).to_string_lossy().into_owned();
    dry_run(agent, &id, &folder, &[&glob]).await;
}

#[when(expr = "the MCP client dry-runs the plan {string} over a folder outside the allowlist")]
async fn dry_runs_outside(agent: &mut Agent, id: String) {
    // The scratch folder holds the demo folder, so it exists and is outside.
    let outside = agent.scratch().to_string_lossy().into_owned();
    dry_run(agent, &id, &outside, &[]).await;
}

#[when(expr = "the MCP client dry-runs the plan {string} over a folder that doesn't exist")]
async fn dry_runs_missing(agent: &mut Agent, id: String) {
    let missing = agent
        .scratch()
        .join("no-such-folder")
        .to_string_lossy()
        .into_owned();
    dry_run(agent, &id, &missing, &[]).await;
}

#[when(expr = "the MCP client dry-runs the plan {string} over the demo folder with {string} set")]
async fn dry_runs_with_set(agent: &mut Agent, id: String, setting: String) {
    let folder = demo_folder(agent).to_string_lossy().into_owned();
    let line = match setting.as_str() {
        "custom-ignorefiles" => "custom-ignorefiles = [\".ignore\"]\n",
        other => &format!("{other} = true\n"),
    };
    dry_run_with(agent, &id, &folder, &[], line).await;
}

#[then(expr = "the error names the setting {string} alone")]
fn error_names_the_setting_alone(agent: &mut Agent, setting: String) {
    let refusal = mcp_steps::refusal_of(mcp_steps::result(agent));
    assert_eq!(
        refusal["args"],
        json!({ "setting": {"type": "text", "value": setting} }),
        "{refusal}"
    );
}

#[then(expr = "the result counts {int} file of {int} bytes")]
fn counts(agent: &mut Agent, files: u64, bytes: u64) {
    let content = mcp_steps::content(agent);
    assert_eq!(content["files"], files, "{content}");
    assert_eq!(content["bytes"], bytes, "{content}");
    assert!(content.get("stopped").is_none(), "{content}");
}

#[then(expr = "the result says the exclude {string} {int} removes {int} file of {int} bytes")]
fn exclude_removes(agent: &mut Agent, setting: String, entry: u64, files: u64, bytes: u64) {
    let content = mcp_steps::content(agent);
    assert_eq!(
        content["excludes"],
        json!([{ "setting": setting, "entry": entry, "files": files, "bytes": bytes }]),
        "{content}"
    );
}

#[then(expr = "the error names the setting {string} and the entry {int}")]
fn error_names_the_setting(agent: &mut Agent, setting: String, entry: u64) {
    let refusal = mcp_steps::refusal_of(mcp_steps::result(agent));
    assert_eq!(
        refusal["args"],
        json!({
            "setting": {"type": "text", "value": setting},
            "entry": {"type": "count", "value": entry},
        }),
        "{refusal}"
    );
    assert!(refusal.get("detail").is_none(), "{refusal}");
}

#[when(expr = "the API token sets the dry-run allowlist to {string} over HTTP")]
async fn token_sets(agent: &mut Agent, folder: String) {
    let request = super::request_checks_steps::Request {
        method: "PUT".to_owned(),
        path: ALLOWLIST_PATH.to_owned(),
        headers: vec![
            format!("Authorization: Bearer {}", agent.api_tokens.text()),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body: json!({ "folders": [folder] }).to_string(),
    };
    let response = api_tokens_steps::exchange(agent, request).await;
    agent.dry_run_refusal = Some(response);
}

#[then(expr = "the daemon refuses it with the status {int}")]
fn refuses_with(agent: &mut Agent, status: u16) {
    let response: &Response = agent
        .dry_run_refusal
        .as_ref()
        .expect("the API token sent a request");
    assert_eq!(response.status, status, "body {:?}", response.body);
}

#[then("the admin's session reads the dry-run allowlist as only the demo folder")]
async fn reads_only_the_demo_folder(agent: &mut Agent) {
    let folder = demo_folder(agent).to_string_lossy().into_owned();
    assert_eq!(allowlist(agent).await["folders"], json!([folder]));
}
