use std::sync::Arc;

use api::sync::{AgentFrame, HubFrame};
use async_trait::async_trait;
use domain::fleet::{
    AgentStatus, Approval, Binding, EpochSeconds, Fingerprint, HubAddress, HubEnvelope, HubRoute,
    KEY_BYTES, Nonce, PayloadDigest, Protocols, PublicKey, SeedFile, SignedMessage, SigningSeed,
    TokenTls, Welcome, WelcomeRefused, WireName, hello_kind,
};
use domain::{SignedDuration, Timestamp, Uuid};

use super::*;
use crate::fakes::{
    FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, LOOPBACK_EXPORTER,
    LoopbackHubConnector, SeededRandom,
};
use crate::pairing::wire::{
    AgentMessage, HubMessage, agent_message, hub_envelope_frame, hub_message, read_welcome,
    welcome_payload,
};
use crate::pairing::{AgentPairing, HubPairing, MessageRefused, PairWithHub, ProofAnswer};
use crate::ports::{AgentRegistry, HubLink};

/// One hub on fakes: its registry, its key files, its clock.
struct Hub {
    registry: Arc<InMemoryAgentRegistry>,
    identity: Arc<InMemoryIdentity>,
    clock: Arc<FakeClock>,
    pairing: Arc<HubPairing>,
}

fn hub(seed: u64) -> Hub {
    let registry = Arc::new(InMemoryAgentRegistry::new());
    let identity = Arc::new(InMemoryIdentity::default());
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let pairing = Arc::new(HubPairing::new(
        registry.clone(),
        identity.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(seed)),
        clock.clone(),
        "0.5.0-hub".to_owned(),
    ));
    Hub {
        registry,
        identity,
        clock,
        pairing,
    }
}

fn agent_id(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn hostname(n: u128) -> Hostname {
    Hostname::parse(&format!("laptop-{n}")).unwrap()
}

/// The key files of machine `n`, paired with `hub` through a token for the
/// hub's own TLS.
async fn paired(hub: &Hub, n: u128) -> Arc<InMemoryIdentity> {
    let keys = hub.pairing.keys().await.unwrap();
    let address = HubAddress::parse("https://hub.example:9443").unwrap();
    let tls = TokenTls::Pinned(Fingerprint::from_digest([7; 32]));
    let token = hub
        .pairing
        .make_token(&keys, address, tls, None)
        .await
        .unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let pairing = AgentPairing::new(
        identity.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(9_000 + u64::try_from(n).unwrap())),
    );
    let connector = LoopbackHubConnector::to(hub.pairing.clone());
    PairWithHub::new(pairing, Arc::new(connector))
        .pair(token, agent_id(n), hostname(n))
        .await
        .unwrap();
    identity
}

/// The key files of a machine that holds `like`'s pin and an agent key of
/// its own, and never paired.
async fn with_pin_of(like: &InMemoryIdentity) -> Arc<InMemoryIdentity> {
    let identity = Arc::new(InMemoryIdentity::default());
    identity
        .write_pin(&like.pin().await.unwrap().unwrap())
        .await
        .unwrap();
    identity
        .keep_seed(SeedFile::AgentKey, SigningSeed::from_bytes([5; KEY_BYTES]))
        .await
        .unwrap();
    identity
}

fn connecting(identity: Arc<InMemoryIdentity>, connector: Arc<dyn HubConnector>) -> ConnectToHub {
    ConnectToHub::new(
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        connector,
        "0.5.0-agent".to_owned(),
    )
}

fn agent_key(seed: &SigningSeed) -> PublicKey {
    FakeSigning.public_key(seed).unwrap()
}

/// A paired agent proves itself and is told the hub holds it pending; the
/// hub keeps the counter of this connection and when it saw the agent
/// (decision 120, "The handshake").
#[tokio::test]
async fn a_paired_agent_is_welcomed_pending_and_its_counter_kept() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let before = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.clock.advance(SignedDuration::from_secs(60));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let connected = connecting(identity.clone(), connector.clone())
        .connect(agent_id(1), hostname(1))
        .await
        .unwrap();
    let seed = identity.seed(SeedFile::AgentKey).await.unwrap().unwrap();
    assert_eq!(
        connected,
        Connected {
            approval: Approval::Pending,
            pin: identity.pin().await.unwrap().unwrap(),
            agent_key: agent_key(&seed),
        }
    );
    let after = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    assert_eq!(after.counter, before.counter + 1);
    assert_eq!(after.last_seen, hub.clock.now());
    assert_eq!(after.status, before.status);
    let kinds: Vec<_> = connector
        .sent()
        .iter()
        .map(
            |text| match agent_message(&AgentFrame::from_text(text).unwrap()) {
                Ok(AgentMessage::Connect(_)) => "connect",
                Ok(AgentMessage::Proof(_)) => "proof",
                Ok(AgentMessage::Signed(envelope)) if envelope.kind == hello_kind() => "hello",
                other => panic!("{other:?}"),
            },
        )
        .collect();
    assert_eq!(kinds, ["connect", "proof", "hello"]);
}

/// An agent a person approved is told so.
#[tokio::test]
async fn an_approved_agent_is_welcomed_approved() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    hub.registry.approve(agent_id(1), hub.clock.now());
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let connected = connecting(identity, connector)
        .connect(agent_id(1), hostname(1))
        .await
        .unwrap();
    assert_eq!(connected.approval, Approval::Approved);
    assert!(matches!(
        hub.registry
            .agent(agent_id(1))
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentStatus::Approved { .. }
    ));
}

/// A machine with no pin, or no agent key, never connects.
#[tokio::test]
async fn a_machine_never_paired_connects_to_nothing() {
    let hub = hub(1);
    let paired_one = paired(&hub, 1).await;
    let pin_only = Arc::new(InMemoryIdentity::default());
    pin_only
        .write_pin(&paired_one.pin().await.unwrap().unwrap())
        .await
        .unwrap();
    for identity in [Arc::new(InMemoryIdentity::default()), pin_only] {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let failure = connecting(identity, connector.clone())
            .connect(agent_id(2), hostname(2))
            .await;
        assert_eq!(failure, Err(ConnectFailure::NotPaired));
        assert!(connector.routes().is_empty());
    }
}

/// A hub with another key than the pinned one gets the agent's `Connect`
/// and nothing after it.
#[tokio::test]
async fn another_hub_gets_no_proof() {
    let pinned = hub(1);
    let identity = paired(&pinned, 1).await;
    let other = hub(2);
    let connector = Arc::new(LoopbackHubConnector::to(other.pairing.clone()));
    let failure = connecting(identity, connector.clone())
        .connect(agent_id(1), hostname(1))
        .await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Challenge(ChallengeRefused::HubKeyMismatch))
    );
    assert_eq!(connector.sent().len(), 1);
}

/// An agent the hub never registered, and another key under a registered
/// agent's ID, get the same signed refusal, and the hub keeps its record
/// as it was.
#[tokio::test]
async fn an_unknown_agent_and_another_key_get_one_signed_refusal() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let stranger = with_pin_of(&identity).await;
    let before = hub.registry.agents().await.unwrap();
    for id in [agent_id(9), agent_id(1)] {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let failure = connecting(stranger.clone(), connector)
            .connect(id, hostname(9))
            .await;
        let Err(ConnectFailure::Refused(refused)) = failure else {
            panic!("{failure:?}");
        };
        assert_eq!(refused.code.as_str(), "proof-refused");
        assert_eq!(refused.holder, None);
    }
    assert_eq!(hub.registry.agents().await.unwrap(), before);
}

/// What the hub signs on one connection, to forge its answers with.
struct Forge {
    seed: SigningSeed,
    hub_key: PublicKey,
    agent_id: AgentId,
    agent_nonce: Nonce,
    hub_nonce: Nonce,
}

impl Forge {
    /// `envelope` signed as the hub signs on this connection.
    fn sign(&self, mut envelope: HubEnvelope) -> HubFrame {
        let signed = SignedMessage::HubMessage {
            hub_key: self.hub_key,
            agent_id: self.agent_id,
            agent_nonce: self.agent_nonce,
            hub_nonce: self.hub_nonce,
            binding: Binding::TlsExporter(LOOPBACK_EXPORTER),
            sequence: envelope.seq,
            expires_at: envelope.expires_at,
            kind: &envelope.kind,
            payload: PayloadDigest::of(&envelope.payload),
        };
        envelope.signature = FakeSigning.sign(&self.seed, &signed.to_bytes()).unwrap();
        hub_envelope_frame(&envelope)
    }

    /// The `Welcome` in `frame`, changed by `change` and signed again.
    fn rewelcome(&self, frame: HubFrame, change: impl FnOnce(&mut Welcome)) -> HubFrame {
        let Ok(HubMessage::Signed(mut envelope)) = hub_message(&frame) else {
            panic!("{frame:?}");
        };
        let mut welcome = read_welcome(&envelope.payload).unwrap();
        change(&mut welcome);
        envelope.payload = welcome_payload(&welcome);
        self.sign(envelope)
    }
}

type Rewrite = fn(&Forge, HubFrame) -> HubFrame;

/// A connector to `hub` whose every answer after the `Challenge` is
/// rewritten, as a proxy or a hub's stolen key would.
struct Rewriting {
    hub: Arc<HubPairing>,
    hub_identity: Arc<InMemoryIdentity>,
    rewrite: Rewrite,
}

struct RewritingLink {
    inner: Box<dyn HubLink>,
    hub_identity: Arc<InMemoryIdentity>,
    rewrite: Rewrite,
    connect: Option<(AgentId, Nonce)>,
    challenge: Option<(PublicKey, Nonce)>,
}

#[async_trait]
impl HubConnector for Rewriting {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        let inner = LoopbackHubConnector::to(self.hub.clone())
            .connect(route)
            .await?;
        Ok(Box::new(RewritingLink {
            inner,
            hub_identity: self.hub_identity.clone(),
            rewrite: self.rewrite,
            connect: None,
            challenge: None,
        }))
    }
}

#[async_trait]
impl HubLink for RewritingLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.inner.exporter()
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        if let Ok(AgentMessage::Connect(connect)) = agent_message(frame) {
            self.connect = Some((connect.agent_id, connect.agent_nonce));
        }
        self.inner.send(frame).await
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        let frame = self.inner.receive().await?;
        if let Ok(HubMessage::Challenge(challenge)) = hub_message(&frame) {
            self.challenge = Some((challenge.hub_key, challenge.hub_nonce));
            return Ok(frame);
        }
        let (agent_id, agent_nonce) = self.connect.unwrap();
        let (hub_key, hub_nonce) = self.challenge.unwrap();
        let forge = Forge {
            seed: self
                .hub_identity
                .seed(SeedFile::HubKey)
                .await
                .unwrap()
                .unwrap(),
            hub_key,
            agent_id,
            agent_nonce,
            hub_nonce,
        };
        Ok((self.rewrite)(&forge, frame))
    }
}

async fn connect_rewritten(
    hub: &Hub,
    identity: Arc<InMemoryIdentity>,
    id: AgentId,
    rewrite: Rewrite,
) -> Result<Connected, ConnectFailure> {
    let connector = Rewriting {
        hub: hub.pairing.clone(),
        hub_identity: hub.identity.clone(),
        rewrite,
    };
    connecting(identity, Arc::new(connector))
        .connect(id, hostname(1))
        .await
}

/// An answer that isn't signed on this connection, or is signed out of
/// sequence, is a dropped connection; a signed message of another kind is
/// out of turn (decision 120, "Signed messages after the handshake").
#[tokio::test]
async fn an_unsigned_or_out_of_sequence_answer_is_a_dropped_connection() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let stranger = with_pin_of(&identity).await;
    let unsigned_refusal: Rewrite = |_, frame| match frame {
        HubFrame::Refused { code, holder, .. } => HubFrame::Refused {
            code,
            holder,
            signature: "A".repeat(86),
        },
        other => other,
    };
    let failure = connect_rewritten(&hub, stranger, agent_id(9), unsigned_refusal).await;
    assert_eq!(failure, Err(ConnectFailure::Unsigned));
    let altered: Rewrite = |_, frame| match frame {
        HubFrame::Signed {
            kind,
            seq,
            expires_at,
            signature,
            ..
        } => HubFrame::Signed {
            kind,
            seq,
            expires_at,
            payload: "e30".to_owned(),
            signature,
        },
        other => other,
    };
    let second: Rewrite = |forge, frame| {
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return frame;
        };
        forge.sign(HubEnvelope { seq: 2, ..envelope })
    };
    let later: Rewrite = |_, frame| match frame {
        HubFrame::Signed {
            kind,
            seq,
            expires_at,
            payload,
            signature,
        } => HubFrame::Signed {
            kind,
            seq,
            expires_at: expires_at + 1,
            payload,
            signature,
        },
        other => other,
    };
    for rewrite in [altered, second, later] {
        let failure = connect_rewritten(&hub, identity.clone(), agent_id(1), rewrite).await;
        assert_eq!(failure, Err(ConnectFailure::Unsigned));
    }
    let another_kind: Rewrite = |forge, frame| {
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return frame;
        };
        forge.sign(HubEnvelope {
            kind: WireName::parse("Removed").unwrap(),
            ..envelope
        })
    };
    let failure = connect_rewritten(&hub, identity, agent_id(1), another_kind).await;
    assert_eq!(failure, Err(ConnectFailure::OutOfTurn));
}

/// A signed `Welcome` the agent can't act on: past its window, with the
/// agent's protocols changed, or of a generation below the pinned one. A
/// higher generation raises the pin's.
#[tokio::test]
async fn a_welcome_is_checked_before_it_is_acted_on() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let past_the_window: Rewrite = |forge, frame| {
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return frame;
        };
        let welcome = read_welcome(&envelope.payload).unwrap();
        forge.sign(HubEnvelope {
            expires_at: EpochSeconds::from_seconds(welcome.signed_at.seconds() + 901),
            ..envelope
        })
    };
    let failure = connect_rewritten(&hub, identity.clone(), agent_id(1), past_the_window).await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Welcome(WelcomeRefused::Expiry))
    );
    let stripped: Rewrite = |forge, frame| {
        forge.rewelcome(frame, |welcome| {
            welcome.agent_protocols = Protocols::new(1, 2).unwrap();
        })
    };
    let failure = connect_rewritten(&hub, identity.clone(), agent_id(1), stripped).await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Welcome(WelcomeRefused::Protocols))
    );
    let mut pin = identity.pin().await.unwrap().unwrap();
    pin.generation = 2;
    identity.write_pin(&pin).await.unwrap();
    let failure = connecting(
        identity.clone(),
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
    )
    .connect(agent_id(1), hostname(1))
    .await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Welcome(WelcomeRefused::GenerationLower))
    );
    let raised: Rewrite = |forge, frame| forge.rewelcome(frame, |welcome| welcome.generation = 3);
    let connected = connect_rewritten(&hub, identity.clone(), agent_id(1), raised)
        .await
        .unwrap();
    assert_eq!(connected.pin.generation, 3);
    assert_eq!(identity.pin().await.unwrap().unwrap().generation, 3);
}

/// A connector to `hub` whose link lets `delay` pass on the agent's clock
/// before it hands over a signed message, as a link that held the
/// `Welcome` in a buffer would.
struct Delaying {
    hub: Arc<HubPairing>,
    clock: Arc<FakeClock>,
    delay: SignedDuration,
}

struct DelayingLink {
    inner: Box<dyn HubLink>,
    clock: Arc<FakeClock>,
    delay: SignedDuration,
}

#[async_trait]
impl HubConnector for Delaying {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        let inner = LoopbackHubConnector::to(self.hub.clone())
            .connect(route)
            .await?;
        Ok(Box::new(DelayingLink {
            inner,
            clock: self.clock.clone(),
            delay: self.delay,
        }))
    }
}

#[async_trait]
impl HubLink for DelayingLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.inner.exporter()
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        self.inner.send(frame).await
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        let frame = self.inner.receive().await?;
        if let Ok(HubMessage::Signed(_)) = hub_message(&frame) {
            self.clock.advance(self.delay);
        }
        Ok(frame)
    }
}

/// A `Welcome` that reaches the agent as long after its `Hello` as the
/// message lives had expired by then, so the agent acts on none of it,
/// whatever its own wall clock says; one a little sooner is taken
/// (decision 120, "Expiry is judged on the hub's time").
#[tokio::test]
async fn a_welcome_that_arrives_after_its_life_is_not_acted_on() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let life = SignedDuration::from_secs(i64::try_from(domain::fleet::MESSAGE_LIFE).unwrap());
    let connect = |delay: SignedDuration| {
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        ));
        ConnectToHub::new(
            identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(77)),
            clock.clone(),
            Arc::new(Delaying {
                hub: hub.pairing.clone(),
                clock,
                delay,
            }),
            "0.5.0-agent".to_owned(),
        )
    };
    let late = connect(life).connect(agent_id(1), hostname(1)).await;
    assert_eq!(late, Err(ConnectFailure::Welcome(WelcomeRefused::Expiry)));
    let in_time = connect(life - SignedDuration::from_secs(1))
        .connect(agent_id(1), hostname(1))
        .await;
    assert!(in_time.is_ok(), "{in_time:?}");
}

/// The hub's side on its own: a proof is checked against the key the hub
/// registered, and a `Hello` must be the agent's, first in sequence, and
/// of its kind; the `Welcome` it gets is signed and lives 10 minutes.
#[tokio::test]
async fn the_hub_welcomes_only_a_proved_agents_first_hello() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let keys = hub.pairing.keys().await.unwrap();
    let agent_seed = identity.seed(SeedFile::AgentKey).await.unwrap().unwrap();
    let agent_nonce = Nonce::from_bytes([3; KEY_BYTES]);
    let connect = domain::fleet::Connect {
        agent_id: agent_id(1),
        agent_nonce,
        binding_kind: domain::fleet::BindingKind::TlsExporter,
    };
    let (challenge, handshake) = hub
        .pairing
        .challenge(&keys, &connect, Some(LOOPBACK_EXPORTER))
        .unwrap();
    let binding = Binding::TlsExporter(LOOPBACK_EXPORTER);
    let proof_bytes = SignedMessage::AgentProof {
        hub_key: *keys.key(),
        agent_id: agent_id(1),
        agent_key: agent_key(&agent_seed),
        agent_nonce,
        hub_nonce: challenge.hub_nonce,
        binding,
        counter: 40,
    }
    .to_bytes();
    let wrong = domain::fleet::Proof {
        counter: 40,
        agent_proof: FakeSigning
            .sign(&SigningSeed::from_bytes([5; KEY_BYTES]), &proof_bytes)
            .unwrap(),
    };
    let refused = hub.pairing.prove(&keys, &handshake, &wrong).await.unwrap();
    assert!(matches!(refused, ProofAnswer::Refused(_)), "{refused:?}");
    let proof = domain::fleet::Proof {
        counter: 40,
        agent_proof: FakeSigning.sign(&agent_seed, &proof_bytes).unwrap(),
    };
    let ProofAnswer::Proved(agent) = hub.pairing.prove(&keys, &handshake, &proof).await.unwrap()
    else {
        panic!("the proof was refused");
    };
    assert_eq!(agent.counter, 40);
    let hello = |seq: u64, kind: WireName, seed: &SigningSeed| {
        let payload = crate::pairing::wire::hello_payload(&domain::fleet::Hello {
            agent_version: "0.5.0-agent".to_owned(),
            hostname: hostname(1),
            protocols: domain::fleet::PROTOCOLS,
            clock: EpochSeconds::from_seconds(1_800_000_000),
        });
        let signed = SignedMessage::AgentMessage {
            hub_key: *keys.key(),
            agent_id: agent_id(1),
            agent_nonce,
            hub_nonce: challenge.hub_nonce,
            binding,
            sequence: seq,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        domain::fleet::AgentEnvelope {
            signature: FakeSigning.sign(seed, &signed.to_bytes()).unwrap(),
            kind,
            seq,
            payload,
        }
    };
    let other_seed = SigningSeed::from_bytes([5; KEY_BYTES]);
    for (sent, refusal) in [
        (
            hello(1, hello_kind(), &other_seed),
            MessageRefused::Unsigned,
        ),
        (
            hello(2, hello_kind(), &agent_seed),
            MessageRefused::OutOfSequence,
        ),
        (
            hello(1, WireName::parse("Goodbye").unwrap(), &agent_seed),
            MessageRefused::Unexpected,
        ),
    ] {
        assert_eq!(
            hub.pairing
                .welcome(&keys, &handshake, &agent, &sent)
                .unwrap(),
            Err(refusal)
        );
    }
    let welcome = hub
        .pairing
        .welcome(
            &keys,
            &handshake,
            &agent,
            &hello(1, hello_kind(), &agent_seed),
        )
        .unwrap()
        .unwrap();
    assert_eq!(welcome.seq, 1);
    assert_eq!(welcome.kind, domain::fleet::welcome_kind());
    assert_eq!(
        welcome.expires_at,
        EpochSeconds::from_seconds(1_800_000_600)
    );
    let signed = SignedMessage::HubMessage {
        hub_key: *keys.key(),
        agent_id: agent_id(1),
        agent_nonce,
        hub_nonce: challenge.hub_nonce,
        binding,
        sequence: 1,
        expires_at: welcome.expires_at,
        kind: &welcome.kind,
        payload: PayloadDigest::of(&welcome.payload),
    };
    assert!(FakeSigning.verify(keys.key(), &signed.to_bytes(), &welcome.signature));
    let read = read_welcome(&welcome.payload).unwrap();
    assert_eq!(read.approval, Approval::Pending);
    assert_eq!(read.hub_version, "0.5.0-hub");
    assert_eq!(read.protocol, Some(1));
}
