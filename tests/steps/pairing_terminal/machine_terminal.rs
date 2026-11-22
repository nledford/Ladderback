//! The machine's terminal: `ladderback pair` with a token or a new address,
//! `ladderback hub-status`, and what the machine says and keeps.

use std::net::SocketAddr;

use cucumber::{given, then, when};

use super::{machine_root, on_machine, said};
use crate::Agent;

async fn pair(agent: &mut Agent, hostname: &str, address: &str) {
    let token = agent.pairing_terminal.token.clone().expect("a token");
    pair_pasting(agent, hostname, address, format!("{token}\n")).await;
}

async fn pair_pasting(agent: &mut Agent, hostname: &str, address: &str, pasted: String) {
    let said = on_machine(
        agent,
        &["pair", address, "--hostname", hostname, "--yes"],
        Some(pasted),
    )
    .await;
    agent.pairing_terminal.said = Some(said);
}

/// `ladderback pair <address>` on the paired machine, with nothing on its
/// standard input: no token.
async fn give_address(agent: &mut Agent, address: SocketAddr) {
    let said = on_machine(agent, &["pair", &format!("https://{address}")], None).await;
    agent.pairing_terminal.said = Some(said);
}

#[when("the person gives the machine the hub's new address from its terminal")]
async fn gives_new_address(agent: &mut Agent) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    give_address(agent, address).await;
}

#[when("the person gives the machine the other hub's address from its terminal")]
async fn gives_other_address(agent: &mut Agent) {
    let address = agent
        .pairing_terminal
        .other_address
        .expect("another serving hub");
    give_address(agent, address).await;
}

#[then("the machine says it keeps the hub's new address")]
fn says_address_kept(agent: &mut Agent) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let connected = format!("pinned at https://{address}");
    for expected in [connected.as_str(), "reaches it there from now on"] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
}

#[then("the machine says it kept the hub addresses it had")]
fn says_addresses_unchanged(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(
        said.stderr.contains("kept the hub addresses it had"),
        "{said:?}"
    );
}

#[when(expr = "a machine named {string} pairs with the token from its terminal")]
async fn machine_pairs(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    pair(agent, &hostname, &format!("http://{address}")).await;
}

#[when(
    expr = "a machine named {string} pairs with the token at the hub's https address from its terminal"
)]
async fn machine_pairs_over_tls(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    pair(agent, &hostname, &format!("https://{address}")).await;
}

#[when(
    expr = "a machine named {string} pairs with the token at the other hub's https address from its terminal"
)]
async fn machine_pairs_with_other(agent: &mut Agent, hostname: String) {
    let address = agent
        .pairing_terminal
        .other_address
        .expect("another serving hub");
    pair(agent, &hostname, &format!("https://{address}")).await;
}

#[given(expr = "a machine named {string} paired with the token from its terminal")]
async fn machine_paired_before(agent: &mut Agent, hostname: String) {
    machine_pairs(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let key = said
        .stderr
        .split("This machine's key: ")
        .nth(1)
        .and_then(|rest| rest.split(". ").next())
        .unwrap_or_else(|| panic!("the machine showed its key: {said:?}"))
        .to_owned();
    agent.pairing_terminal.machine_key = Some(key);
}

#[when(
    expr = "a machine named {string} in the time zone {string} pairs with the token from its terminal"
)]
async fn machine_in_zone_pairs(agent: &mut Agent, hostname: String, zone: String) {
    agent.pairing_terminal.machine_zone = Some(zone);
    machine_pairs(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[given(
    expr = "a machine named {string} in the time zone {string} paired with the token from its terminal"
)]
async fn machine_in_zone_paired_before(agent: &mut Agent, hostname: String, zone: String) {
    agent.pairing_terminal.machine_zone = Some(zone);
    machine_paired_before(agent, hostname).await;
}

#[then("the machine says the hub approved it")]
fn says_approved(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(
        said.stderr
            .contains("Connected. The hub approved this machine."),
        "{said:?}"
    );
}

#[given(
    expr = "a machine named {string} paired with the token at the hub's https address from its terminal"
)]
async fn machine_paired_over_tls_before(agent: &mut Agent, hostname: String) {
    machine_pairs_over_tls(agent, hostname).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[then("the machine read a token pinned to the hub's own certificate")]
fn read_pinned(agent: &mut Agent) {
    let said = said(agent);
    assert!(
        said.stderr
            .contains("its certificate is pinned by the token"),
        "{said:?}"
    );
}

#[then("the machine read a token for an address that isn't encrypted, naming no certificate")]
fn read_plain(agent: &mut Agent) {
    let said = said(agent);
    assert!(
        said.stderr.contains("The address isn't encrypted"),
        "{said:?}"
    );
    assert!(!said.stderr.contains("certificate"), "{said:?}");
}

#[when("the machine checks its link to the hub from its terminal")]
async fn machine_checks(agent: &mut Agent) {
    let said = on_machine(agent, &["hub-status"], None).await;
    agent.pairing_terminal.said = Some(said);
}

#[when(
    expr = "the machine, now in the time zone {string}, checks its link to the hub from its terminal"
)]
async fn moved_machine_checks(agent: &mut Agent, zone: String) {
    agent.pairing_terminal.machine_zone = Some(zone);
    machine_checks(agent).await;
    let said = said(agent);
    assert!(said.success, "{said:?}");
}

#[when(expr = "a machine pairs with the token at the address {string} from its terminal")]
async fn machine_pairs_at(agent: &mut Agent, address: String) {
    pair(agent, "e2e-laptop", &address).await;
}

#[when(
    expr = "a machine named {string} pairs with the token pasted across two lines from its terminal"
)]
async fn machine_pairs_wrapped(agent: &mut Agent, hostname: String) {
    let address = agent.pairing_terminal.address.expect("a serving hub");
    let token = agent.pairing_terminal.token.clone().expect("a token");
    let (first, second) = token.split_at(token.len() / 2);
    let pasted = format!("{first}\n{second}\n");
    pair_pasting(agent, &hostname, &format!("http://{address}"), pasted).await;
}

#[then("the machine is paired and waits for approval")]
fn machine_paired(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(
        said.stderr.contains("The hub holds this machine pending"),
        "{said:?}"
    );
}

#[then(regex = r"^the machine is refused because (.+)$")]
fn machine_refused(agent: &mut Agent, reason: String) {
    let expected = match reason.as_str() {
        "the token was revoked" => "This token was canceled on the hub",
        "the address would show the token to the network" => "shows the token to the network",
        "the hub's certificate isn't the one the token names" => {
            "showed a certificate that isn't the one its token names"
        }
        "the hub can't be reached" => "couldn't reach the hub at",
        other => panic!("no refusal is named {other:?}"),
    };
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(said.stderr.contains(expected), "{said:?}");
}

#[then("the machine says the hub holds it waiting for approval")]
fn says_pending(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(
        said.stderr
            .contains("The hub holds this machine waiting for approval"),
        "{said:?}"
    );
}

#[then("the machine says it isn't paired with a hub")]
fn says_not_paired(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    assert!(said.stderr.contains("isn't paired with a hub"), "{said:?}");
}

/// A command that only checks writes nothing, not even the agent ID a
/// machine gets when it first pairs or serves.
#[then("the machine's data folder holds nothing")]
fn data_folder_empty(agent: &mut Agent) {
    let data = testkit::daemon::data_dir(&machine_root(agent));
    let held: Vec<_> = match std::fs::read_dir(&data) {
        Ok(entries) => entries.map(|entry| entry.unwrap().file_name()).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(error) => panic!("{}: {error}", data.display()),
    };
    assert!(held.is_empty(), "{held:?}");
}

#[then(expr = "the machine says the hub holds it under the hostname {string}, not {string}")]
fn says_held_hostname(agent: &mut Agent, held: String, asked: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    let expected = format!("under the hostname {held} and not {asked}");
    assert!(said.stderr.contains(&expected), "{expected}: {said:?}");
}

#[then(
    expr = "the machine says the hub approved it already, under the hostname {string}, not {string}"
)]
fn says_approved_held(agent: &mut Agent, held: String, asked: String) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    for expected in [
        "The hub approved this machine already".to_owned(),
        format!("under the hostname {held} and not {asked}"),
    ] {
        assert!(said.stderr.contains(&expected), "{expected}: {said:?}");
    }
    assert!(!said.stderr.contains("pending"), "{said:?}");
}

#[given("the machine lost the hub's reply, so it holds no pin")]
fn machine_holds_no_pin(agent: &mut Agent) {
    let pin = testkit::daemon::data_dir(&machine_root(agent)).join("hub-pin");
    std::fs::remove_file(&pin).unwrap_or_else(|error| panic!("{}: {error}", pin.display()));
}
