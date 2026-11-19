use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use api::sync::{AgentFrame, HubFrame};
use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domain::fleet::{
    AgentStatus, HubAddress, HubRoute, KEY_BYTES, PairingToken, RouteRefused, RouteSession,
    TokenTls,
};

use super::*;
use crate::fakes::InMemoryIdentity;
use crate::pairing::session::{BINDING_UNAVAILABLE, HubStep};
use crate::pairing::tests::{EXPORTER, Hub, agent, agent_id, laptop, pinned};
use crate::ports::{AgentRegistry, HubLink, Identity};

/// What the agent sent, and the routes it connected to.
#[derive(Default)]
struct Wiretap {
    routes: Mutex<Vec<HubRoute>>,
    sent: Mutex<Vec<String>>,
}

/// A connector to a hub in this process: each frame goes through its text
/// to the hub's session and its answer back.
struct Loopback {
    hub: Arc<Hub>,
    exporter: Option<[u8; KEY_BYTES]>,
    tap: Arc<Wiretap>,
}

struct LoopLink {
    hub: Arc<Hub>,
    exporter: Option<[u8; KEY_BYTES]>,
    tap: Arc<Wiretap>,
    session: Option<crate::pairing::HubSession>,
    inbox: VecDeque<String>,
    closed: Option<&'static str>,
}

#[async_trait]
impl HubConnector for Loopback {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        self.tap.routes.lock().unwrap().push(route.clone());
        Ok(Box::new(LoopLink {
            hub: self.hub.clone(),
            exporter: self.exporter,
            tap: self.tap.clone(),
            session: None,
            inbox: VecDeque::new(),
            closed: None,
        }))
    }
}

#[async_trait]
impl HubLink for LoopLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.exporter
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        let text = frame.to_text();
        self.tap.sent.lock().unwrap().push(text.clone());
        if self.session.is_none() {
            self.session = Some(self.hub.pairing.session(self.exporter).await.unwrap());
        }
        let session = self.session.as_mut().unwrap();
        let read = AgentFrame::from_text(&text).unwrap();
        match self.hub.pairing.step(session, &read).await {
            HubStep::Reply(frame) | HubStep::Last(frame) => self.inbox.push_back(frame.to_text()),
            HubStep::Close(reason) => self.closed = Some(reason),
            HubStep::Fail(error) => panic!("the hub failed: {error:?}"),
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        match self.inbox.pop_front() {
            Some(text) => HubFrame::from_text(&text).map_err(|_| LinkError::Unreadable),
            None => Err(LinkError::Closed {
                reason: self.closed.map(str::to_owned),
            }),
        }
    }
}

struct Setup {
    hub: Arc<Hub>,
    identity: Arc<InMemoryIdentity>,
    tap: Arc<Wiretap>,
    client: PairWithHub,
}

fn setup(exporter: Option<[u8; KEY_BYTES]>) -> Setup {
    let hub = Arc::new(Hub::new(1));
    let identity = Arc::new(InMemoryIdentity::default());
    let tap = Arc::new(Wiretap::default());
    let connector = Loopback {
        hub: hub.clone(),
        exporter,
        tap: tap.clone(),
    };
    let client = PairWithHub::new(agent(identity.clone()), Arc::new(connector));
    Setup {
        hub,
        identity,
        tap,
        client,
    }
}

fn at(token: PairingToken, address: &str) -> PairingToken {
    token.with_address(HubAddress::parse(address).unwrap())
}

/// A CA-checked token pairs over a session the hub doesn't bind to, and a
/// pinned one over the hub's own TLS: the agent pins the hub and the hub
/// keeps it pending (decision 120, "Pairing").
#[tokio::test]
async fn a_token_pairs_through_the_frames_and_pins_the_hub() {
    for (tls, exporter, session) in [
        (TokenTls::Ca, None, RouteSession::Ca),
        (
            pinned(),
            Some(EXPORTER),
            RouteSession::Pinned(match pinned() {
                TokenTls::Pinned(fingerprint) => fingerprint,
                TokenTls::Ca => panic!("a pinned token"),
            }),
        ),
    ] {
        let setup = setup(exporter);
        let token = setup.hub.token(tls).await;
        let paired = setup
            .client
            .pair(token, agent_id(), laptop())
            .await
            .unwrap();
        assert_eq!(
            setup.identity.pin().await.unwrap(),
            Some(paired.pin.clone())
        );
        let routes = setup.tap.routes.lock().unwrap().clone();
        assert_eq!(
            routes.iter().map(|route| route.session).collect::<Vec<_>>(),
            [session]
        );
        let agents = setup.hub.registry.agents().await.unwrap();
        assert_eq!(agents.len(), 1);
        assert!(matches!(agents[0].status, AgentStatus::Pending { .. }));
    }
}

/// A plain address off this computer would show the token to the network:
/// nothing connects and nothing is sent.
#[tokio::test]
async fn a_plain_address_off_this_computer_is_never_connected_to() {
    let setup = setup(None);
    let token = at(setup.hub.token(TokenTls::Ca).await, "http://192.0.2.7:9898");
    let failure = setup.client.pair(token, agent_id(), laptop()).await;
    assert_eq!(
        failure,
        Err(PairFailure::Route(RouteRefused::PlainOffLoopback))
    );
    assert!(setup.tap.routes.lock().unwrap().is_empty());
    let loopback = at(setup.hub.token(TokenTls::Ca).await, "http://127.0.0.1:9898");
    setup
        .client
        .pair(loopback, agent_id(), laptop())
        .await
        .unwrap();
    let routes = setup.tap.routes.lock().unwrap().clone();
    assert_eq!(routes[0].session, RouteSession::Plain);
}

/// A pinned token on a session without the hub's own TLS: the hub closes
/// with `binding-unavailable` after the `Connect`, which carries nothing
/// of the token.
#[tokio::test]
async fn a_hub_without_the_binding_closes_before_anything_of_the_token() {
    let setup = setup(None);
    let token = setup.hub.token(pinned()).await;
    let seed = URL_SAFE_NO_PAD.encode(token.pairing_seed().expose());
    let failure = setup.client.pair(token, agent_id(), laptop()).await;
    assert_eq!(
        failure,
        Err(PairFailure::Link(LinkError::Closed {
            reason: Some(BINDING_UNAVAILABLE.to_owned()),
        }))
    );
    let sent = setup.tap.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("\"connect\""), "{sent:?}");
    assert!(!sent[0].contains(&seed));
    assert_eq!(setup.identity.pin().await.unwrap(), None);
}

/// A hub that answers out of turn, or with a frame that doesn't read, is a
/// dropped connection: nothing is pinned.
#[tokio::test]
async fn a_hub_answering_out_of_turn_is_a_dropped_connection() {
    struct Scripted(Mutex<Option<HubFrame>>);
    #[async_trait]
    impl HubConnector for Scripted {
        async fn connect(&self, _: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
            Ok(Box::new(ScriptedLink(self.0.lock().unwrap().take())))
        }
    }
    struct ScriptedLink(Option<HubFrame>);
    #[async_trait]
    impl HubLink for ScriptedLink {
        fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
            None
        }
        async fn send(&mut self, _: &AgentFrame) -> Result<(), LinkError> {
            Ok(())
        }
        async fn receive(&mut self) -> Result<HubFrame, LinkError> {
            self.0.take().ok_or(LinkError::Closed { reason: None })
        }
    }
    let hub = Hub::new(1);
    let out_of_turn = HubFrame::Paired {
        recovery_key: "A".repeat(43),
        signature: "A".repeat(86),
    };
    let unreadable = HubFrame::Challenge {
        hub_nonce: "short".to_owned(),
        hub_key: String::new(),
        hub_handshakes: 1,
        hub_proof: String::new(),
    };
    for frame in [out_of_turn, unreadable] {
        let identity = Arc::new(InMemoryIdentity::default());
        let client = PairWithHub::new(
            agent(identity.clone()),
            Arc::new(Scripted(Mutex::new(Some(frame)))),
        );
        let token = hub.token(TokenTls::Ca).await;
        let failure = client.pair(token, agent_id(), laptop()).await;
        assert_eq!(failure, Err(PairFailure::OutOfTurn));
        assert_eq!(identity.pin().await.unwrap(), None);
    }
}

/// A machine pinned to another hub stops before it connects.
#[tokio::test]
async fn a_machine_pinned_to_another_hub_never_connects() {
    let setup = setup(None);
    let other = Hub::new(2);
    let token = setup.hub.token(TokenTls::Ca).await;
    setup
        .client
        .pair(token, agent_id(), laptop())
        .await
        .unwrap();
    let routes_before = setup.tap.routes.lock().unwrap().len();
    let failure = setup
        .client
        .pair(other.token(TokenTls::Ca).await, agent_id(), laptop())
        .await;
    assert!(
        matches!(
            failure,
            Err(PairFailure::Pairing(
                AgentPairingError::PinnedToAnotherHub { .. }
            ))
        ),
        "{failure:?}"
    );
    assert_eq!(setup.tap.routes.lock().unwrap().len(), routes_before);
}
