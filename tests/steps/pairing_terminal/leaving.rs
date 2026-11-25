//! `ladderback unpair` at the machine: it leaves a lost or replaced hub,
//! and a token from another hub is refused in words until it does
//! (`features/fleet/leaving_a_hub.feature`; §16, decision 169).

use cucumber::{given, then, when};

use super::hub_terminal::make_token;
use super::{hub_address, hub_key_printed, hub_root, machine_root, on_machine, said, token};
use crate::Agent;

fn pin_kept(agent: &Agent) -> bool {
    testkit::daemon::data_dir(&machine_root(agent))
        .join("hub-pin")
        .exists()
}

/// The hub stops and its folders are gone, as a lost computer's are; a new
/// hub on new folders makes a token for the address the lost one served
/// on, so a machine pairs with it as it is, with no address of its own.
#[given(
    "the hub is lost, and a new hub with a pairing token for the same address made at its terminal takes its place"
)]
async fn hub_replaced(agent: &mut Agent) {
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.hub = None;
    agent.pairing_terminal.lost_hub_key = Some(hub_key_printed(agent));
    let root = hub_root(agent);
    std::fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("{}: {error}", root.display()));
    make_token(agent, &format!("http://{}", hub_address(agent))).await;
    assert_ne!(
        agent.pairing_terminal.lost_hub_key,
        Some(hub_key_printed(agent)),
        "the new hub has a key of its own"
    );
}

#[when(expr = "a machine named {string} pairs with the token as it is from its terminal")]
async fn pairs_as_it_is(agent: &mut Agent, hostname: String) {
    let pasted = format!("{}\n", token(agent));
    let said = on_machine(
        agent,
        &["pair", "--hostname", &hostname, "--yes"],
        Some(pasted),
    )
    .await;
    agent.pairing_terminal.said = Some(said);
}

#[when("the person unpairs the machine from its terminal with --yes")]
async fn unpairs(agent: &mut Agent) {
    let said = on_machine(agent, &["unpair", "--yes"], None).await;
    agent.pairing_terminal.said = Some(said);
}

#[when("the person unpairs the machine with no terminal to ask on")]
async fn unpairs_unasked(agent: &mut Agent) {
    let said = on_machine(agent, &["unpair"], None).await;
    agent.pairing_terminal.said = Some(said);
}

#[then(
    expr = "the machine refuses the token, since it pins the lost hub, naming both hubs' keys and {string}"
)]
fn refuses_pinned(agent: &mut Agent, command: String) {
    let lost = agent
        .pairing_terminal
        .lost_hub_key
        .clone()
        .expect("a lost hub");
    let new = hub_key_printed(agent);
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    for expected in [lost.as_str(), new.as_str(), command.as_str()] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
    assert!(!said.stderr.contains("PinnedToAnotherHub"), "{said:?}");
    assert!(pin_kept(agent), "the machine still pins the lost hub");
}

#[then("the machine says it left the hub and maintains its own repositories again")]
fn says_left(agent: &mut Agent) {
    let lost = agent
        .pairing_terminal
        .lost_hub_key
        .clone()
        .expect("a lost hub");
    let said = said(agent);
    assert!(said.success, "{said:?}");
    for expected in [
        "left the hub whose key is",
        lost.as_str(),
        "maintains its own repositories again",
    ] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
    assert!(!pin_kept(agent), "the pin is forgotten");
}

#[then("the terminal left the hub nothing and says to run again with --yes")]
fn left_nothing(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    for expected in ["still paired", "ladderback unpair --yes"] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
}

#[then("the machine still pins the hub")]
fn still_pins(agent: &mut Agent) {
    assert!(pin_kept(agent), "the machine still pins its hub");
}

#[then("the terminal refuses to unpair while the machine's daemon runs")]
fn refuses_while_running(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.success, "{said:?}");
    for expected in ["daemon is running", "Nothing changed"] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
}

#[then("the terminal says the machine isn't paired with a hub")]
fn says_unpaired(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    assert!(said.stderr.contains("isn't paired with a hub"), "{said:?}");
}

#[given("the machine's hub pin can't be read")]
fn pin_damaged(agent: &mut Agent) {
    let pin = testkit::daemon::data_dir(&machine_root(agent)).join("hub-pin");
    std::fs::write(&pin, b"not a pin\n")
        .unwrap_or_else(|error| panic!("{}: {error}", pin.display()));
}

#[then(
    "the machine says it left its hub, which it couldn't name, and maintains its own repositories again"
)]
fn says_left_unnamed(agent: &mut Agent) {
    let said = said(agent);
    assert!(said.success, "{said:?}");
    for expected in [
        "the file that names it can't be read",
        "left its hub",
        "maintains its own repositories again",
    ] {
        assert!(said.stderr.contains(expected), "{expected}: {said:?}");
    }
    assert!(!pin_kept(agent), "the pin is forgotten");
}
