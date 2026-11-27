//! The `HubConnector` port's contract (§7.1; §16, decision 120, "The
//! handshake"): what `pair` relies on whichever implementation carries its
//! frames. The hub at the other end runs [`HubPairing`]'s sessions on fakes.
//! The real adapter's own tests add what only a real connection shows: a
//! certificate this machine doesn't trust, or not the pinned one, is
//! refused.

use std::sync::Arc;

use api::sync::{AgentFrame, HubFrame};
use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
    frames_the_agent_has_not_read_are_kept_in_order(harness).await;
    a_listen_given_up_loses_no_frame(harness).await;
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
        "0.0.0-test".to_owned(),
    ))
}

/// A `Connect` from agent 1 with a zero nonce, of the binding kind the
/// link's session allows.
fn connect(link: &dyn HubLink) -> AgentFrame {
    AgentFrame::Connect {
        machine_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        machine_nonce: "A".repeat(43),
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

/// A `Pair` that reads as one but that no `Challenge` of the hub's was made
/// for, so no proof in it holds.
fn a_pair_no_challenge_was_made_for() -> AgentFrame {
    let encoded = |bytes: &[u8]| URL_SAFE_NO_PAD.encode(bytes);
    AgentFrame::Pair {
        pairing_key: encoded(&[7; 32]),
        machine_key: encoded(&[8; 32]),
        counter: 0,
        hostname: "laptop".to_owned(),
        zone: None,
        pairing_signature: encoded(&[9; 64]),
        machine_signature: encoded(&[10; 64]),
    }
}

/// What the hub sent before the agent read any of it is read in the order
/// it was sent: a `Connect` and a `Pair` sent back to back are answered by
/// the `Challenge` and then the refusal, not the refusal alone.
async fn frames_the_agent_has_not_read_are_kept_in_order(harness: &dyn HubConnectorHarness) {
    let Some((mut link, _)) = open(harness).await else {
        return;
    };
    let frame = connect(link.as_ref());
    assert_eq!(link.send(&frame).await, Ok(()));
    // The hub may close before the frame is through.
    let _ = link.send(&a_pair_no_challenge_was_made_for()).await;
    let first = link.receive().await;
    assert!(
        matches!(first, Ok(HubFrame::Challenge { .. })),
        "the first frame read is the Challenge: {first:?}"
    );
    let second = link.receive().await;
    assert!(
        matches!(second, Ok(HubFrame::Refused { .. })),
        "the second is the refusal: {second:?}"
    );
}

/// The hub closes the connection on a `Pair` before any `Connect`, and the
/// agent reads the reason it gave.
async fn a_frame_out_of_turn_ends_the_link_with_its_reason(harness: &dyn HubConnectorHarness) {
    let Some((mut link, _)) = open(harness).await else {
        return;
    };
    let early = a_pair_no_challenge_was_made_for();
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

/// A listen given up before the hub sent anything loses nothing: the
/// listen after it hears the hub's next frame, however long it waits for
/// it, as the agent's sync waits for the hub's commands between its own
/// messages.
async fn a_listen_given_up_loses_no_frame(harness: &dyn HubConnectorHarness) {
    use futures::FutureExt;

    let Some((mut link, _)) = open(harness).await else {
        return;
    };
    let given_up = link.listen().now_or_never();
    assert!(given_up.is_none(), "nothing was sent yet: {given_up:?}");
    let frame = connect(link.as_ref());
    assert_eq!(link.send(&frame).await, Ok(()));
    let heard = link.listen().await;
    assert!(
        matches!(heard, Ok(HubFrame::Challenge { .. })),
        "the listen hears the Challenge: {heard:?}"
    );
}
