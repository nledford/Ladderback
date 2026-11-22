//! The hub's page: the routes it lists the machines and approves them
//! through, asked as the page asks them.

use cucumber::{given, then, when};

use super::machine_key;
use crate::request_checks_steps::{Request, exchange};
use crate::{Agent, blocking};

/// A request to the serving hub as its page sends it, over plain HTTP on
/// loopback with sign-in off: the status and body of its answer.
async fn page_asks(agent: &Agent, method: &str, path: &str, body: String) -> (u16, String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    let request = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers: vec![
            "Origin: http://localhost:{port}".to_owned(),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body,
    };
    let answer = blocking(move || exchange(address, &request)).await;
    (answer.status, answer.body)
}

/// The machines the hub's page lists.
pub(super) async fn listed_machines(agent: &Agent) -> api::HubAgentsDto {
    let (status, body) = page_asks(agent, "GET", api::HUB_AGENTS_PATH, String::new()).await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"))
}

/// The hub key's key ID, which confirms an approval with sign-in off.
async fn hub_key_id(agent: &Agent) -> String {
    let (status, body) = page_asks(agent, "GET", api::HUB_PATH, String::new()).await;
    assert_eq!(status, 200, "{body}");
    let hub: api::HubDto = serde_json::from_str(&body).expect("the hub's key");
    hub.key_id
}

/// Approves `agent_id` under `key` on the hub's page, confirmed with the
/// hub key's key ID, and keeps the hub's answer.
async fn page_approves(agent: &mut Agent, agent_id: &str, key: String) {
    let confirmation = hub_key_id(agent).await;
    let body = serde_json::to_string(&api::ApproveAgentDto { key, confirmation }).expect("JSON");
    let path = api::agent_approval_path(agent_id);
    let answer = page_asks(agent, "POST", &path, body).await;
    agent.pairing_terminal.page_answer = Some(answer);
}

async fn approves_on_the_page(agent: &mut Agent, hostname: &str) {
    let listed = listed_machines(agent).await;
    let machine = listed
        .agents
        .into_iter()
        .find(|machine| machine.hostname == hostname)
        .unwrap_or_else(|| panic!("the page lists {hostname}"));
    page_approves(agent, &machine.agent_id, machine.key).await;
}

fn page_answer(agent: &Agent) -> &(u16, String) {
    agent
        .pairing_terminal
        .page_answer
        .as_ref()
        .expect("the page asked the hub")
}

#[when("the person reads the machines on the hub's page")]
async fn reads_machines(agent: &mut Agent) {
    let (status, body) = page_asks(agent, "GET", api::HUB_AGENTS_PATH, String::new()).await;
    agent.pairing_terminal.page_answer = Some((status, body));
}

#[then(expr = "the page lists {string} as waiting with the key the machine showed")]
fn page_lists_waiting(agent: &mut Agent, hostname: String) {
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
    let listed: api::HubAgentsDto = serde_json::from_str(body).expect("the machines");
    let [machine] = listed.agents.as_slice() else {
        panic!("one machine: {body}");
    };
    assert_eq!(machine.hostname, hostname);
    assert_eq!(machine.status, api::AgentStatusDto::Pending);
    assert_eq!(machine.fingerprint, machine_key(agent));
}

#[when(expr = "the person approves {string} on the hub's page, confirming with the hub's key ID")]
async fn approves_on_page(agent: &mut Agent, hostname: String) {
    approves_on_the_page(agent, &hostname).await;
}

#[given(expr = "the person approved {string} on the hub's page")]
async fn approved_on_page_before(agent: &mut Agent, hostname: String) {
    approves_on_the_page(agent, &hostname).await;
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
}

#[when(
    "the person approves an agent ID that never paired on the hub's page, confirming with the hub's key ID"
)]
async fn approves_unknown_on_page(agent: &mut Agent) {
    // A UUIDv7 no machine made, under a key no machine holds.
    let key = format!("05{}", "0".repeat(62));
    page_approves(agent, "01890a5d-ac96-774b-bcce-b302099a8057", key).await;
}

#[then(expr = "the hub answers that {string} is approved")]
fn answers_approved(agent: &mut Agent, hostname: String) {
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
    let machine: api::HubAgentDto = serde_json::from_str(body).expect("the machine");
    assert_eq!(machine.hostname, hostname);
    assert_eq!(machine.status, api::AgentStatusDto::Approved);
    assert_eq!(machine.fingerprint, machine_key(agent));
}

#[then(expr = "the hub refuses with the code {string}")]
fn refuses_with_code(agent: &mut Agent, code: String) {
    let (status, body) = page_answer(agent);
    let answer: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    assert_eq!(
        answer["code"].as_str(),
        Some(code.as_str()),
        "{status} {body}"
    );
}
