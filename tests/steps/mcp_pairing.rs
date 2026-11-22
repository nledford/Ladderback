//! Steps for `features/automation/mcp_pairing.feature`: the pairing guide,
//! the resource `ladderback://pairing`, and `pairing_state`, called on the
//! scenario's MCP server through the steps of `mcp.rs`, on a machine the
//! steps of `pairing_terminal/` paired.

use api::{ErrorCode, MsgArg};
use cucumber::{given, then, when};
use domain::fleet::{Fingerprint, HubAddress, PairingToken};
use l10n::Localizer;
use serde_json::{Value, json};

use super::Agent;
use super::mcp_steps::{ask, content, current, result, shows_none, start_serving};
use super::pairing_terminal_steps::{hub_address, machine_root, token};

/// What a refusal in the guide says where its sentence names something
/// only a real refusal knows: the placeholders a person reads as such.
const PLACEHOLDERS: [(&str, &str); 3] = [
    ("address", "<address>"),
    ("hostname", "<hostname>"),
    ("last_seen", "<when>"),
];

#[when("the MCP client reads the pairing guide")]
async fn reads_the_guide(agent: &mut Agent) {
    ask(
        agent,
        "resources/read",
        current(json!({"uri": "ladderback://pairing"})),
    )
    .await;
}

/// The guide the latest read answered, read from its one text content.
fn guide(agent: &Agent) -> Value {
    let contents = &result(agent)["contents"];
    assert_eq!(contents[0]["uri"], "ladderback://pairing", "{contents}");
    assert_eq!(contents[0]["mimeType"], "application/json", "{contents}");
    let text = contents[0]["text"].as_str().expect("the guide as text");
    serde_json::from_str(text).expect("the guide is JSON")
}

/// The guide's step named `name`.
fn step(guide: &Value, name: &str) -> Value {
    guide["steps"]
        .as_array()
        .expect("a list of steps")
        .iter()
        .find(|step| step["step"] == name)
        .unwrap_or_else(|| panic!("no step {name:?}: {guide}"))
        .clone()
}

#[then(
    expr = "the guide's steps are {string} on the hub, {string} on the machine and {string} on the hub"
)]
fn steps_are(agent: &mut Agent, token: String, pair: String, approve: String) {
    let guide = guide(agent);
    let steps: Vec<(&str, &str)> = guide["steps"]
        .as_array()
        .expect("a list of steps")
        .iter()
        .map(|step| {
            (
                step["step"].as_str().expect("a step's name"),
                step["on"].as_str().expect("where it is taken"),
            )
        })
        .collect();
    assert_eq!(
        steps,
        [
            (token.as_str(), "hub"),
            (pair.as_str(), "machine"),
            (approve.as_str(), "hub"),
        ]
    );
    for step in guide["steps"].as_array().expect("a list of steps") {
        let says = step["says"].as_str().expect("what the step says");
        assert!(!says.is_empty(), "{step}");
    }
}

#[then("every refusal the guide names is a code Ladderback has, said in the catalog's words")]
fn refusals_are_codes(agent: &mut Agent) {
    let guide = guide(agent);
    let mut named = 0;
    for step in guide["steps"].as_array().expect("a list of steps") {
        for refusal in step["refusals"].as_array().expect("a list of refusals") {
            let code: ErrorCode = serde_json::from_value(refusal["code"].clone())
                .unwrap_or_else(|error| panic!("{error}: {refusal}"));
            assert_ne!(code, ErrorCode::Unknown, "{refusal}");
            assert_eq!(refusal["says"], catalog(code), "{refusal}");
            named += 1;
        }
    }
    assert!(named > 0, "the guide names no refusal: {guide}");
}

/// What the catalog says of `code` in English, with the placeholders.
fn catalog(code: ErrorCode) -> String {
    let args: Vec<(&str, MsgArg)> = PLACEHOLDERS
        .iter()
        .map(|(name, value)| (*name, MsgArg::Text((*value).to_owned())))
        .collect();
    Localizer::new(&[]).error(code, &args, None).into_text()
}

#[then(expr = "the step {string} names the refusal {string}, saying {string}")]
fn step_names_the_refusal(agent: &mut Agent, name: String, code: String, says: String) {
    let step = step(&guide(agent), &name);
    let refusal = step["refusals"]
        .as_array()
        .expect("a list of refusals")
        .iter()
        .find(|refusal| refusal["code"] == code.as_str())
        .unwrap_or_else(|| panic!("no refusal {code:?}: {step}"))
        .clone();
    assert_eq!(refusal["says"], says.as_str(), "{refusal}");
}

#[then(expr = "the step {string} says the hub holds the machine waiting for approval")]
fn step_says_waiting(agent: &mut Agent, name: String) {
    let step = step(&guide(agent), &name);
    let says = step["says"].as_str().expect("what the step says");
    assert!(
        says.contains("holds the machine waiting until a person approves it"),
        "{step}"
    );
    assert!(says.contains("ladderback hub-status"), "{step}");
}

#[given("an MCP server running headless on the machine's folders")]
async fn headless_on_the_machine(agent: &mut Agent) {
    let root = machine_root(agent);
    start_serving(agent, root, &["--headless"]).await;
}

#[then("the pairing state says this machine isn't paired with a hub")]
fn not_paired(agent: &mut Agent) {
    assert_eq!(*content(agent), json!({"paired": false}));
}

#[then(
    "the pairing state names the hub's address and its key's fingerprint, and no certificate, since the address isn't encrypted"
)]
fn names_the_hub(agent: &mut Agent) {
    let token = PairingToken::parse_pasted(token(agent)).expect("the scenario's token");
    let address =
        HubAddress::parse(&format!("http://{}", hub_address(agent))).expect("the hub's address");
    assert_eq!(
        *content(agent),
        json!({
            "paired": true,
            "hub": {
                "address": address.as_str(),
                "fingerprint": Fingerprint::of_key(token.hub_key()).to_string(),
            },
        })
    );
}

/// The token as pasted, and its pairing key's seed, the one secret it
/// carries, in hex as a key file would hold it (I4).
#[then("nothing the MCP server wrote shows the pairing token")]
fn shows_no_token(agent: &mut Agent) {
    let text = token(agent).to_owned();
    let parsed = PairingToken::parse_pasted(&text).expect("the scenario's token");
    let seed = parsed.pairing_seed().to_hex().to_string();
    shows_none(agent, &[&text, &seed]);
}
