//! Steps for the untagged scenarios of `features/fleet/pairing.feature` and
//! `features/fleet/connecting.feature`: an agent pairing with its hub, then
//! connecting to it, both sides run in process on fakes (§7.1, §7.2; §16,
//! decision 120, "The handshake", "Pairing" and "Pending approval"). The
//! hub lists its agents as its page does, so a waiting agent that lapsed
//! is gone from them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use api::sync::AgentFrame;
use app::fakes::{
    FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, InMemoryOwnershipRegistry,
    InMemoryReportInbox, LoopbackHubConnector, SeededRandom,
};
use app::ownership::RepositoryOwners;
use app::pairing::{
    AgentPairing, AgentPairingError, ChallengeRefused, ConnectFailure, ConnectToHub, Connected,
    HubKeys, HubPairing, PairFailure, PairWithHub, PairedHub,
};
use app::ports::{AgentRegistry, Clock, Identity, LinkError, Signing};
use cucumber::{given, then, when};
use domain::fleet::{
    AgentStatus, Approval, Fingerprint, Hostname, HubAddress, ListedAgent, MachineIdentifier,
    MachineZone, PairingToken, RouteRefused, SeedFile, SigningSeed, TokenId, TokenTls,
};
use domain::{AgentId, Offset, SignedDuration, TimeZone, TimeZoneDatabase, Timestamp, Uuid};

use super::Agent;

#[path = "pairing_dialog.rs"]
mod dialog;
#[path = "held_prunes.rs"]
mod held_prunes;
#[path = "hub_plans.rs"]
mod hub_plans;
#[path = "leaving.rs"]
mod leaving;
#[path = "offline_alerts.rs"]
mod offline_alerts;
#[path = "removed_new_id.rs"]
mod removed_new_id;
#[path = "removing.rs"]
mod removing;
#[path = "second_machine.rs"]
mod second_machine;
#[path = "staying_connected.rs"]
mod staying_connected;
#[path = "sync_replay.rs"]
mod sync_replay;

const EXPORTER: [u8; 32] = [0xee; 32];

/// One hub: its registry, its key files, its keys and its clock.
struct Hub {
    registry: Arc<InMemoryAgentRegistry>,
    identity: Arc<InMemoryIdentity>,
    clock: Arc<FakeClock>,
    pairing: Arc<HubPairing>,
    keys: HubKeys,
    /// The reports it took of its agents.
    inbox: Arc<InMemoryReportInbox>,
    /// Where it tells its browsers of its machines.
    events: Arc<app::event_bus::BroadcastEventBus>,
    /// What a browser of the hub heard and the steps haven't read.
    heard: Arc<std::sync::Mutex<futures::stream::BoxStream<'static, api::UiEvent>>>,
}

impl Hub {
    async fn new(seed: u64) -> Self {
        Self::with_access(seed, None).await
    }

    /// A hub whose sign-in by the admin password is on when `access` is
    /// given. It decides which machine maintains each repository its
    /// machines report, as a real hub does.
    async fn with_access(seed: u64, access: Option<Arc<app::access::Access>>) -> Self {
        let registry = Arc::new(InMemoryAgentRegistry::new());
        let identity = Arc::new(InMemoryIdentity::default());
        let inbox = Arc::new(InMemoryReportInbox::default());
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        ));
        let pairing = HubPairing::new(
            registry.clone(),
            identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(seed)),
            clock.clone(),
            "0.5.0-hub".to_owned(),
        )
        .with_inbox(inbox.clone())
        .with_owners(Arc::new(RepositoryOwners::new(
            registry.clone(),
            Arc::new(InMemoryOwnershipRegistry::new()),
            clock.clone(),
            Arc::new(app::fakes::InMemoryDriveFlush::default()),
        )));
        let events = Arc::new(app::event_bus::BroadcastEventBus::new());
        let heard = Arc::new(std::sync::Mutex::new(
            app::ports::EventBus::subscribe(&*events).await,
        ));
        let pairing = pairing.with_events(events.clone());
        let pairing = Arc::new(match access {
            Some(access) => pairing.with_access(access),
            None => pairing,
        });
        let keys = pairing.keys().await.expect("the hub's keys");
        Self {
            registry,
            identity,
            clock,
            pairing,
            keys,
            inbox,
            events,
            heard,
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
        let restarted = self
            .restarted_on(Arc::new(InMemoryIdentity::default()))
            .await;
        assert_ne!(restarted.keys.key(), self.keys.key());
        restarted
    }

    /// This hub's daemon started again: on its registry, its key files and
    /// its reports, with nothing of what it held in memory.
    async fn restarted(&self) -> Self {
        self.restarted_on(self.identity.clone()).await
    }

    async fn restarted_on(&self, identity: Arc<InMemoryIdentity>) -> Self {
        let pairing = Arc::new(
            HubPairing::new(
                self.registry.clone(),
                identity.clone(),
                Arc::new(FakeSigning),
                Arc::new(SeededRandom::new(2)),
                self.clock.clone(),
                "0.5.0-hub".to_owned(),
            )
            .with_inbox(self.inbox.clone())
            .with_events(self.events.clone()),
        );
        let keys = pairing.keys().await.expect("the hub's keys");
        Self {
            registry: self.registry.clone(),
            identity,
            clock: self.clock.clone(),
            pairing,
            keys,
            inbox: self.inbox.clone(),
            events: self.events.clone(),
            heard: self.heard.clone(),
        }
    }
}

/// One machine: its key files, its agent ID, its hostname, the machine
/// zone it reports, the identifier of the computer it runs on, and the
/// leases and epoch floors it keeps.
#[derive(Clone)]
struct Machine {
    identity: Arc<InMemoryIdentity>,
    agent_id: AgentId,
    hostname: Hostname,
    zone: Option<MachineZone>,
    seed: u64,
    identifier: Option<MachineIdentifier>,
    leases: Arc<app::fakes::InMemoryLeaseCache>,
}

impl Machine {
    fn new(n: u64, hostname: &str) -> Self {
        Self {
            identity: Arc::default(),
            agent_id: AgentId::from_uuid(Uuid::from_u128(u128::from(n))),
            hostname: Hostname::parse(hostname).unwrap(),
            zone: None,
            seed: n * 1_000_000,
            identifier: MachineIdentifier::parse(&format!("{n:032x}")).ok(),
            leases: Arc::default(),
        }
    }

    fn pairing(&self) -> AgentPairing {
        AgentPairing::new(
            self.identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(self.seed)),
            self.leases.clone(),
        )
    }

    /// This machine's connection to its hub through `connector`.
    fn connecting(&self, connector: Arc<LoopbackHubConnector>) -> ConnectToHub {
        ConnectToHub::new(
            self.identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(self.seed + 1)),
            Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).unwrap(),
            )),
            connector,
            "0.5.0-agent".to_owned(),
            self.identifier.clone(),
        )
    }

    /// This machine holding `like`'s pin and an agent key of its own, as a
    /// copy of the pin alone would leave it: it never paired.
    async fn with_pin_of(self, like: &Self) -> Self {
        let pin = like.identity.pin().await.unwrap().expect("a pin");
        self.identity.write_pin(&pin).await.unwrap();
        self.identity
            .keep_seed(SeedFile::AgentKey, SigningSeed::from_bytes([0x5a; 32]))
            .await
            .unwrap();
        self
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
    /// The connector the last pairing over a connection went through.
    connector: Option<Arc<LoopbackHubConnector>>,
    /// Why the last pairing over a connection stopped short of the
    /// handshake, when it did.
    failure: Option<PairFailure>,
    /// How each connection to the hub ended, in order.
    connections: Vec<Result<Connected, ConnectFailure>>,
    /// The hub's counter for the first agent after each connection.
    counters: Vec<u64>,
    /// What the hub answered the last token asked for on its page.
    asked: Option<Result<app::pairing::MadeToken, app::pairing::TokenAskRefused>>,
    /// What the hub answered the last approval asked for on its page.
    approved: Option<
        Result<
            Result<domain::fleet::AgentRecord, domain::fleet::ApprovalRefused>,
            app::pairing::TokenAskRefused,
        >,
    >,
    /// What the hub answered the last removal asked for on its page.
    removed: Option<removing::Removal>,
    /// The one host and port the hub answers at, once it answers at one.
    only_at: Option<(&'static str, u16)>,
    /// The sync of the agents' reports (`fleet/sync_replay.feature`).
    syncing: sync_replay::SyncWorld,
    /// The sync of the first agent's plans and progress
    /// (`fleet/plans_and_progress.feature`).
    live: hub_plans::LiveWorld,
    /// Two machines' prunes and backups through their syncs
    /// (`maintenance/prune_guard.feature`).
    held: held_prunes::HeldWorld,
    /// The first agent's machine ID before it took a new one
    /// (`fleet/removed_new_id.feature`).
    old_id: Option<AgentId>,
    /// The hub's notification rules for its offline alerts
    /// (`fleet/offline_alerts.feature`).
    alerts: Option<offline_alerts::Alerting>,
    /// The hub the first agent paired with after its own
    /// (`fleet/leaving_a_hub.feature`).
    other_hub: Option<Hub>,
    /// How the first agent's last leaving of its hub ended.
    left: Option<Result<(), AgentPairingError>>,
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
            .connect(
                token,
                machine.agent_id,
                machine.hostname.clone(),
                machine.zone.clone(),
            )
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
        let listed = self.hub().pairing.agents().await.expect("the agents");
        listed.into_iter().map(|listed| listed.record).collect()
    }

    /// The second agent as the hub lists it.
    async fn second_listed(&self) -> ListedAgent {
        let second = self.second.as_ref().expect("the second agent");
        let hub = self.hub();
        let agents = hub.pairing.agents().await.expect("the agents");
        agents
            .into_iter()
            .find(|listed| listed.record.agent_id == second.agent_id)
            .expect("the second agent listed")
    }

    /// The world's token as a person pastes it: between quotes, with
    /// spaces and a line break around it.
    fn pasted_token(&self) -> PairingToken {
        let token = self.token.as_ref().expect("a token");
        let pasted = format!("  \"{}\"\n", token.to_text().expose());
        PairingToken::parse_pasted(&pasted).expect("the pasted token reads")
    }

    /// A first agent pairs with `token` over a connection from `connector`,
    /// as `ladderback pair` does.
    async fn pair_over(&mut self, token: PairingToken, connector: LoopbackHubConnector) {
        self.pair_machine_over(Machine::new(1, "laptop"), token, connector)
            .await;
    }

    /// `machine`, the first agent, pairs with `token` over a connection
    /// from `connector`, as `ladderback pair` does.
    async fn pair_machine_over(
        &mut self,
        machine: Machine,
        token: PairingToken,
        connector: LoopbackHubConnector,
    ) {
        let connector = Arc::new(connector);
        let outcome = PairWithHub::new(machine.pairing(), connector.clone())
            .pair(
                token,
                machine.agent_id,
                machine.hostname.clone(),
                machine.zone.clone(),
            )
            .await;
        (self.outcome, self.failure) = match outcome {
            Ok(paired) => (Some(Ok(paired)), None),
            Err(PairFailure::Pairing(error)) => (Some(Err(error)), None),
            Err(failure) => (None, Some(failure)),
        };
        self.connector = Some(connector);
        self.first = Some(machine);
    }

    fn hub_link(&self) -> LoopbackHubConnector {
        LoopbackHubConnector::to(self.hub().pairing.clone())
    }

    fn first(&self) -> &Machine {
        self.first.as_ref().expect("the first agent")
    }

    /// The key files a new machine ID is taken over: the copy's, or the
    /// first agent's; `None` when the scenario paired no machine.
    pub fn new_id_identity(&self, copy: bool) -> Option<Arc<InMemoryIdentity>> {
        let machine = if copy { &self.second } else { &self.first };
        machine
            .as_ref()
            .map(|machine| Arc::clone(&machine.identity))
    }

    /// The connector to `hub`, which answers only where the world says.
    fn connector_to(&self, hub: &Hub) -> Arc<LoopbackHubConnector> {
        let connector = LoopbackHubConnector::to(hub.pairing.clone()).watching(hub.clock.clone());
        Arc::new(match self.only_at {
            Some((host, port)) => connector.only_at(host, port),
            None => connector,
        })
    }

    /// `machine` connects to its hub; `answering` is the hub that answers
    /// at the pinned address, the world's by default.
    async fn connect(&mut self, machine: &Machine, answering: Option<&Hub>) {
        let hub = answering.unwrap_or_else(|| self.hub());
        let connector = self.connector_to(hub);
        let outcome = machine
            .connecting(connector.clone())
            .connect(
                machine.agent_id,
                machine.hostname.clone(),
                machine.zone.clone(),
            )
            .await;
        self.record(outcome, connector).await;
    }

    /// The person gives `machine` the address `address` for its hub;
    /// `answering` is the hub that answers there, the world's by default.
    async fn give_address(&mut self, machine: &Machine, address: &str, answering: Option<&Hub>) {
        let hub = answering.unwrap_or_else(|| self.hub());
        let connector = self.connector_to(hub);
        let outcome = machine
            .connecting(connector.clone())
            .connect_at(
                machine.agent_id,
                machine.hostname.clone(),
                machine.zone.clone(),
                HubAddress::parse(address).unwrap(),
            )
            .await;
        self.record(outcome, connector).await;
    }

    async fn record(
        &mut self,
        outcome: Result<Connected, ConnectFailure>,
        connector: Arc<LoopbackHubConnector>,
    ) {
        let first = self.first().agent_id;
        let record = self.hub().registry.agent(first).await.unwrap();
        self.counters
            .push(record.map_or(0, |record| record.counter));
        self.connections.push(outcome);
        self.connector = Some(connector);
    }

    /// The addresses the first agent's pin says to try, in order.
    async fn addresses(&self) -> Vec<String> {
        let pin = self.first().identity.pin().await.unwrap().expect("a pin");
        pin.addresses()
            .map(|address| address.as_str().to_owned())
            .collect()
    }

    /// The host and port of each route the last connection tried.
    fn tried(&self) -> Vec<String> {
        let connector = self.connector.as_ref().expect("a connector");
        connector
            .routes()
            .into_iter()
            .map(|route| format!("{}:{}", route.host, route.port))
            .collect()
    }

    fn connected(&self) -> &Connected {
        match self.connections.last() {
            Some(Ok(connected)) => connected,
            other => panic!("the agent didn't connect: {other:?}"),
        }
    }

    fn connect_failure(&self) -> &ConnectFailure {
        match self.connections.last() {
            Some(Err(failure)) => failure,
            other => panic!("the connection didn't fail: {other:?}"),
        }
    }

    /// The counters of the proofs the agent sent on the last connection.
    fn proved_counters(&self) -> Vec<u64> {
        let connector = self.connector.as_ref().expect("a connector");
        connector
            .sent()
            .iter()
            .filter_map(|text| match AgentFrame::from_text(text) {
                Ok(AgentFrame::Proof { counter, .. }) => Some(counter),
                _ => None,
            })
            .collect()
    }

    /// A first machine reporting `zone` pairs with the world's token over
    /// its connection, and the pairing goes through.
    async fn pair_in_zone(&mut self, zone: Option<MachineZone>) {
        let machine = Machine {
            zone,
            ..Machine::new(1, "laptop")
        };
        let token = self.token.clone().expect("a token");
        let connector = self.hub_link();
        self.pair_machine_over(machine, token, connector).await;
        assert!(matches!(self.outcome, Some(Ok(_))), "{:?}", self.outcome);
    }

    /// The first machine, now reporting `zone`, connects to its hub.
    async fn connect_in_zone(&mut self, zone: Option<MachineZone>) {
        let machine = Machine {
            zone,
            ..self.first().clone()
        };
        self.connect(&machine, None).await;
        self.connected();
        self.first = Some(machine);
    }

    /// The machine zone the hub holds for the first machine.
    async fn held_zone(&self) -> Option<MachineZone> {
        let first = self.first().agent_id;
        let record = self.hub().registry.agent(first).await.unwrap();
        record.expect("the hub holds the machine").zone
    }
}

/// The machine zone a machine whose system zone is `name` reports, as
/// `ladderback` makes it from the system zone.
fn reported_from(name: &str) -> Option<MachineZone> {
    let zone = TimeZoneDatabase::bundled()
        .get(name)
        .expect("a zone the bundled database knows");
    MachineZone::of(&zone)
}

#[when(expr = "a machine in the time zone {string} pairs with the token")]
async fn machine_in_zone_pairs(agent: &mut Agent, zone: String) {
    agent.pairing.pair_in_zone(reported_from(&zone)).await;
}

#[when(
    expr = "a machine in a time zone named {string} that the hub's zone database doesn't know pairs with the token"
)]
async fn machine_in_newer_zone_pairs(agent: &mut Agent, zone: String) {
    assert!(TimeZoneDatabase::bundled().get(&zone).is_err(), "{zone}");
    let reported = MachineZone::parse(&zone).expect("an IANA-shaped name");
    agent.pairing.pair_in_zone(Some(reported)).await;
}

#[given(expr = "a machine paired with its hub from the time zone {string}")]
async fn machine_paired_in_zone(agent: &mut Agent, zone: String) {
    let world = &mut agent.pairing;
    world.new_token().await;
    world.pair_in_zone(reported_from(&zone)).await;
}

#[when(expr = "the machine moves to the time zone {string} and connects to its hub")]
async fn machine_moves(agent: &mut Agent, zone: String) {
    agent.pairing.connect_in_zone(reported_from(&zone)).await;
}

#[when(
    expr = "the machine moves to a time zone named {string} that the hub's zone database doesn't know and connects to its hub"
)]
async fn machine_moves_to_newer_zone(agent: &mut Agent, zone: String) {
    assert!(TimeZoneDatabase::bundled().get(&zone).is_err(), "{zone}");
    let reported = MachineZone::parse(&zone).expect("an IANA-shaped name");
    agent.pairing.connect_in_zone(Some(reported)).await;
}

#[when(
    expr = "the machine's zone becomes a fixed offset of {int} hours and it connects to its hub"
)]
async fn machine_on_fixed_offset(agent: &mut Agent, hours: i8) {
    let fixed = TimeZone::fixed(Offset::constant(hours));
    agent.pairing.connect_in_zone(MachineZone::of(&fixed)).await;
}

#[then(expr = "the hub holds the machine's time zone as {string}")]
async fn hub_holds_zone(agent: &mut Agent, zone: String) {
    let held = agent.pairing.held_zone().await;
    assert_eq!(held.as_ref().map(MachineZone::as_str), Some(zone.as_str()));
}

#[then("the hub holds the machine's time zone as unknown")]
async fn hub_holds_unknown_zone(agent: &mut Agent) {
    assert_eq!(agent.pairing.held_zone().await, None);
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

#[given(expr = "a hub with a pairing token for the address {string}")]
async fn a_token_for(agent: &mut Agent, address: String) {
    let world = &mut agent.pairing;
    world.ensure_hub().await;
    let hub = world.hub();
    let address = HubAddress::parse(&address).expect("a hub address");
    let token = hub
        .pairing
        .make_token(&hub.keys, address, TokenTls::Ca, None)
        .await
        .expect("a token");
    world.token = Some(token);
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

#[when("an agent pairs over its connection with the token pasted between quotes")]
async fn pairs_pasted(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let token = world.pasted_token();
    let link = world.hub_link();
    world.pair_over(token, link).await;
}

#[when("an agent pairs over its connection with the token")]
async fn pairs_over(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let token = world.token.clone().expect("a token");
    let link = world.hub_link();
    world.pair_over(token, link).await;
}

#[when("an agent pairs over its connection with the token and nothing answers")]
async fn pairs_unanswered(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let token = world.token.clone().expect("a token");
    world
        .pair_over(token, LoopbackHubConnector::unanswered())
        .await;
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

#[when(regex = r#"^another agent pairs as "([^"]+)"( with a Cyrillic letter)?$"#)]
async fn another_pairs_as(agent: &mut Agent, hostname: String, cyrillic: String) {
    if !cyrillic.is_empty() {
        let is_cyrillic = |c: char| ('\u{400}'..='\u{4ff}').contains(&c);
        assert!(hostname.chars().any(is_cyrillic), "{hostname:?}");
    }
    let world = &mut agent.pairing;
    world.new_token().await;
    let machine = Machine::new(99, &hostname);
    world.pair_and_keep(&machine).await;
    assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
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
        "the ID belongs to another key" => "machine-key-mismatch",
        "the hub's key isn't the token's" => "hub-key-mismatch",
        "too many agents wait for approval" => "pending-limit",
        "the hub removed the agent" => "machine-removed",
        "the hub removed the agent's key" => "machine-key-removed",
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

#[then("the hub lists it as waiting for approval")]
async fn second_listed_pending(agent: &mut Agent) {
    let listed = agent.pairing.second_listed().await;
    assert!(matches!(listed.record.status, AgentStatus::Pending { .. }));
}

#[then(expr = "the hub says the name looks like {string}")]
async fn looks_like(agent: &mut Agent, hostname: String) {
    let world = &agent.pairing;
    let first = world.first.as_ref().expect("the first agent");
    let looks_like = world.second_listed().await.looks_like.expect("a lookalike");
    assert_eq!(looks_like.hostname.as_str(), hostname);
    assert_eq!(looks_like.agent_id, first.agent_id);
}

#[then("the hub says the name looks like no other")]
async fn looks_like_no_other(agent: &mut Agent) {
    let listed = agent.pairing.second_listed().await;
    assert_eq!(listed.looks_like, None);
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

#[then("the agent stops because the address would show the token to the network")]
fn stops_on_plain(agent: &mut Agent) {
    assert_eq!(
        agent.pairing.failure,
        Some(PairFailure::Route(RouteRefused::PlainOffLoopback))
    );
}

#[then("the agent stops because the hub can't be reached")]
fn stops_unreached(agent: &mut Agent) {
    let failure = agent.pairing.failure.as_ref();
    assert!(
        matches!(
            failure,
            Some(PairFailure::Link(LinkError::Unreachable { .. }))
        ),
        "{failure:?}"
    );
}

#[then("the agent connected to nothing")]
fn connected_nowhere(agent: &mut Agent) {
    let connector = agent.pairing.connector.as_ref().expect("a connector");
    assert_eq!(connector.routes(), Vec::new());
}

#[given("an agent paired with a hub")]
async fn an_agent_paired(agent: &mut Agent) {
    paired_under(agent, "laptop".to_owned(), String::new()).await;
}

#[given("a person approved the agent on the hub")]
async fn approved(agent: &mut Agent) {
    let world = &agent.pairing;
    let hub = world.hub();
    let id = world.first().agent_id;
    let waiting = hub.registry.agent(id).await.unwrap().unwrap();
    hub.registry
        .approve(id, waiting.key, hub.clock.now())
        .await
        .unwrap()
        .unwrap();
}

#[when(regex = r"^the agent connects to its hub( twice)?$")]
async fn connects(agent: &mut Agent, twice: String) {
    let world = &mut agent.pairing;
    let machine = world.first().clone();
    let times = if twice.is_empty() { 1 } else { 2 };
    for _ in 0..times {
        world.connect(&machine, None).await;
    }
}

#[given("the agent connected to its hub")]
async fn connected_once(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let machine = world.first().clone();
    world.connect(&machine, None).await;
    world.connected();
}

#[when("another hub that has another key answers its connection")]
async fn another_hub_connected(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let other = Hub::new(500).await;
    let machine = world.first().clone();
    world.connect(&machine, Some(&other)).await;
}

#[when("a machine that holds the same pin but never paired connects to the hub")]
async fn stranger_connects(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let machine = Machine::new(9, "stranger").with_pin_of(world.first()).await;
    world.connect(&machine, None).await;
}

#[when("a machine with another key connects under the agent's ID")]
async fn impostor_connects(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let first = world.first().clone();
    let machine = Machine {
        agent_id: first.agent_id,
        ..Machine::new(99, "desktop")
    }
    .with_pin_of(&first)
    .await;
    world.connect(&machine, None).await;
}

#[then("the hub accepts the agent's proof")]
async fn proof_accepted(agent: &mut Agent) {
    let world = &agent.pairing;
    world.connected();
    let hub = world.hub();
    let record = hub
        .registry
        .agent(world.first().agent_id)
        .await
        .unwrap()
        .expect("the agent's record");
    assert_eq!(record.last_seen, hub.clock.now());
    assert_eq!(world.proved_counters(), [record.counter]);
}

#[then("the agent is told it waits for approval")]
fn told_pending(agent: &mut Agent) {
    assert_eq!(agent.pairing.connected().approval, Approval::Pending);
}

#[then("the agent is told the hub approved it")]
fn told_approved(agent: &mut Agent) {
    assert_eq!(agent.pairing.connected().approval, Approval::Approved);
}

#[then("the hub holds the counter of the agent's second connection")]
fn holds_second_counter(agent: &mut Agent) {
    let world = &agent.pairing;
    assert!(
        world.connections.iter().all(Result::is_ok),
        "{:?}",
        world.connections
    );
    let [first, second] = world.counters.as_slice() else {
        panic!("two connections: {:?}", world.counters);
    };
    assert!(second > first, "{first} then {second}");
    assert_eq!(world.proved_counters(), [*second]);
}

#[then("the agent stops because the hub's key isn't the one it trusts")]
fn stops_on_untrusted_key(agent: &mut Agent) {
    assert_eq!(
        agent.pairing.connect_failure(),
        &ConnectFailure::Challenge(ChallengeRefused::HubKeyMismatch)
    );
}

#[then("the agent sent no proof")]
fn sent_no_proof(agent: &mut Agent) {
    let world = &agent.pairing;
    assert_eq!(world.proved_counters(), Vec::<u64>::new());
    let connector = world.connector.as_ref().expect("a connector");
    assert_eq!(connector.sent().len(), 1);
}

#[then("the hub refuses the machine's proof")]
fn proof_refused(agent: &mut Agent) {
    let failure = agent.pairing.connect_failure();
    let ConnectFailure::Refused(refused) = failure else {
        panic!("the proof wasn't refused: {failure:?}");
    };
    assert_eq!(refused.code.as_str(), "proof-refused");
}

#[then("the hub lists only the paired agent")]
async fn lists_only_paired(agent: &mut Agent) {
    let world = &agent.pairing;
    let ids: Vec<_> = world
        .agents()
        .await
        .iter()
        .map(|record| record.agent_id)
        .collect();
    assert_eq!(ids, [world.first().agent_id]);
}

#[then("the hub still holds the agent's key")]
async fn still_holds_key(agent: &mut Agent) {
    let world = &agent.pairing;
    let first = world.first();
    let seed = first
        .identity
        .seed(SeedFile::AgentKey)
        .await
        .unwrap()
        .expect("the agent's key");
    let agents = world.agents().await;
    let [held] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    assert_eq!(held.key, FakeSigning.public_key(&seed).unwrap());
    assert!(held.is_pending(), "{held:?}");
}

/// The hub's new address in `following_the_hub.feature`; the token's is
/// `https://hub.example:9443`.
const NEW_ADDRESS: &str = "https://hub-new.example:9443";

#[given("the hub answers only at a new address")]
fn answers_at_new(agent: &mut Agent) {
    agent.pairing.only_at = Some(("hub-new.example", 9443));
}

#[given("the hub answers only at the address in its token")]
fn answers_at_token(agent: &mut Agent) {
    agent.pairing.only_at = Some(("hub.example", 9443));
}

#[when("the person gives the agent the hub's new address")]
async fn gives_new_address(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let machine = world.first().clone();
    world.give_address(&machine, NEW_ADDRESS, None).await;
}

#[given("the person gave the agent the hub's new address")]
async fn gave_new_address(agent: &mut Agent) {
    gives_new_address(agent).await;
    agent.pairing.connected();
}

#[when("the person gives the agent an address where another hub that has another key answers")]
async fn gives_other_hubs_address(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let other = Hub::new(500).await;
    let machine = world.first().clone();
    world
        .give_address(&machine, NEW_ADDRESS, Some(&other))
        .await;
}

#[then("the agent keeps the hub's new address")]
async fn keeps_new_address(agent: &mut Agent) {
    let world = &agent.pairing;
    world.connected();
    assert_eq!(
        world.addresses().await,
        [NEW_ADDRESS, "https://hub.example:9443"]
    );
}

#[then("the agent keeps only the address in its token")]
async fn keeps_token_address(agent: &mut Agent) {
    assert_eq!(
        agent.pairing.addresses().await,
        ["https://hub.example:9443"]
    );
}

#[then("the agent tried the hub's new address first")]
fn tried_new_first(agent: &mut Agent) {
    assert_eq!(agent.pairing.tried(), ["hub-new.example:9443"]);
}

#[then("the agent tried the hub's new address, then the address in its token")]
fn tried_new_then_token(agent: &mut Agent) {
    assert_eq!(
        agent.pairing.tried(),
        ["hub-new.example:9443", "hub.example:9443"]
    );
}

#[when(expr = "the same agent pairs with the token under the hostname {string}")]
async fn pairs_under_another_hostname(agent: &mut Agent, hostname: String) {
    let world = &mut agent.pairing;
    let first = world.first.clone().expect("the first agent");
    let machine = Machine {
        hostname: Hostname::parse(&hostname).unwrap(),
        ..first
    };
    world.pair_and_keep(&machine).await;
}

#[then(expr = "the agent is told the hub holds it under {string}")]
fn told_the_held_hostname(agent: &mut Agent, hostname: String) {
    let Some(Ok(paired)) = &agent.pairing.outcome else {
        panic!("{:?}", agent.pairing.outcome);
    };
    assert_eq!(
        paired.held_as.as_ref().map(Hostname::as_str),
        Some(hostname.as_str())
    );
}

#[then(expr = "the hub lists one agent, under the hostname {string}")]
async fn lists_one_under(agent: &mut Agent, hostname: String) {
    let agents = agent.pairing.agents().await;
    let [listed] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    assert_eq!(listed.hostname.as_str(), hostname);
}
