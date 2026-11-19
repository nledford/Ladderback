//! Steps for the untagged scenarios of `features/fleet/pairing.feature`: an
//! agent pairing with its hub, both sides run in process on fakes (§7.1;
//! §16, decision 120, "The handshake", "Pairing" and "Pending approval").

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app::fakes::{FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, SeededRandom};
use app::pairing::{AgentPairing, AgentPairingError, HubKeys, HubPairing, PairedHub};
use app::ports::{AgentRegistry, Clock, Identity, Signing};
use cucumber::{given, then, when};
use domain::fleet::{
    AgentStatus, Fingerprint, Hostname, HubAddress, PairingToken, TokenId, TokenTls,
};
use domain::{AgentId, SignedDuration, Timestamp, Uuid};

use super::Agent;

const EXPORTER: [u8; 32] = [0xee; 32];

/// One hub: its registry, its key files, its keys and its clock.
struct Hub {
    registry: Arc<InMemoryAgentRegistry>,
    clock: Arc<FakeClock>,
    pairing: HubPairing,
    keys: HubKeys,
}

impl Hub {
    async fn new(seed: u64) -> Self {
        let registry = Arc::new(InMemoryAgentRegistry::new());
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        ));
        let pairing = HubPairing::new(
            registry.clone(),
            Arc::new(InMemoryIdentity::default()),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(seed)),
            clock.clone(),
        );
        let keys = pairing.keys().await.expect("the hub's keys");
        Self {
            registry,
            clock,
            pairing,
            keys,
        }
    }

    async fn token(&self) -> PairingToken {
        let address = HubAddress::parse("https://hub.example:9443").unwrap();
        let tls = TokenTls::Pinned(Fingerprint::from_digest([7; 32]));
        self.pairing
            .make_token(&self.keys, address, tls, None)
            .await
            .expect("a token")
    }

    /// This hub started again on its registry with its key files gone,
    /// which makes new keys while no agent is registered.
    async fn without_key_files(&self) -> Self {
        let pairing = HubPairing::new(
            self.registry.clone(),
            Arc::new(InMemoryIdentity::default()),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(2)),
            self.clock.clone(),
        );
        let keys = pairing.keys().await.expect("the hub's new keys");
        assert_ne!(keys.key(), self.keys.key());
        Self {
            registry: self.registry.clone(),
            clock: self.clock.clone(),
            pairing,
            keys,
        }
    }
}

/// One machine: its key files, its agent ID and its hostname.
#[derive(Clone)]
struct Machine {
    identity: Arc<InMemoryIdentity>,
    agent_id: AgentId,
    hostname: Hostname,
    seed: u64,
}

impl Machine {
    fn new(n: u64, hostname: &str) -> Self {
        Self {
            identity: Arc::default(),
            agent_id: AgentId::from_uuid(Uuid::from_u128(u128::from(n))),
            hostname: Hostname::parse(hostname).unwrap(),
            seed: n * 1_000_000,
        }
    }

    fn pairing(&self) -> AgentPairing {
        AgentPairing::new(
            self.identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(self.seed)),
        )
    }
}

#[derive(Default)]
pub struct PairingWorld {
    hub: Option<Hub>,
    token: Option<PairingToken>,
    first: Option<Machine>,
    second: Option<Machine>,
    outcome: Option<Result<PairedHub, AgentPairingError>>,
    /// Whether the agent sent a `Pair`, the one message signed with the
    /// token's pairing key.
    sent_pair: AtomicBool,
}

impl std::fmt::Debug for PairingWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingWorld")
            .field("outcome", &self.outcome)
            .finish_non_exhaustive()
    }
}

impl PairingWorld {
    fn hub(&self) -> &Hub {
        self.hub.as_ref().expect("a hub")
    }

    async fn ensure_hub(&mut self) {
        if self.hub.is_none() {
            self.hub = Some(Hub::new(1).await);
        }
    }

    async fn new_token(&mut self) {
        self.ensure_hub().await;
        self.token = Some(self.hub().token().await);
    }

    /// `machine` pairs with the world's token; `answering` is the hub that
    /// answers at the token's address. With `lose_answer`, the hub's answer
    /// never reaches the agent.
    async fn pair(
        &self,
        machine: &Machine,
        answering: Option<&Hub>,
        lose_answer: bool,
    ) -> Option<Result<PairedHub, AgentPairingError>> {
        let hub = answering.unwrap_or_else(|| self.hub());
        let token = self.token.clone().expect("a token");
        let agent = machine.pairing();
        let (connect, handshake) = match agent
            .connect(token, machine.agent_id, machine.hostname.clone())
            .await
        {
            Ok(started) => started,
            Err(error) => return Some(Err(error)),
        };
        let (challenge, hub_side) = hub
            .pairing
            .challenge(&hub.keys, &connect, Some(EXPORTER))
            .expect("the hub's challenge");
        let (pair, awaiting) = match agent
            .answer_challenge(handshake, &challenge, Some(EXPORTER))
            .await
        {
            Ok(paired) => paired,
            Err(error) => return Some(Err(error)),
        };
        self.sent_pair.store(true, Ordering::SeqCst);
        let answer = hub
            .pairing
            .pair(&hub.keys, &hub_side, &pair)
            .await
            .expect("the hub's answer");
        if lose_answer {
            return None;
        }
        Some(agent.take_answer(awaiting, &answer).await)
    }

    async fn pair_and_keep(&mut self, machine: &Machine) {
        let outcome = self.pair(machine, None, false).await;
        self.outcome = outcome;
    }

    fn refusal_code(&self) -> String {
        match self.outcome.as_ref().expect("a pairing") {
            Err(AgentPairingError::Refused(refused)) => refused.code.as_str().to_owned(),
            other => panic!("the pairing wasn't refused: {other:?}"),
        }
    }

    async fn agents(&self) -> Vec<domain::fleet::AgentRecord> {
        self.hub().registry.agents().await.expect("the agents")
    }
}

#[given(regex = r"^(?:a hub with )?a pairing token$")]
async fn a_token(agent: &mut Agent) {
    agent.pairing.new_token().await;
}

#[given("a hub with a pairing token that has lapsed")]
async fn a_lapsed_token(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.new_token().await;
    world.hub().clock.advance(SignedDuration::from_mins(15));
}

#[given(regex = r#"^an agent paired under the hostname "([^"]+)"( in one Unicode form)?$"#)]
async fn paired_under(agent: &mut Agent, hostname: String, one_form: String) {
    let world = &mut agent.pairing;
    world.new_token().await;
    let name = if one_form.is_empty() {
        hostname
    } else {
        "caf\u{e9}".to_owned()
    };
    let machine = Machine::new(1, &name);
    world.pair_and_keep(&machine).await;
    assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
    world.first = Some(machine);
}

#[given("an agent that paired but never received the hub's answer")]
async fn paired_but_lost(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.new_token().await;
    let machine = Machine::new(1, "laptop");
    assert!(world.pair(&machine, None, true).await.is_none());
    world.first = Some(machine);
}

#[given("a hub with 20 agents waiting for approval")]
async fn twenty_waiting(agent: &mut Agent) {
    let world = &mut agent.pairing;
    for n in 1..=20 {
        world.new_token().await;
        let machine = Machine::new(n, &format!("machine-{n}"));
        world.pair_and_keep(&machine).await;
        assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
    }
}

#[when("an agent pairs with the token")]
async fn an_agent_pairs(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let machine = Machine::new(1, "laptop");
    world.pair_and_keep(&machine).await;
    world.first = Some(machine);
}

#[when("the person revokes the token")]
async fn revokes(agent: &mut Agent) {
    let world = &agent.pairing;
    let token = world.token.as_ref().expect("a token");
    let id = TokenId::of_pairing_key(&FakeSigning.public_key(token.pairing_seed()).unwrap());
    assert!(world.hub().pairing.revoke_token(id).await.unwrap());
}

#[when("the hub starts again without its key files and makes new keys")]
async fn new_keys(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.hub = Some(world.hub().without_key_files().await);
}

#[when("a client that doesn't check the hub's key pairs with the token")]
async fn unchecked_pairs(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let token = world.token.as_ref().expect("a token");
    world.token = Some(PairingToken::new(
        *world.hub().keys.key(),
        token.tls(),
        token.pairing_seed().clone(),
        token.address().clone(),
    ));
    let machine = Machine::new(1, "laptop");
    world.pair_and_keep(&machine).await;
    world.first = Some(machine);
}

#[when("it pairs again with the same token")]
async fn pairs_again(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let machine = world.first.clone().expect("the first agent");
    world.pair_and_keep(&machine).await;
}

#[when(
    regex = r#"^a second agent named "([^"]+)"( in the other Unicode form)? pairs with the token$"#
)]
async fn second_pairs(agent: &mut Agent, hostname: String, other_form: String) {
    let world = &mut agent.pairing;
    let name = if other_form.is_empty() {
        hostname
    } else {
        "cafe\u{301}".to_owned()
    };
    let machine = Machine::new(99, &name);
    world.pair_and_keep(&machine).await;
    world.second = Some(machine);
}

#[when(regex = r#"^the second agent is renamed "([^"]+)" and pairs with the same token$"#)]
async fn renamed_pairs(agent: &mut Agent, hostname: String) {
    let world = &mut agent.pairing;
    let mut machine = world.second.clone().expect("the second agent");
    machine.hostname = Hostname::parse(&hostname).unwrap();
    world.pair_and_keep(&machine).await;
}

#[when("another machine pairs with the token under the first agent's ID")]
async fn takes_over(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let first = world.first.clone().expect("the first agent");
    let machine = Machine {
        agent_id: first.agent_id,
        ..Machine::new(99, "desktop")
    };
    world.pair_and_keep(&machine).await;
}

#[when("another hub answers at the token's address and the agent pairs with the token")]
async fn another_hub_answers(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let other = Hub::new(500).await;
    let machine = Machine::new(1, "laptop");
    world.outcome = world.pair(&machine, Some(&other), false).await;
    world.first = Some(machine);
}

#[then("the hub lists the agent under its hostname as waiting for approval")]
async fn listed_pending(agent: &mut Agent) {
    let world = &agent.pairing;
    let first = world.first.as_ref().expect("the agent");
    let agents = world.agents().await;
    let [listed] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    assert_eq!(listed.hostname, first.hostname);
    assert_eq!(listed.agent_id, first.agent_id);
    assert!(matches!(listed.status, AgentStatus::Pending { .. }));
}

#[then("the token can't be used again")]
async fn token_spent(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.pair_and_keep(&Machine::new(2, "desktop")).await;
    assert_eq!(world.refusal_code(), "token-used");
}

#[then("the agent pins the hub's key, its certificate and its recovery key")]
async fn pins_the_hub(agent: &mut Agent) {
    let world = &agent.pairing;
    let first = world.first.as_ref().expect("the agent");
    let pin = first.identity.pin().await.unwrap().expect("a pin");
    let hub = world.hub();
    assert_eq!(pin.hub_key, *hub.keys.key());
    assert_eq!(pin.recovery_key, *hub.keys.recovery_key());
    assert_eq!(pin.tls, world.token.as_ref().unwrap().tls());
}

#[then(regex = r"^the pairing is refused (?:as|because) (.+)$")]
fn refused(agent: &mut Agent, reason: String) {
    let code = match reason.as_str() {
        "expired" => "token-expired",
        "revoked" => "token-revoked",
        "the hostname is taken" => "hostname-taken",
        "the ID belongs to another key" => "agent-key-mismatch",
        "the hub's key isn't the token's" => "hub-key-mismatch",
        "too many agents wait for approval" => "pending-limit",
        other => panic!("no refusal is named {other:?}"),
    };
    assert_eq!(agent.pairing.refusal_code(), code);
}

#[then("the hub lists no agent")]
async fn no_agent(agent: &mut Agent) {
    assert!(agent.pairing.agents().await.is_empty());
}

#[then("the hub lists no unused token")]
async fn no_unused_token(agent: &mut Agent) {
    let hub = agent.pairing.hub();
    let unused = hub.registry.unused_tokens(hub.clock.now()).await;
    assert_eq!(unused.expect("the unused tokens"), Vec::new());
}

#[then("the hub answers it as before and lists one agent")]
async fn answered_again(agent: &mut Agent) {
    let world = &agent.pairing;
    assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
    assert_eq!(world.agents().await.len(), 1);
}

#[then("the refusal names the agent that holds it")]
fn names_the_holder(agent: &mut Agent) {
    let world = &agent.pairing;
    let first = world.first.as_ref().expect("the holder");
    let Some(Err(AgentPairingError::Refused(refused))) = &world.outcome else {
        panic!("{:?}", world.outcome);
    };
    let holder = refused.holder.as_ref().expect("a holder");
    assert_eq!(holder.hostname, first.hostname);
}

#[then("the hub lists both agents")]
async fn both_listed(agent: &mut Agent) {
    assert_eq!(agent.pairing.agents().await.len(), 2);
}

#[then("the agent stops because the hub's key isn't the token's")]
fn stops_on_another_key(agent: &mut Agent) {
    let outcome = agent.pairing.outcome.as_ref().expect("a pairing");
    assert_eq!(outcome, &Err(AgentPairingError::HubKeyMismatch));
}

#[then("the agent sent nothing signed with the token")]
fn sent_nothing(agent: &mut Agent) {
    assert!(!agent.pairing.sent_pair.load(Ordering::SeqCst));
}

#[then("the agent pins no hub")]
async fn pins_nothing(agent: &mut Agent) {
    let first = agent.pairing.first.as_ref().expect("the agent");
    assert_eq!(first.identity.pin().await.unwrap(), None);
}

#[then("the token is still unused")]
async fn still_unused(agent: &mut Agent) {
    let world = &agent.pairing;
    let token = world.token.as_ref().expect("a token");
    let key = FakeSigning.public_key(token.pairing_seed()).unwrap();
    let hub = world.hub();
    let unused = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    assert!(unused.iter().any(|kept| kept.pairing_key == key));
}
