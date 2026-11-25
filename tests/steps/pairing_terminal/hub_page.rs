//! The hub's page: the routes it lists the machines and approves them
//! through, asked as the page asks them.

use cucumber::{given, then, when};

use super::{hub_key_printed, machine_key};
use crate::request_checks_steps::{Request, exchange};
use crate::{Agent, blocking};

/// A request to the serving hub as its page sends it, over plain HTTP on
/// loopback with sign-in off: the status and body of its answer.
pub(super) async fn page_asks(
    agent: &Agent,
    method: &str,
    path: &str,
    body: String,
) -> (u16, String) {
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
    let (status, body) = page_asks(agent, "GET", api::HUB_MACHINES_PATH, String::new()).await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"))
}

/// The hub's key as the page reads it, kept as the page's last answer.
async fn read_hub_key(agent: &mut Agent) -> api::HubDto {
    let answer = page_asks(agent, "GET", api::HUB_PATH, String::new()).await;
    let (status, body) = &answer;
    assert_eq!(*status, 200, "{body}");
    let hub = serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}"));
    agent.pairing_terminal.page_answer = Some(answer);
    hub
}

/// Approves `agent_id` under `key` on the hub's page, confirmed with
/// `confirmation`, and keeps the hub's answer.
async fn page_approves(agent: &mut Agent, agent_id: &str, key: String, confirmation: String) {
    let body = serde_json::to_string(&api::ApproveAgentDto { key, confirmation }).expect("JSON");
    let path = api::machine_approval_path(agent_id);
    let answer = page_asks(agent, "POST", &path, body).await;
    agent.pairing_terminal.page_answer = Some(answer);
}

/// Approves the machine named `hostname` under the key the page lists for
/// it, confirmed with the hub key's fingerprint as the page shows it.
async fn approves_on_the_page(agent: &mut Agent, hostname: &str) {
    let confirmation = read_hub_key(agent).await.fingerprint;
    approves_on_the_page_with(agent, hostname, confirmation).await;
}

async fn approves_on_the_page_with(agent: &mut Agent, hostname: &str, confirmation: String) {
    let listed = listed_machines(agent).await;
    let machine = listed
        .machines
        .into_iter()
        .find(|machine| machine.hostname == hostname)
        .unwrap_or_else(|| panic!("the page lists {hostname}"));
    page_approves(agent, &machine.machine_id, machine.key, confirmation).await;
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
    let (status, body) = page_asks(agent, "GET", api::HUB_MACHINES_PATH, String::new()).await;
    agent.pairing_terminal.page_answer = Some((status, body));
}

#[then(expr = "the page lists {string} as waiting with the key the machine showed")]
fn page_lists_waiting(agent: &mut Agent, hostname: String) {
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
    let listed: api::HubAgentsDto = serde_json::from_str(body).expect("the machines");
    let [machine] = listed.machines.as_slice() else {
        panic!("one machine: {body}");
    };
    assert_eq!(machine.hostname, hostname);
    assert_eq!(machine.status, api::AgentStatusDto::Pending);
    assert_eq!(machine.fingerprint, machine_key(agent));
}

#[when(
    expr = "the person approves {string} on the hub's page, confirming with the hub key's fingerprint"
)]
async fn approves_on_page(agent: &mut Agent, hostname: String) {
    approves_on_the_page(agent, &hostname).await;
}

#[when(
    expr = "the person approves {string} on the hub's page, confirming with the fingerprint the hub's terminal printed"
)]
async fn approves_on_page_with_printed(agent: &mut Agent, hostname: String) {
    let printed = hub_key_printed(agent);
    approves_on_the_page_with(agent, &hostname, printed).await;
}

#[when("the person reads the hub's key on the hub's page")]
async fn reads_hub_key(agent: &mut Agent) {
    read_hub_key(agent).await;
}

#[then(
    "the page shows the hub key as the fingerprint the hub's terminal printed, and no other ID for it"
)]
fn shows_printed_fingerprint(agent: &mut Agent) {
    let printed = hub_key_printed(agent);
    let (_, body) = page_answer(agent);
    let read: serde_json::Value = serde_json::from_str(body).expect("JSON");
    let fields = read.as_object().expect("an object");
    assert_eq!(
        fields
            .get("fingerprint")
            .and_then(serde_json::Value::as_str),
        Some(printed.as_str()),
        "{body}"
    );
    // The key is shown by its fingerprint alone: the page is given no
    // second value to show or ask for (#1344).
    let ids: Vec<_> = fields.keys().filter(|name| name.contains("id")).collect();
    assert!(ids.is_empty(), "{body}");
}

#[given(expr = "the person approved {string} on the hub's page")]
async fn approved_on_page_before(agent: &mut Agent, hostname: String) {
    approves_on_the_page(agent, &hostname).await;
    let (status, body) = page_answer(agent);
    assert_eq!(*status, 200, "{body}");
}

#[when(
    "the person approves an agent ID that never paired on the hub's page, confirming with the hub key's fingerprint"
)]
async fn approves_unknown_on_page(agent: &mut Agent) {
    // A UUIDv7 no machine made, under a key no machine holds.
    let key = format!("05{}", "0".repeat(62));
    let confirmation = read_hub_key(agent).await.fingerprint;
    page_approves(
        agent,
        "01890a5d-ac96-774b-bcce-b302099a8057",
        key,
        confirmation,
    )
    .await;
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
