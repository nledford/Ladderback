//! Steps for `features/fleet/staying_connected.feature`: an approved
//! machine's sync loop holding one connection to its hub, with a heartbeat
//! every 30 seconds, and the hub showing it online while it holds that
//! connection (technical design §7.2; §16, decision 159). The loop runs on
//! a clock of its own, which the steps move from one of its waits to the
//! next; the hub's transport, which closes a connection silent for 90
//! seconds, is the loopback connector watching the hub's clock.

use api::UiEvent;
use api::sync::AgentFrame;
use app::ports::Clock as _;
use cucumber::{given, then, when};
use domain::SignedDuration;
use futures::{FutureExt as _, StreamExt as _};

use super::Agent;
use super::sync_replay::{WAKES, first};

/// Whether the hub shows the first agent online, as its dashboard does.
async fn online(agent: &mut Agent) -> bool {
    let id = agent.pairing.first().agent_id;
    let machines = agent.pairing.hub().pairing.machines().await.unwrap();
    machines
        .iter()
        .find(|machine| machine.record.agent_id == id)
        .expect("the agent on the hub's dashboard")
        .online
}

/// How far the agent's own clock moved since its loop started.
fn agent_time(agent: &mut Agent) -> SignedDuration {
    let running = first(agent)
        .running
        .as_ref()
        .expect("the agent keeps syncing");
    running.clock.monotonic()
}

#[given("the agent is connected to its hub")]
async fn is_connected(agent: &mut Agent) {
    for _ in 0..WAKES {
        if online(agent).await {
            return;
        }
        let running = first(agent)
            .running
            .as_ref()
            .expect("the agent keeps syncing");
        assert!(running.wake().await, "the agent's loop stopped");
    }
    panic!("the agent never came online");
}

#[when("an hour passes with nothing to report")]
async fn an_hour_passes(agent: &mut Agent) {
    let hour = SignedDuration::from_hours(1);
    while agent_time(agent) < hour {
        let running = first(agent)
            .running
            .as_ref()
            .expect("the agent keeps syncing");
        assert!(running.wake().await, "the agent's loop stopped");
    }
    first(agent).running.as_ref().unwrap().settle().await;
}

#[then(regex = r"^the agent connected to its hub (once|twice)$")]
async fn connected_times(agent: &mut Agent, times: String) {
    let expected = if times == "once" { 1 } else { 2 };
    assert_eq!(first(agent).connector.routes().len(), expected);
}

#[then(regex = r"^the agent sent its hub (\d+) heartbeats, the first as it connected$")]
async fn heartbeats_sent(agent: &mut Agent, count: usize) {
    let sent = first(agent).connector.sent();
    // The first message after the handshake's.
    let first_signed = sent
        .iter()
        .find_map(|text| match AgentFrame::from_text(text) {
            Ok(AgentFrame::Signed { kind, .. }) if kind != "Hello" => Some(kind),
            _ => None,
        });
    assert_eq!(first_signed.as_deref(), Some("Heartbeat"));
    let heartbeats = sent
        .iter()
        .filter(|text| {
            matches!(
                AgentFrame::from_text(text),
                Ok(AgentFrame::Signed { kind, .. }) if kind == "Heartbeat"
            )
        })
        .count();
    assert_eq!(heartbeats, count);
}

#[then(regex = r"^the hub shows the agent (online|offline)$")]
async fn shows(agent: &mut Agent, shown: String) {
    assert_eq!(online(agent).await, shown == "online");
}

#[when("the hub drops the agent's connection")]
async fn hub_drops(agent: &mut Agent) {
    first(agent).connector.drop_links().await;
}

#[then(
    "the agent connects to its hub again within 31 seconds, 1 second after its next heartbeat found the connection closed"
)]
async fn connects_again(agent: &mut Agent) {
    let dropped = agent_time(agent);
    let mut found = None;
    for _ in 0..WAKES {
        let side = first(agent);
        let running = side.running.as_ref().expect("the agent keeps syncing");
        if side.connector.routes().len() == 2 {
            found = Some(running.clock.monotonic());
            break;
        }
        assert!(running.wake().await, "the agent's loop stopped");
        // The connection a wake made is there once the loop waits again.
        running.settle().await;
    }
    let again = found.expect("the agent connected again") - dropped;
    let heartbeat = SignedDuration::from_secs(30);
    assert!(
        again <= heartbeat + SignedDuration::from_secs(1),
        "{again:?}"
    );
    let since_start = found.unwrap().as_secs();
    assert_eq!(
        since_start % 30,
        1,
        "1 second after a heartbeat: {since_start}"
    );
    first(agent).running.as_ref().unwrap().settle().await;
}

#[when("the agent stops sending anything")]
async fn stops_sending(agent: &mut Agent) {
    let running = first(agent)
        .running
        .take()
        .expect("the agent keeps syncing");
    drop(running);
}

#[when(regex = r"^(\d+) (?:more )?seconds? pass(?:es)? on the hub's clock$")]
async fn hub_time_passes(agent: &mut Agent, seconds: i64) {
    agent
        .pairing
        .hub()
        .clock
        .advance(SignedDuration::from_secs(seconds));
    first(agent).connector.close_silent().await;
}

#[then("the hub holds no connection of the agent")]
async fn no_connection(agent: &mut Agent) {
    assert_eq!(first(agent).connector.open_links(), 0);
}

#[when("the machine checks its hub from its terminal")]
async fn terminal_checks(agent: &mut Agent) {
    let machine = agent.pairing.first().clone();
    let connector = std::sync::Arc::new(app::fakes::LoopbackHubConnector::to(
        agent.pairing.hub().pairing.clone(),
    ));
    let checked = machine
        .connecting(connector)
        .connect_channel(
            machine.agent_id,
            machine.hostname.clone(),
            machine.zone.clone(),
        )
        .await;
    assert!(checked.is_ok(), "the terminal's check: {:?}", checked.err());
}

#[when("a minute passes on the agent's clock")]
async fn a_minute_passes(agent: &mut Agent) {
    let until = agent_time(agent) + SignedDuration::from_mins(1);
    while agent_time(agent) < until {
        let running = first(agent)
            .running
            .as_ref()
            .expect("the agent keeps syncing");
        assert!(running.wake().await, "the agent's loop stopped");
    }
    first(agent).running.as_ref().unwrap().settle().await;
}

/// The online and offline events the hub's browsers heard of the first
/// agent so far, in order.
fn presence_heard(agent: &mut Agent) -> (String, Vec<(&'static str, String)>) {
    let id = agent.pairing.first().agent_id.as_uuid().to_string();
    let hub = agent.pairing.hub();
    let mut heard = Vec::new();
    {
        let mut stream = hub.heard.lock().unwrap();
        while let Some(Some(event)) = stream.next().now_or_never() {
            heard.push(event);
        }
    }
    let presence: Vec<_> = heard
        .into_iter()
        .filter_map(|event| match event {
            UiEvent::AgentOnline { agent, .. } => Some(("online", agent)),
            UiEvent::AgentOffline { agent, .. } => Some(("offline", agent)),
            _ => None,
        })
        .collect();
    (id, presence)
}

#[then("the hub's browsers heard the agent come online, then go offline")]
async fn heard_online_then_offline(agent: &mut Agent) {
    let (id, presence) = presence_heard(agent);
    assert_eq!(presence, [("online", id.clone()), ("offline", id)]);
}

#[then("the hub's browsers heard the agent come online and nothing more")]
async fn heard_online_only(agent: &mut Agent) {
    let (id, presence) = presence_heard(agent);
    assert_eq!(presence, [("online", id)]);
}
