//! The hub's terminal: `ladderback pairing-token` making, revoking and
//! listing tokens, listing the machines waiting and approving them, and
//! what the terminal says.

use app::ports::AgentRegistry as _;
use cucumber::{given, then, when};
use domain::Timestamp;
use domain::fleet::AgentStatus;
use store::SqliteStore;

use super::{hub_root, ladderback, machine_key, on_hub, said};
use crate::Agent;

/// The address a token is made for: loopback, which the scenarios' machines
/// replace with the port the hub serves on.
const TOKEN_ADDRESS: &str = "http://127.0.0.1:9";

/// The same over the hub's built-in TLS, whose certificate a token for it
/// pins.
const TLS_TOKEN_ADDRESS: &str = "https://127.0.0.1:9";

#[given("a hub with a pairing token made at its terminal")]
async fn token_made(agent: &mut Agent) {
    make_token(agent, TOKEN_ADDRESS).await;
}

#[given("a hub with a pairing token for its https address made at its terminal")]
async fn tls_token_made(agent: &mut Agent) {
    make_token(agent, TLS_TOKEN_ADDRESS).await;
}

#[given("the person made another pairing token at the hub's terminal")]
async fn another_token_made(agent: &mut Agent) {
    make_token(agent, TOKEN_ADDRESS).await;
}

async fn make_token(agent: &mut Agent, address: &str) {
    let said = ladderback(
        hub_root(agent),
        &["pairing-token", "--address", address],
        None,
    )
    .await;
    assert!(said.success, "{said:?}");
    let token = said.stdout.trim();
    assert!(token.starts_with("ladderback_pair_"), "{said:?}");
    agent.pairing_terminal.token = Some(token.to_owned());
}

#[given("the person revoked the token at the hub's terminal")]
async fn token_revoked(agent: &mut Agent) {
    let listed = ladderback(hub_root(agent), &["pairing-token", "--list"], None).await;
    let id = listed
        .stdout
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("a token ID: {listed:?}"))
        .to_owned();
    let said = ladderback(hub_root(agent), &["pairing-token", "--revoke", &id], None).await;
    assert!(said.success, "{said:?}");
}

#[when(expr = "the person approves {string} at the hub's terminal with --yes")]
async fn approves_with_yes(agent: &mut Agent, hostname: String) {
    let args = ["pairing-token", "--approve", &hostname, "--yes"];
    let said = ladderback(hub_root(agent), &args, None).await;
    agent.pairing_terminal.said = Some(said);
}

#[given(expr = "the person approved {string} at the hub's terminal")]
async fn approved_before(agent: &mut Agent, hostname: String) {
    approves_with_yes(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[when(expr = "the person approves {string} at the hub's terminal with no terminal to ask on")]
async fn approves_without_terminal(agent: &mut Agent, hostname: String) {
    let args = ["pairing-token", "--approve", &hostname];
    let said = ladderback(hub_root(agent), &args, None).await;
    agent.pairing_terminal.said = Some(said);
}

#[when("the person lists the machines waiting at the hub's terminal")]
async fn lists_waiting(agent: &mut Agent) {
    let said = on_hub(agent, &["pairing-token", "--pending"]).await;
    agent.pairing_terminal.said = Some(said);
}

#[given(expr = "the hub's terminal runs in the time zone {string}")]
fn hub_terminal_zone(agent: &mut Agent, zone: String) {
    agent.pairing_terminal.hub_zone = Some(zone);
}

#[when("the person lists the unused tokens at the hub's terminal")]
async fn lists_unused(agent: &mut Agent) {
    let said = on_hub(agent, &["pairing-token", "--list"]).await;
    agent.pairing_terminal.said = Some(said);
}

/// `at` as English writes a time in words in `zone`, the narrow no-break
/// space before `AM` folded to a plain one: the person's own clock, read
/// by `jiff` and not by the catalog's formatter.
fn in_words(at: Timestamp, zone: &str) -> String {
    let local = at.in_tz(zone).expect("a time zone the database knows");
    local.strftime("%B %-d, %Y at %-I:%M %p").to_string()
}

fn folded(text: &str) -> String {
    text.replace(['\u{202f}', '\u{a0}'], " ")
}

fn hub_terminal_zone_name(agent: &Agent) -> String {
    agent
        .pairing_terminal
        .hub_zone
        .clone()
        .expect("the hub's terminal runs in a time zone")
}

/// The hub's store, read while no daemon runs on it.
async fn hub_store(agent: &Agent) -> SqliteStore {
    let database = testkit::daemon::data_dir(&hub_root(agent)).join("ladderback.db");
    SqliteStore::open(&database).await.expect("the hub's store")
}

#[then(
    expr = "the terminal says the token is usable until its lapse time in that zone, in words, with the offset {string}"
)]
async fn says_lapse_in_zone(agent: &mut Agent, offset: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let store = hub_store(agent).await;
    let tokens = store.unused_tokens(Timestamp::now()).await.expect("tokens");
    store.close().await;
    let [token] = tokens.as_slice() else {
        panic!("one unused token: {tokens:?}");
    };
    let zone = hub_terminal_zone_name(agent);
    let expected = format!(
        "usable until {} ({offset})",
        in_words(token.expires_at, &zone)
    );
    assert!(
        folded(&said.stderr).contains(&expected),
        "{expected}: {said:?}"
    );
    assert!(
        !said.stderr.contains(&token.expires_at.to_string()),
        "no UTC timestamp: {said:?}"
    );
}

#[then(
    expr = "the terminal lists the token with its lapse time in that zone, in words, with the offset {string}"
)]
async fn lists_lapse_in_zone(agent: &mut Agent, offset: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let store = hub_store(agent).await;
    let tokens = store.unused_tokens(Timestamp::now()).await.expect("tokens");
    store.close().await;
    let [token] = tokens.as_slice() else {
        panic!("one unused token: {tokens:?}");
    };
    let zone = hub_terminal_zone_name(agent);
    let expected = format!(
        "{}  usable until {} ({offset})\n",
        token.id(),
        in_words(token.expires_at, &zone)
    );
    assert_eq!(folded(&said.stdout), expected, "{said:?}");
}

#[then(
    expr = "the terminal lists {string} as waiting since it paired, in that zone, in words, with the offset {string}"
)]
async fn lists_since_in_zone(agent: &mut Agent, hostname: String, offset: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let store = hub_store(agent).await;
    let agents = store.agents().await.expect("the hub's agents");
    store.close().await;
    let [listed] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    let AgentStatus::Pending { since } = listed.status else {
        panic!("a machine waiting: {listed:?}");
    };
    let zone = hub_terminal_zone_name(agent);
    // The agent ID, the key the machine showed and when it paired, then the
    // hostname, which alone may hold spaces.
    let expected = format!(
        "{}  {}  waiting since {} ({offset})  {hostname}\n",
        listed.agent_id.as_uuid(),
        machine_key(agent),
        in_words(since, &zone)
    );
    assert_eq!(folded(&said.stdout), expected, "{said:?}");
}

#[then(expr = "the terminal named {string} with the key the machine showed, and approved it")]
fn named_and_approved(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let key = machine_key(agent);
    assert!(
        said.stderr.contains(&format!("{hostname} (agent ID "))
            && said.stderr.contains(&format!("is waiting with key {key}")),
        "{said:?}"
    );
    assert!(
        said.stderr.contains(&format!("{hostname} is approved")),
        "{said:?}"
    );
}

#[then(expr = "the terminal lists {string} with the key the machine showed")]
fn lists_with_key(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let key = machine_key(agent);
    let lines: Vec<&str> = said.stdout.lines().collect();
    let [line] = lines.as_slice() else {
        panic!("one machine waits: {said:?}");
    };
    // The hostname comes last, since it alone may hold spaces.
    assert!(
        line.contains(&format!("  {key}  ")) && line.ends_with(&format!("  {hostname}")),
        "{said:?}"
    );
}

#[then("the terminal says no machine waits for approval")]
fn says_none_waiting(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(said.stdout.is_empty(), "{said:?}");
    assert!(
        said.stderr.contains("no machines waiting for approval"),
        "{said:?}"
    );
}

#[then("the terminal approved nothing and says to compare the key and run again with --yes")]
fn approved_nothing(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr
            .contains(&format!("is waiting with key {}", machine_key(agent))),
        "{said:?}"
    );
    assert!(
        said.stderr.contains("nothing was approved") && said.stderr.contains("--yes"),
        "{said:?}"
    );
}

#[then(expr = "the terminal refuses because {string} is approved already")]
fn refused_approved(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr
            .contains(&format!("{hostname} is approved already")),
        "{said:?}"
    );
    // An approved machine is refused before anything is asked.
    assert!(!said.stderr.contains("is waiting with key"), "{said:?}");
}

#[then(expr = "the terminal refuses because no machine named {string} paired with the hub")]
fn refused_unknown(agent: &mut Agent, hostname: String) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr.contains(&format!(
            "no machine named {hostname} has paired with this hub"
        )),
        "{said:?}"
    );
}

#[when("the person asks the hub's terminal for a pairing token")]
async fn token_asked(agent: &mut Agent) {
    ask_for_token(agent, &["pairing-token", "--address", TOKEN_ADDRESS]).await;
}

#[when(expr = "the person asks the hub's terminal for a pairing token for {string}")]
async fn token_asked_for(agent: &mut Agent, address: String) {
    ask_for_token(agent, &["pairing-token", "--address", &address]).await;
}

#[when(expr = "the person asks the hub's terminal for a pairing token lasting {word} minutes")]
async fn token_asked_lasting(agent: &mut Agent, minutes: String) {
    let args = [
        "pairing-token",
        "--address",
        TOKEN_ADDRESS,
        "--ttl",
        &minutes,
    ];
    ask_for_token(agent, &args).await;
}

async fn ask_for_token(agent: &mut Agent, args: &[&str]) {
    let said = on_hub(agent, args).await;
    agent.pairing_terminal.said = Some(said);
}

#[then("the hub's terminal lists no unused token")]
async fn no_unused_token(agent: &mut Agent) {
    agent.pairing_terminal.hub = None;
    let said = ladderback(hub_root(agent), &["pairing-token", "--list"], None).await;
    assert!(said.success, "{said:?}");
    assert_eq!(said.stdout, "", "{said:?}");
    assert!(said.stderr.contains("no unused pairing tokens"), "{said:?}");
}

#[then("the terminal refuses because the daemon holds the data folder")]
fn refused_while_running(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr
            .contains("the daemon is running on the data folder"),
        "{said:?}"
    );
}

#[then("the terminal refuses because the address isn't a hub address")]
fn refused_address(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(said.stderr.contains("isn't a hub address"), "{said:?}");
}

#[then("the terminal refuses because a token lives 5 to 1440 minutes")]
fn refused_life(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr.contains("a token lives 5 to 1440 minutes"),
        "{said:?}"
    );
}

#[then(
    "the terminal refuses because no machine pairs until the hub starts with its TLS key restored from a backup"
)]
fn refused_tls_key_lost(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    for expected in [
        "hub-tls-key",
        "pairs no machine until it starts with the key restored",
        "Restore the file from your backup",
        "start the hub",
    ] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
}

#[then("the terminal shows no token")]
fn no_token_shown(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.stdout.contains("ladderback_pair_"), "{said:?}");
    assert!(!said.stderr.contains("ladderback_pair_"), "{said:?}");
}
