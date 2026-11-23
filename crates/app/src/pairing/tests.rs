use std::sync::Arc;

use domain::fleet::{
    AgentRecord, AgentStatus, ApprovalRefused, BindingKind, Challenge, Fingerprint, Handshakes,
    Hostname, HubAddress, KeyFile, PairAnswer, PairingToken, PublicKey, Refused, SeedFile,
    Signature, TokenTls, WireName,
};
use domain::maintenance::{EpochFloor, HeldLease, HeldLeases, LeaseEpoch};
use domain::{AgentId, RepositoryId, SignedDuration, Timestamp, Uuid};

use super::*;
use crate::fakes::{
    FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, InMemoryLeaseCache,
    SeededRandom,
};
use crate::ports::{
    AgentRegistry, Clock, Identity, IdentityError, LeaseCache, Signing, StoreError,
};

pub(super) const EXPORTER: [u8; 32] = [0xee; 32];

pub(super) struct Hub {
    pub(super) registry: Arc<InMemoryAgentRegistry>,
    identity: Arc<InMemoryIdentity>,
    clock: Arc<FakeClock>,
    pub(super) pairing: HubPairing,
}

impl Hub {
    pub(super) fn new(seed: u64) -> Self {
        let registry = Arc::new(InMemoryAgentRegistry::new());
        let identity = Arc::new(InMemoryIdentity::default());
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        ));
        let pairing = HubPairing::new(
            registry.clone(),
            identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(seed)),
            clock.clone(),
            "0.0.0-test".to_owned(),
        );
        Self {
            registry,
            identity,
            clock,
            pairing,
        }
    }

    pub(super) async fn token(&self, tls: TokenTls) -> PairingToken {
        let keys = self.pairing.keys().await.unwrap();
        let address = HubAddress::parse("https://hub.example:9443").unwrap();
        self.pairing
            .make_token(&keys, address, tls, None)
            .await
            .unwrap()
    }
}

pub(super) fn pinned() -> TokenTls {
    TokenTls::Pinned(Fingerprint::from_digest([7; 32]))
}

pub(super) fn agent(identity: Arc<InMemoryIdentity>) -> AgentPairing {
    agent_keeping(identity, Arc::new(InMemoryLeaseCache::default()))
}

/// An agent whose leases and epoch floors are in `leases`.
fn agent_keeping(identity: Arc<InMemoryIdentity>, leases: Arc<dyn LeaseCache>) -> AgentPairing {
    AgentPairing::new(
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(9_000)),
        leases,
    )
}

pub(super) fn agent_id() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(1))
}

pub(super) fn laptop() -> Hostname {
    Hostname::parse("laptop").unwrap()
}

/// Runs a pairing from `Connect` to the hub's answer, and answers the
/// agent's result.
async fn pair(
    hub: &Hub,
    agent: &AgentPairing,
    token: PairingToken,
) -> Result<PairedHub, AgentPairingError> {
    pair_as(hub, agent, token, laptop()).await
}

/// [`pair`], asking to be held under `hostname`.
async fn pair_as(
    hub: &Hub,
    agent: &AgentPairing,
    token: PairingToken,
    hostname: Hostname,
) -> Result<PairedHub, AgentPairingError> {
    let keys = hub.pairing.keys().await.unwrap();
    let (connect, handshake) = agent.connect(token, agent_id(), hostname, None).await?;
    let (challenge, hub_side) = hub
        .pairing
        .challenge(&keys, &connect, Some(EXPORTER))
        .unwrap();
    let (pair, awaiting) = agent
        .answer_challenge(handshake, &challenge, Some(EXPORTER))
        .await?;
    let answer = hub.pairing.pair(&keys, &hub_side, &pair).await.unwrap();
    agent.take_answer(awaiting, &answer).await
}

/// A machine that pairs again under another hostname, on the token it spent
/// or on a new one, is told the hostname the hub holds it under: a pending
/// machine can't rename itself (decision 120).
#[tokio::test]
async fn an_agent_pairing_again_under_another_hostname_is_told_the_one_held() {
    let hub = Hub::new(1);
    let machine = agent(Arc::default());
    let token = hub.token(pinned()).await;
    let first = pair(&hub, &machine, token.clone()).await.unwrap();
    assert_eq!(
        first.held_as, None,
        "the hostname asked for is the one held"
    );

    let desktop = Hostname::parse("desktop").unwrap();
    let on_the_spent_token = pair_as(&hub, &machine, token, desktop.clone())
        .await
        .unwrap();
    assert_eq!(on_the_spent_token.held_as, Some(laptop()));

    let new_token = hub.token(pinned()).await;
    let on_a_new_token = pair_as(&hub, &machine, new_token, desktop).await.unwrap();
    assert_eq!(on_a_new_token.held_as, Some(laptop()));
}

/// A machine the hub holds approved that pairs again, under another
/// hostname, on the token it spent or on a new one, is told it is approved
/// and the hostname held (#1341); a new or pending one is told it isn't,
/// and so is one whose hub doesn't say, as a hub that predates it doesn't.
#[tokio::test]
async fn an_agent_pairing_again_is_told_whether_the_hub_approved_it() {
    let hub = Hub::new(1);
    let machine = agent(Arc::default());
    let token = hub.token(pinned()).await;
    let first = pair(&hub, &machine, token.clone()).await.unwrap();
    assert!(!first.approved, "a new machine waits for approval");
    let pending = pair(&hub, &machine, token.clone()).await.unwrap();
    assert!(!pending.approved, "a pending machine still waits");

    let held = hub.pairing.agent_named(&laptop()).await.unwrap().unwrap();
    hub.pairing.approve(&held).await.unwrap().unwrap();
    let desktop = Hostname::parse("desktop").unwrap();
    let on_the_spent_token = pair_as(&hub, &machine, token, desktop.clone())
        .await
        .unwrap();
    assert!(on_the_spent_token.approved, "on the spent token");
    assert_eq!(on_the_spent_token.held_as, Some(laptop()));
    let new_token = hub.token(pinned()).await;
    let on_a_new_token = pair_as(&hub, &machine, new_token.clone(), desktop)
        .await
        .unwrap();
    assert!(on_a_new_token.approved, "on a new token");
    assert_eq!(on_a_new_token.held_as, Some(laptop()));

    let keys = hub.pairing.keys().await.unwrap();
    let (connect, handshake) = machine
        .connect(new_token, agent_id(), laptop(), None)
        .await
        .unwrap();
    let (challenge, hub_side) = hub
        .pairing
        .challenge(&keys, &connect, Some(EXPORTER))
        .unwrap();
    let (pair, awaiting) = machine
        .answer_challenge(handshake, &challenge, Some(EXPORTER))
        .await
        .unwrap();
    let PairAnswer::Paired(mut paired) = hub.pairing.pair(&keys, &hub_side, &pair).await.unwrap()
    else {
        panic!("the hub refused");
    };
    paired.approval = None;
    let told = machine
        .take_answer(awaiting, &PairAnswer::Paired(paired))
        .await
        .unwrap();
    assert!(!told.approved, "a hub that doesn't say");
}

/// A machine that pairs again under the hostname it first used is told
/// nothing more, and neither is one whose hub sends no hostname, as a hub
/// that predates it doesn't.
#[tokio::test]
async fn a_hostname_the_hub_holds_that_is_the_one_asked_for_or_unsent_says_nothing() {
    let hub = Hub::new(1);
    let machine = agent(Arc::default());
    let token = hub.token(pinned()).await;
    pair(&hub, &machine, token.clone()).await.unwrap();
    let again = pair(&hub, &machine, token).await.unwrap();
    assert_eq!(again.held_as, None);

    let keys = hub.pairing.keys().await.unwrap();
    let token = hub.token(pinned()).await;
    let (connect, handshake) = machine
        .connect(token, agent_id(), Hostname::parse("desktop").unwrap(), None)
        .await
        .unwrap();
    let (challenge, hub_side) = hub
        .pairing
        .challenge(&keys, &connect, Some(EXPORTER))
        .unwrap();
    let (pair, awaiting) = machine
        .answer_challenge(handshake, &challenge, Some(EXPORTER))
        .await
        .unwrap();
    let PairAnswer::Paired(mut paired) = hub.pairing.pair(&keys, &hub_side, &pair).await.unwrap()
    else {
        panic!("the hub refused");
    };
    assert_eq!(paired.hostname, Some(laptop()));
    paired.hostname = None;
    let told = machine
        .take_answer(awaiting, &PairAnswer::Paired(paired))
        .await
        .unwrap();
    assert_eq!(told.held_as, None);
}

/// The hub's keys are made once and read back the same after that; the
/// recovery seed is kept and its public half is the recovery key.
#[tokio::test]
async fn the_hub_keys_are_made_once() {
    let hub = Hub::new(1);
    let first = hub.pairing.keys().await.unwrap();
    let again = hub.pairing.keys().await.unwrap();
    assert_eq!(first.key(), again.key());
    assert_eq!(first.recovery_key(), again.recovery_key());
    let hub_seed = hub.identity.seed(SeedFile::HubKey).await.unwrap().unwrap();
    assert_eq!(FakeSigning.public_key(&hub_seed).unwrap(), *first.key());
    let recovery = hub
        .identity
        .seed(SeedFile::RecoverySeed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        FakeSigning.public_key(&recovery).unwrap(),
        *first.recovery_key()
    );
    assert_ne!(first.key(), first.recovery_key());
}

/// Once the Recovery Kit is confirmed only the recovery key's public half
/// is left, and the hub reads it.
#[tokio::test]
async fn a_confirmed_kit_leaves_the_recovery_key_enough() {
    let hub = Hub::new(1);
    let first = hub.pairing.keys().await.unwrap();
    hub.identity.forget_recovery_seed().await.unwrap();
    let again = hub.pairing.keys().await.unwrap();
    assert_eq!(first.recovery_key(), again.recovery_key());
}

/// A hub's first start makes its keys, before anything asks for one: the
/// hub key, the recovery key and the recovery seed. A later start reads the
/// same ones.
#[tokio::test]
async fn a_first_start_makes_the_hubs_keys() {
    let hub = Hub::new(1);
    assert!(hub.identity.seed(SeedFile::HubKey).await.unwrap().is_none());

    let HubStart::Keys(first) = hub.pairing.start().await.unwrap() else {
        panic!("a first start makes keys");
    };
    assert!(hub.identity.seed(SeedFile::HubKey).await.unwrap().is_some());
    assert!(
        hub.identity
            .seed(SeedFile::RecoverySeed)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        hub.identity.recovery_key().await.unwrap(),
        Some(*first.recovery_key())
    );

    let HubStart::Keys(again) = hub.pairing.start().await.unwrap() else {
        panic!("a later start reads the keys");
    };
    assert_eq!(first.key(), again.key());
    assert_eq!(first.recovery_key(), again.recovery_key());
}

/// A hub that starts with a key file gone while agents are registered
/// runs, with no new key made, so that it refuses every agent's session
/// until the file is restored.
#[tokio::test]
async fn a_start_with_a_key_gone_and_agents_registered_makes_no_key() {
    let hub = Hub::new(1);
    let token = hub.token(pinned()).await;
    pair(&hub, &agent(Arc::default()), token).await.unwrap();

    let no_key_files = Arc::new(InMemoryIdentity::default());
    let without_keys = HubPairing::new(
        hub.registry.clone(),
        no_key_files.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(2)),
        hub.clock.clone(),
        "0.0.0-test".to_owned(),
    );
    assert!(matches!(
        without_keys.start().await.unwrap(),
        HubStart::Unserved(KeyFile::HubKey)
    ));
    assert!(no_key_files.seed(SeedFile::HubKey).await.unwrap().is_none());
    assert!(
        no_key_files
            .seed(SeedFile::RecoverySeed)
            .await
            .unwrap()
            .is_none()
    );

    let only_hub_key = Arc::new(InMemoryIdentity::default());
    let hub_seed = hub.identity.seed(SeedFile::HubKey).await.unwrap().unwrap();
    only_hub_key
        .keep_seed(SeedFile::HubKey, hub_seed)
        .await
        .unwrap();
    let without_recovery = HubPairing::new(
        hub.registry.clone(),
        only_hub_key.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(2)),
        hub.clock.clone(),
        "0.0.0-test".to_owned(),
    );
    assert!(matches!(
        without_recovery.start().await.unwrap(),
        HubStart::Unserved(KeyFile::RecoveryKey)
    ));
    assert_eq!(only_hub_key.recovery_key().await.unwrap(), None);
}

/// A key file the hub can't read fails its start with the error that says
/// so, rather than being found out at the first session or token.
#[tokio::test]
async fn a_start_that_cant_read_a_key_file_fails() {
    let damaged = IdentityError::Damaged {
        file: KeyFile::HubKey,
    };
    let pairing = HubPairing::new(
        Arc::new(InMemoryAgentRegistry::new()),
        Arc::new(InMemoryIdentity::unreadable(damaged.clone())),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(1)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        )),
        "0.0.0-test".to_owned(),
    );
    assert_eq!(
        pairing.start().await.unwrap_err(),
        HubPairingError::Identity(damaged)
    );
}

/// A hub whose key files are gone while agents are registered makes no new
/// key, which would lock every agent out, and voids no token.
#[tokio::test]
async fn no_key_is_made_again_while_agents_are_registered() {
    let hub = Hub::new(1);
    let token = hub.token(pinned()).await;
    pair(&hub, &agent(Arc::default()), token).await.unwrap();
    hub.token(pinned()).await;

    let no_key_files = Arc::new(InMemoryIdentity::default());
    let without_keys = HubPairing::new(
        hub.registry.clone(),
        no_key_files.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(2)),
        hub.clock.clone(),
        "0.0.0-test".to_owned(),
    );
    assert_eq!(
        without_keys.keys().await.unwrap_err(),
        HubPairingError::KeyMissing {
            file: KeyFile::HubKey
        }
    );
    assert!(
        no_key_files.seed(SeedFile::HubKey).await.unwrap().is_none(),
        "no key is kept for the next start to read"
    );

    let only_hub_key = Arc::new(InMemoryIdentity::default());
    let hub_seed = hub.identity.seed(SeedFile::HubKey).await.unwrap().unwrap();
    only_hub_key
        .keep_seed(SeedFile::HubKey, hub_seed)
        .await
        .unwrap();
    let without_recovery = HubPairing::new(
        hub.registry.clone(),
        only_hub_key.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(2)),
        hub.clock.clone(),
        "0.0.0-test".to_owned(),
    );
    assert_eq!(
        without_recovery.keys().await.unwrap_err(),
        HubPairingError::KeyMissing {
            file: KeyFile::RecoveryKey
        }
    );
    assert_eq!(only_hub_key.recovery_key().await.unwrap(), None);
    let unused = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    assert_eq!(unused.len(), 1, "a key not made voids no token");
}

/// The token `token` as a client that skips the agent's check of the
/// hub's key holds it: its pairing seed, under `hub_key`.
fn under_hub_key(token: &PairingToken, hub_key: PublicKey) -> PairingToken {
    PairingToken::new(
        hub_key,
        token.tls(),
        token.pairing_seed().clone(),
        token.address().clone(),
    )
}

/// A new hub key, made while no agent is registered, voids every unused
/// token, each of which carries the old key; reading the keys again voids
/// nothing. A client that skips the agent's check of the hub's key and
/// pairs with a voided token is refused, and nothing is registered.
#[tokio::test]
async fn a_new_hub_key_voids_the_unused_tokens() {
    let hub = Hub::new(1);
    let old = hub.pairing.keys().await.unwrap();
    let token = hub.token(pinned()).await;
    hub.pairing.keys().await.unwrap();
    let unused = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    assert_eq!(unused.len(), 1, "reading the keys again voids nothing");

    let identity = Arc::new(InMemoryIdentity::default());
    let renewed = Hub {
        registry: hub.registry.clone(),
        identity: identity.clone(),
        clock: hub.clock.clone(),
        pairing: HubPairing::new(
            hub.registry.clone(),
            identity,
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(2)),
            hub.clock.clone(),
            "0.0.0-test".to_owned(),
        ),
    };
    let new = renewed.pairing.keys().await.unwrap();
    assert_ne!(new.key(), old.key());
    let unused = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    assert_eq!(unused, Vec::new());

    let unchecked = under_hub_key(&token, *new.key());
    let refused = pair(&renewed, &agent(Arc::default()), unchecked).await;
    let Err(AgentPairingError::Refused(refused)) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(refused.code.as_str(), "hub-key-mismatch");
    assert!(hub.registry.agents().await.unwrap().is_empty());
}

/// A token whose record carries another hub key than the hub's is refused
/// at `Pair` though unused, and stays unused: a token made under keys the
/// hub no longer holds pairs no one.
#[tokio::test]
async fn a_token_of_another_hub_key_is_refused() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let stale = Hub::new(2).pairing.keys().await.unwrap();
    assert_ne!(stale.key(), keys.key());
    let address = HubAddress::parse("https://hub.example:9443").unwrap();
    let token = hub
        .pairing
        .make_token(&stale, address, pinned(), None)
        .await
        .unwrap();
    let unchecked = under_hub_key(&token, *keys.key());
    let refused = pair(&hub, &agent(Arc::default()), unchecked).await;
    let Err(AgentPairingError::Refused(refused)) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(refused.code.as_str(), "hub-key-mismatch");
    assert!(hub.registry.agents().await.unwrap().is_empty());
    let unused = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    assert_eq!(unused.len(), 1);
}

/// A token carries the hub key and keeps only its pairing key's public
/// half on the hub, for 15 minutes or the life asked for within bounds.
#[tokio::test]
async fn a_token_is_kept_by_its_public_half() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let token = hub.token(pinned()).await;
    assert_eq!(token.hub_key(), keys.key());
    let kept = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    let pairing_key = FakeSigning.public_key(token.pairing_seed()).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].pairing_key, pairing_key);
    assert_eq!(kept[0].kind, BindingKind::TlsExporter);
    assert_eq!(
        kept[0].expires_at,
        hub.clock.now() + SignedDuration::from_mins(15)
    );

    let address = HubAddress::parse("https://hub.example").unwrap();
    let day = Some(SignedDuration::from_hours(24));
    let ca = hub
        .pairing
        .make_token(&keys, address.clone(), TokenTls::Ca, day)
        .await
        .unwrap();
    let ca_key = FakeSigning.public_key(ca.pairing_seed()).unwrap();
    let kept = hub.registry.unused_tokens(hub.clock.now()).await.unwrap();
    let ca_kept = kept.iter().find(|t| t.pairing_key == ca_key).unwrap();
    assert_eq!(ca_kept.kind, BindingKind::None);
    assert_eq!(
        ca_kept.expires_at,
        hub.clock.now() + SignedDuration::from_hours(24)
    );

    let too_long = Some(SignedDuration::from_hours(25));
    assert_eq!(
        hub.pairing
            .make_token(&keys, address, TokenTls::Ca, too_long)
            .await
            .unwrap_err(),
        HubPairingError::TokenLife
    );
}

/// The twenty-first unused token is refused; a revoked one frees a place.
#[tokio::test]
async fn at_most_twenty_tokens_are_unused() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let address = HubAddress::parse("https://hub.example").unwrap();
    let mut first = None;
    for _ in 0..20 {
        let token = hub
            .pairing
            .make_token(&keys, address.clone(), TokenTls::Ca, None)
            .await
            .unwrap();
        first.get_or_insert(token);
    }
    assert_eq!(
        hub.pairing
            .make_token(&keys, address.clone(), TokenTls::Ca, None)
            .await
            .unwrap_err(),
        HubPairingError::TooManyUnusedTokens
    );
    let first_key = FakeSigning
        .public_key(first.unwrap().pairing_seed())
        .unwrap();
    let id = domain::fleet::TokenId::of_pairing_key(&first_key);
    assert!(hub.pairing.revoke_token(id).await.unwrap());
    assert!(!hub.pairing.revoke_token(id).await.unwrap());
    assert!(
        hub.pairing
            .make_token(&keys, address, TokenTls::Ca, None)
            .await
            .is_ok()
    );
}

/// A first pairing pins the hub's key, the token's TLS and address and the
/// recovery key, and the hub keeps the agent pending under its key.
#[tokio::test]
async fn a_pairing_pins_the_hub_and_is_kept_pending() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let token = hub.token(pinned()).await;
    let address = token.address().clone();
    let identity = Arc::new(InMemoryIdentity::default());
    let paired = pair(&hub, &agent(identity.clone()), token).await.unwrap();
    assert_eq!(paired.pin.hub_key, *keys.key());
    assert_eq!(paired.pin.recovery_key, *keys.recovery_key());
    assert_eq!(paired.pin.tls, pinned());
    assert_eq!(paired.pin.address, address);
    assert_eq!(identity.pin().await.unwrap(), Some(paired.pin.clone()));
    let agent_seed = identity.seed(SeedFile::AgentKey).await.unwrap().unwrap();
    assert_eq!(
        FakeSigning.public_key(&agent_seed).unwrap(),
        paired.agent_key
    );
    let agents = hub.registry.agents().await.unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].key, paired.agent_key);
    assert_eq!(agents[0].agent_id, agent_id());
    assert_eq!(agents[0].counter, 1, "the attempt's counter");
    assert!(matches!(agents[0].status, AgentStatus::Pending { .. }));
}

/// A pin of the same hub is kept as it is, its generation included.
#[tokio::test]
async fn a_pin_of_the_same_hub_is_kept() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let mut pin = domain::fleet::HubPin::new(
        *keys.key(),
        TokenTls::Ca,
        *keys.recovery_key(),
        HubAddress::parse("https://old.example").unwrap(),
    );
    pin.generation = 4;
    identity.write_pin(&pin).await.unwrap();
    let token = hub.token(pinned()).await;
    let paired = pair(&hub, &agent(identity.clone()), token).await.unwrap();
    assert_eq!(paired.pin, pin);
    assert_eq!(identity.pin().await.unwrap(), Some(pin));
}

/// A machine the hub no longer knows pairs again with a new token at the
/// hub's new address (`ladderback pair <address>`): it keeps its pin and
/// reaches the hub at that address from then on (decision 120,
/// "Addresses and the transport statement").
#[tokio::test]
async fn pairing_again_at_another_address_keeps_that_address() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let mut pin = domain::fleet::HubPin::new(
        *keys.key(),
        pinned(),
        *keys.recovery_key(),
        HubAddress::parse("https://old.example").unwrap(),
    );
    pin.generation = 4;
    identity.write_pin(&pin).await.unwrap();
    let moved = HubAddress::parse("https://hub-new.example:9443").unwrap();
    let token = hub.token(pinned()).await.with_address(moved.clone());
    let paired = pair(&hub, &agent(identity.clone()), token).await.unwrap();
    pin.given_address = Some(moved);
    assert_eq!(paired.pin, pin);
    assert_eq!(identity.pin().await.unwrap(), Some(pin));
}

/// An agent that pins another hub sends nothing before its person
/// confirms the change.
#[tokio::test]
async fn an_agent_pinned_to_another_hub_stops() {
    let hub = Hub::new(1);
    let other = Hub::new(500);
    let other_keys = other.pairing.keys().await.unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let pin = domain::fleet::HubPin::new(
        *other_keys.key(),
        TokenTls::Ca,
        *other_keys.recovery_key(),
        HubAddress::parse("https://other.example").unwrap(),
    );
    identity.write_pin(&pin).await.unwrap();
    let token = hub.token(pinned()).await;
    let hub_key = *token.hub_key();
    let refused = agent(identity.clone())
        .connect(token, agent_id(), laptop(), None)
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        AgentPairingError::PinnedToAnotherHub {
            pinned: *other_keys.key(),
            token: hub_key,
        }
    );
    assert_eq!(identity.pin().await.unwrap(), Some(pin));
}

/// A challenge whose proof isn't the hub key's over this connection, or
/// that names no handshake this build speaks, gets no `Pair`.
#[tokio::test]
async fn a_bad_challenge_gets_nothing_of_the_token() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let agent = agent(Arc::default());
    let (connect, _) = agent
        .connect(hub.token(pinned()).await, agent_id(), laptop(), None)
        .await
        .unwrap();
    let (good, _) = hub
        .pairing
        .challenge(&keys, &connect, Some(EXPORTER))
        .unwrap();
    let forged = Challenge {
        hub_proof: Signature::from_bytes([0; 64]).unwrap(),
        ..good
    };
    let another_session = good;
    let none_spoken = Challenge {
        hub_handshakes: Handshakes::from_bits(0b10),
        ..good
    };
    for (challenge, exporter, expected) in [
        (forged, Some(EXPORTER), AgentPairingError::HubProofInvalid),
        (
            another_session,
            Some([0x11; 32]),
            AgentPairingError::HubProofInvalid,
        ),
        (
            none_spoken,
            Some(EXPORTER),
            AgentPairingError::HubProofInvalid,
        ),
        (good, None, AgentPairingError::BindingUnavailable),
    ] {
        let (_, handshake) = agent
            .connect(hub.token(pinned()).await, agent_id(), laptop(), None)
            .await
            .unwrap();
        let refused = agent
            .answer_challenge(handshake, &challenge, exporter)
            .await
            .unwrap_err();
        assert_eq!(refused, expected);
    }
}

/// A hub that signs a handshake set this build doesn't speak is told
/// apart from a forged proof.
#[tokio::test]
async fn a_hub_that_speaks_no_common_handshake_is_named() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let agent = agent(Arc::default());
    let (connect, handshake) = agent
        .connect(hub.token(TokenTls::Ca).await, agent_id(), laptop(), None)
        .await
        .unwrap();
    let hub_seed = hub.identity.seed(SeedFile::HubKey).await.unwrap().unwrap();
    let hub_nonce = domain::fleet::Nonce::from_bytes([3; 32]);
    let handshakes = Handshakes::from_bits(0b10);
    let proof = domain::fleet::SignedMessage::HubProof {
        hub_key: *keys.key(),
        agent_id: connect.agent_id,
        agent_nonce: connect.agent_nonce,
        hub_nonce,
        binding: domain::fleet::Binding::None,
        handshakes,
    };
    let challenge = Challenge {
        hub_nonce,
        hub_key: *keys.key(),
        hub_handshakes: handshakes,
        hub_proof: FakeSigning.sign(&hub_seed, &proof.to_bytes()).unwrap(),
    };
    assert_eq!(
        agent
            .answer_challenge(handshake, &challenge, None)
            .await
            .unwrap_err(),
        AgentPairingError::NoCommonHandshake
    );
}

/// The hub refuses a session without the binding a pinned token's agent
/// names.
#[tokio::test]
async fn the_hub_needs_the_binding_the_agent_names() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let (connect, _) = agent(Arc::default())
        .connect(hub.token(pinned()).await, agent_id(), laptop(), None)
        .await
        .unwrap();
    assert_eq!(
        hub.pairing.challenge(&keys, &connect, None).unwrap_err(),
        HubPairingError::BindingUnavailable
    );
}

/// A `Pair` whose agent signature or pairing signature doesn't hold is
/// refused as a proof would be, and nothing is registered.
#[tokio::test]
async fn a_pair_with_a_bad_signature_is_refused() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let agent = agent(Arc::default());
    for spoil_agent_signature in [true, false] {
        let token = hub.token(pinned()).await;
        let (connect, handshake) = agent
            .connect(token, agent_id(), laptop(), None)
            .await
            .unwrap();
        let (challenge, hub_side) = hub
            .pairing
            .challenge(&keys, &connect, Some(EXPORTER))
            .unwrap();
        let (mut pair, awaiting) = agent
            .answer_challenge(handshake, &challenge, Some(EXPORTER))
            .await
            .unwrap();
        let spoiled = Signature::from_bytes([0; 64]).unwrap();
        if spoil_agent_signature {
            pair.agent_signature = spoiled;
        } else {
            pair.pairing_signature = spoiled;
        }
        let answer = hub.pairing.pair(&keys, &hub_side, &pair).await.unwrap();
        let PairAnswer::Refused(refused) = &answer else {
            panic!("{answer:?}");
        };
        assert_eq!(refused.code.as_str(), "proof-refused");
        assert_eq!(refused.holder, None);
        let taken = agent.take_answer(awaiting, &answer).await.unwrap_err();
        assert!(matches!(taken, AgentPairingError::Refused(_)), "{taken:?}");
    }
    assert!(hub.registry.agents().await.unwrap().is_empty());
}

/// An answer the hub didn't sign, a refusal or a `Paired`, is a dropped
/// connection: the agent acts on nothing in it and pins nothing.
#[tokio::test]
async fn an_unsigned_answer_is_a_dropped_connection() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let agent = agent(identity.clone());
    let unsigned = Signature::from_bytes([0; 64]).unwrap();
    let answers = [
        PairAnswer::Refused(Refused {
            code: WireName::parse("identity-conflict").unwrap(),
            holder: None,
            signature: unsigned,
        }),
        PairAnswer::Paired(domain::fleet::Paired {
            recovery_key: *keys.recovery_key(),
            signature: unsigned,
            hostname: None,
            approval: None,
        }),
    ];
    for answer in answers {
        let (connect, handshake) = agent
            .connect(hub.token(pinned()).await, agent_id(), laptop(), None)
            .await
            .unwrap();
        let (challenge, _) = hub
            .pairing
            .challenge(&keys, &connect, Some(EXPORTER))
            .unwrap();
        let (_, awaiting) = agent
            .answer_challenge(handshake, &challenge, Some(EXPORTER))
            .await
            .unwrap();
        assert_eq!(
            agent.take_answer(awaiting, &answer).await.unwrap_err(),
            AgentPairingError::Unsigned
        );
    }
    assert_eq!(identity.pin().await.unwrap(), None);
}

/// A signed refusal of a taken hostname names the holder beside its
/// signed code.
#[tokio::test]
async fn a_taken_hostname_names_its_holder() {
    let hub = Hub::new(1);
    let first = hub.token(pinned()).await;
    pair(&hub, &agent(Arc::default()), first).await.unwrap();
    let second = hub.token(pinned()).await;
    let other = AgentPairing::new(
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77_000)),
        Arc::new(InMemoryLeaseCache::default()),
    );
    let keys = hub.pairing.keys().await.unwrap();
    let other_id = AgentId::from_uuid(Uuid::from_u128(2));
    let (connect, handshake) = other
        .connect(second, other_id, Hostname::parse("LAPTOP").unwrap(), None)
        .await
        .unwrap();
    let (challenge, hub_side) = hub
        .pairing
        .challenge(&keys, &connect, Some(EXPORTER))
        .unwrap();
    let (pair, awaiting) = other
        .answer_challenge(handshake, &challenge, Some(EXPORTER))
        .await
        .unwrap();
    let answer = hub.pairing.pair(&keys, &hub_side, &pair).await.unwrap();
    let AgentPairingError::Refused(refused) =
        other.take_answer(awaiting, &answer).await.unwrap_err()
    else {
        panic!("{answer:?}");
    };
    assert_eq!(refused.code.as_str(), "hostname-taken");
    let holder = refused.holder.unwrap();
    assert_eq!(holder.hostname, laptop());
    assert_eq!(
        Some(holder.last_seen),
        domain::fleet::EpochSeconds::from_timestamp(hub.clock.now())
    );
}

/// A record the hub holds of agent `n` under `name`, pending or approved.
fn held(n: u8, name: &str, pending: bool) -> AgentRecord {
    let at = Timestamp::from_second(1_700_000_000).unwrap();
    AgentRecord {
        agent_id: AgentId::from_uuid(Uuid::from_u128(u128::from(n))),
        key: PublicKey::from_bytes([n; 32]).unwrap(),
        hostname: Hostname::parse(name).unwrap(),
        status: if pending {
            AgentStatus::Pending { since: at }
        } else {
            AgentStatus::Approved { at }
        },
        counter: 1,
        last_seen: at,
        zone: None,
    }
}

/// The hub lists only the agents waiting for approval, and finds an agent
/// by a hostname that compares equal to its own, pending or approved.
#[tokio::test]
async fn the_hub_lists_the_waiting_agents_and_finds_one_by_its_name() {
    let hub = Hub::new(1);
    let (laptop, desktop, nas) = (
        held(1, "laptop", true),
        held(2, "desktop", false),
        held(3, "nas", true),
    );
    for one in [&laptop, &desktop, &nas] {
        hub.registry.hold(one.clone());
    }
    assert_eq!(
        hub.pairing.pending().await.unwrap(),
        vec![laptop.clone(), nas]
    );
    let named = |name: &str| Hostname::parse(name).unwrap();
    let pairing = &hub.pairing;
    assert_eq!(
        pairing.agent_named(&named("LAPTOP")).await.unwrap(),
        Some(laptop)
    );
    assert_eq!(
        pairing.agent_named(&named("desktop")).await.unwrap(),
        Some(desktop)
    );
    assert_eq!(pairing.agent_named(&named("phone")).await.unwrap(), None);
}

/// A person's approval approves the agent at the hub's time, once.
#[tokio::test]
async fn an_approval_approves_the_agent_as_compared_at_the_hubs_time() {
    let hub = Hub::new(1);
    let laptop = held(1, "laptop", true);
    hub.registry.hold(laptop.clone());
    let approved = AgentRecord {
        status: AgentStatus::Approved {
            at: hub.clock.now(),
        },
        ..laptop.clone()
    };
    assert_eq!(
        hub.pairing.approve(&laptop).await.unwrap(),
        Ok(approved.clone())
    );
    assert_eq!(hub.registry.agents().await.unwrap(), vec![approved]);
    assert_eq!(
        hub.pairing.approve(&laptop).await.unwrap(),
        Err(ApprovalRefused::AlreadyApproved)
    );
}

/// Nothing the pairing shows in its `Debug` form holds a seed, the token's
/// text or the hub's seed (I4).
#[tokio::test]
async fn nothing_shown_holds_a_secret() {
    let hub = Hub::new(1);
    let keys = hub.pairing.keys().await.unwrap();
    let token = hub.token(pinned()).await;
    let text = token.to_text();
    let pairing_seed = token.pairing_seed().to_hex();
    let hub_seed = hub.identity.seed(SeedFile::HubKey).await.unwrap().unwrap();
    let agent = agent(Arc::default());
    let (connect, handshake) = agent
        .connect(token, agent_id(), laptop(), None)
        .await
        .unwrap();
    let (challenge, hub_side) = hub
        .pairing
        .challenge(&keys, &connect, Some(EXPORTER))
        .unwrap();
    let shown_handshake = format!("{handshake:?}");
    let (pair, awaiting) = agent
        .answer_challenge(handshake, &challenge, Some(EXPORTER))
        .await
        .unwrap();
    let shown = [
        format!("{keys:?}"),
        format!("{hub_side:?}"),
        shown_handshake,
        format!("{pair:?}"),
        format!("{awaiting:?}"),
    ];
    let secrets = [
        text.expose().to_owned(),
        pairing_seed.to_string(),
        hub_seed.to_hex().to_string(),
        format!("{:?}", hub_seed.expose()),
    ];
    for one in &shown {
        for secret in &secrets {
            assert!(!one.contains(secret.as_str()), "{one}");
        }
    }
}

/// What a machine keeps of the hub it paired with: a lease on one
/// repository at epoch 3, and its floor there.
fn kept() -> HeldLeases {
    let repository = RepositoryId::parse(&"a".repeat(64)).unwrap();
    let epoch = LeaseEpoch::new(3).unwrap();
    HeldLeases::from_parts(
        [(
            repository.clone(),
            HeldLease {
                epoch,
                ends_at: Timestamp::from_second(1_900_000_000).unwrap(),
            },
        )],
        [(
            repository,
            EpochFloor {
                recovery_number: 0,
                generation: 1,
                epoch,
            },
        )],
    )
}

/// A machine that pins no hub forgets the leases and epoch floors it kept
/// when it pairs: they are another hub's, whose epochs the new hub's don't
/// follow, so they would refuse its every grant (#1481).
#[tokio::test]
async fn i1_a_machine_pairing_with_a_new_hub_forgets_the_old_hubs_leases_and_floors() {
    let hub = Hub::new(1);
    let cache = Arc::new(InMemoryLeaseCache::default());
    cache.save(&kept()).await.unwrap();
    let agent = agent_keeping(Arc::default(), cache.clone());
    pair(&hub, &agent, hub.token(pinned()).await).await.unwrap();
    assert_eq!(cache.load().await.unwrap(), HeldLeases::default());
}

/// A cache that reads but can't be written.
struct Unwritable;

#[async_trait::async_trait]
impl LeaseCache for Unwritable {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        Ok(kept())
    }

    async fn save(&self, _leases: &HeldLeases) -> Result<(), StoreError> {
        Err(StoreError::Unavailable {
            detail: "read-only".to_owned(),
        })
    }
}

/// A machine that can't forget the old hub's floors pins no new hub, so
/// the floors never stay under a new pin, and pairing again tries again.
#[tokio::test]
async fn a_machine_that_cant_forget_the_old_hubs_floors_pins_no_new_hub() {
    let hub = Hub::new(1);
    let identity = Arc::new(InMemoryIdentity::default());
    let agent = agent_keeping(identity.clone(), Arc::new(Unwritable));
    let outcome = pair(&hub, &agent, hub.token(pinned()).await).await;
    assert!(
        matches!(outcome, Err(AgentPairingError::Leases(_))),
        "{outcome:?}"
    );
    assert_eq!(identity.pin().await.unwrap(), None);
}

/// A machine that pairs again with the hub it pins keeps its leases and
/// epoch floors: that hub's epochs only rose past them, and they still
/// catch a rewound copy of it (decision 137).
#[tokio::test]
async fn i1_a_machine_pairing_again_with_its_hub_keeps_its_leases_and_floors() {
    let hub = Hub::new(1);
    let cache = Arc::new(InMemoryLeaseCache::default());
    let agent = agent_keeping(Arc::default(), cache.clone());
    pair(&hub, &agent, hub.token(pinned()).await).await.unwrap();
    cache.save(&kept()).await.unwrap();
    pair(&hub, &agent, hub.token(pinned()).await).await.unwrap();
    assert_eq!(cache.load().await.unwrap(), kept());
}
