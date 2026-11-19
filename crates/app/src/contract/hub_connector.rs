//! The `HubConnector` port's contract (§7.1; §16, decision 120, "The
//! handshake"): what `pair` relies on whichever implementation carries its
//! frames. The hub at the other end runs [`HubPairing`]'s sessions on fakes.
//! The real adapter's own tests add what only a real connection shows: a
//! certificate this machine doesn't trust, or not the pinned one, is
//! refused.

use std::sync::Arc;

use api::sync::{AgentFrame, HubFrame};
use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{HubRoute, RouteSession};

use crate::fakes::{FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, SeededRandom};
use crate::pairing::{HubPairing, PROTOCOL_ERROR};
use crate::ports::{HubConnector, HubLink, LinkError};

#[async_trait]
pub trait HubConnectorHarness: Sync {
    /// A connector, and a route on which it reaches a hub running
    /// `pairing`'s sessions on its connections.
    async fn hub(&self, pairing: Arc<HubPairing>) -> (Arc<dyn HubConnector>, HubRoute);

    /// A connector, and a route on which nothing answers.
    async fn unanswered(&self) -> (Arc<dyn HubConnector>, HubRoute);
}

pub async fn run(harness: &dyn HubConnectorHarness) {
    a_connect_is_answered_with_the_hubs_challenge(harness).await;
    a_frame_out_of_turn_ends_the_link_with_its_reason(harness).await;
    only_a_pinned_route_has_an_exporter(harness).await;
    a_route_nothing_answers_on_is_unreachable(harness).await;
}

fn pairing() -> Arc<HubPairing> {
    let clock =
        FakeClock::new(Timestamp::from_second(1_800_000_000).unwrap_or(Timestamp::UNIX_EPOCH));
    Arc::new(HubPairing::new(
        Arc::new(InMemoryAgentRegistry::new()),
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(7)),
        Arc::new(clock),
    ))
}

/// A `Connect` from agent 1 with a zero nonce, of the binding kind the
/// link's session allows.
fn connect(link: &dyn HubLink) -> AgentFrame {
    AgentFrame::Connect {
        agent_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        agent_nonce: "A".repeat(43),
        binding_kind: u8::from(link.exporter().is_some()),
    }
}

/// A link to a new hub; `None`, after a failed assertion, if none opened.
async fn open(harness: &dyn HubConnectorHarness) -> Option<(Box<dyn HubLink>, HubRoute)> {
    let (connector, route) = harness.hub(pairing()).await;
    let link = connector.connect(&route).await;
    assert!(link.is_ok(), "no link to the hub: {:?}", link.err());
    Some((link.ok()?, route))
}

async fn a_connect_is_answered_with_the_hubs_challenge(harness: &dyn HubConnectorHarness) {
    let Some((mut link, _)) = open(harness).await else {
        return;
    };
    let frame = connect(link.as_ref());
    assert_eq!(link.send(&frame).await, Ok(()));
    let answer = link.receive().await;
    assert!(
        matches!(answer, Ok(HubFrame::Challenge { .. })),
        "a Connect is answered with a Challenge: {answer:?}"
    );
}

/// The hub closes the connection on a `Pair` before any `Connect`, and the
/// agent reads the reason it gave.
async fn a_frame_out_of_turn_ends_the_link_with_its_reason(harness: &dyn HubConnectorHarness) {
    let Some((mut link, _)) = open(harness).await else {
        return;
    };
    let early = AgentFrame::Pair {
        pairing_key: "A".repeat(43),
        agent_key: "A".repeat(43),
        counter: 0,
        hostname: "laptop".to_owned(),
        pairing_signature: "A".repeat(86),
        agent_signature: "A".repeat(86),
    };
    // The hub may close before the frame is through; what is read next is
    // what tells.
    let _ = link.send(&early).await;
    assert_eq!(
        link.receive().await,
        Err(LinkError::Closed {
            reason: Some(PROTOCOL_ERROR.to_owned())
        })
    );
}

async fn only_a_pinned_route_has_an_exporter(harness: &dyn HubConnectorHarness) {
    let Some((link, route)) = open(harness).await else {
        return;
    };
    assert_eq!(
        link.exporter().is_some(),
        matches!(route.session, RouteSession::Pinned(_)),
        "{route:?}"
    );
}

async fn a_route_nothing_answers_on_is_unreachable(harness: &dyn HubConnectorHarness) {
    let (connector, route) = harness.unanswered().await;
    let link = connector.connect(&route).await;
    assert!(
        matches!(link, Err(LinkError::Unreachable { .. })),
        "nothing answers on {route:?}: {:?}",
        link.err()
    );
}
