//! Steps for `features/automation/mcp_proposals.feature`: an MCP server
//! connected to the real daemon proposes, reads and withdraws configuration
//! changes with the tools `propose_setup`, `get_proposal` and
//! `withdraw_proposal`, and the steps check what the daemon kept against
//! what the admin's session reads (decision 89, §4.6).

use cucumber::{given, then, when};
use serde_json::{Value, json};

use super::{Agent, api_tokens_steps, mcp_steps, proposals_steps};

/// Calls `propose_setup` with one change adding the plan `id` over
/// `source` into the repository `demo`, and keeps the proposal's ID when
/// the tool made one.
async fn propose(agent: &mut Agent, id: &str, source: &str) {
    let arguments = json!({
        "changes": [{
            "kind": "add-plan",
            "plan": { "id": id, "repository": "demo", "sources": [source] },
        }],
        "rationale": "Back up the demo home folder every night.",
    });
    mcp_steps::call(agent, "propose_setup".to_owned(), arguments).await;
    let result = mcp_steps::result(agent);
    if result["isError"] == false {
        let made = result["structuredContent"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("no proposal ID: {result}"))
            .to_owned();
        proposals_steps::keep(agent, made);
    }
}

#[when(expr = "the MCP client proposes adding the plan {string} over {string}")]
async fn proposes(agent: &mut Agent, id: String, source: String) {
    propose(agent, &id, &source).await;
}

#[given(expr = "the MCP client has proposed adding the plan {string} over {string}")]
async fn has_proposed(agent: &mut Agent, id: String, source: String) {
    propose(agent, &id, &source).await;
    assert_eq!(
        mcp_steps::content(agent)["status"],
        "pending",
        "{}",
        mcp_steps::result(agent)
    );
}

/// Calls `tool` with the kept proposal's ID as its `proposal`.
async fn call_with_the_proposal(agent: &mut Agent, tool: &str) {
    let arguments = json!({ "proposal": proposals_steps::the_proposal(agent) });
    mcp_steps::call(agent, tool.to_owned(), arguments).await;
}

#[when("the MCP client reads that proposal")]
async fn reads(agent: &mut Agent) {
    call_with_the_proposal(agent, "get_proposal").await;
}

#[when("the MCP client withdraws that proposal")]
async fn withdraws(agent: &mut Agent) {
    call_with_the_proposal(agent, "withdraw_proposal").await;
}

#[when(expr = "the MCP client calls the tool {string} with that proposal")]
async fn calls_with_the_proposal(agent: &mut Agent, tool: String) {
    call_with_the_proposal(agent, &tool).await;
}

#[then(expr = "the result is a proposal {string}, proposed by {string}")]
fn is_a_proposal(agent: &mut Agent, status: String, label: String) {
    let content = mcp_steps::content(agent);
    assert_eq!(
        content["id"],
        proposals_steps::the_proposal(agent).as_str(),
        "{content}"
    );
    assert_eq!(content["status"], status.as_str(), "{content}");
    assert_eq!(content["proposed-by"], label.as_str(), "{content}");
}

/// No tool of that name: a protocol error, invalid params, as for any name
/// the server doesn't list, never a tool's refusal.
#[then("the server answers that it has no such tool")]
fn no_such_tool(agent: &mut Agent) {
    let answer = mcp_steps::answer(agent);
    assert!(answer.get("result").is_none(), "{answer}");
    assert_eq!(answer["error"]["code"], json!(-32602), "{answer}");
    let message = answer["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("no tool is named"), "{answer}");
}

#[then(expr = "the admin's session reads that proposal as {string}")]
async fn session_reads(agent: &mut Agent, status: String) {
    let path = format!("/api/v1/proposals/{}", proposals_steps::the_proposal(agent));
    let request = api_tokens_steps::in_session(agent, "GET", &path, String::new());
    let response = api_tokens_steps::exchange(agent, request).await;
    assert_eq!(response.status, 200, "body {:?}", response.body);
    let read: Value = serde_json::from_str(&response.body).expect("a proposal as JSON");
    assert_eq!(read["status"], status.as_str(), "{read}");
}
