//! The `@e2e` scenario of `features/fleet/staying_connected.feature`: a
//! paired, approved machine's daemon holding its connection to a real hub,
//! which shows it online on its page while it does (§7.2; §16, decision
//! 159). The machine finds a restarted hub's connection closed at its next
//! heartbeat, 30 seconds at most, and connects again after a second.

use std::time::{Duration, Instant};

use cucumber::{then, when};

use super::hub_page::page_asks;
use super::machine_root;
use crate::Agent;
use crate::request_checks_steps::{Outcome, start};

/// How long the hub may take to show a change: a heartbeat's 30 seconds,
/// the second's backoff after it, and slack for a loaded runner.
const SHOWN_WITHIN: Duration = Duration::from_secs(45);

#[when("the machine's daemon starts")]
async fn machine_starts(agent: &mut Agent) {
    let (child, outcome, _log) = start(
        agent.scratch(),
        Some(&machine_root(agent)),
        &["--listen", "127.0.0.1:0"],
        None,
    )
    .await;
    let Outcome::Listening(_) = outcome else {
        panic!("the machine's daemon didn't start: {outcome:?}");
    };
    agent.pairing_terminal.machine = Some(child);
}

#[when("the machine's daemon stops")]
fn machine_stops(agent: &mut Agent) {
    // Dropping the daemon ends it and waits for it.
    agent.pairing_terminal.machine = None;
}

#[then(expr = "the hub soon shows {string} {word}")]
async fn hub_soon_shows(agent: &mut Agent, hostname: String, shown: String) {
    let online = match shown.as_str() {
        "online" => true,
        "offline" => false,
        other => panic!("online or offline, not {other}"),
    };
    let started = Instant::now();
    let mut last = String::new();
    while started.elapsed() < SHOWN_WITHIN {
        let (status, body) = page_asks(agent, "GET", api::HUB_MACHINES_PATH, String::new()).await;
        assert_eq!(status, 200, "{body}");
        let machines: api::HubMachinesDto =
            serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"));
        if machines
            .machines
            .iter()
            .any(|machine| machine.hostname == hostname && machine.online == online)
        {
            return;
        }
        last = body;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("the hub didn't show {hostname} {shown} within {SHOWN_WITHIN:?}: {last}");
}
