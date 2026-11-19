//! Steps for `features/fleet/hub_tls_key.feature`: the hub's TLS key, made
//! on its first start and never replaced by the hub itself (§16, decision
//! 120, "The hub key").

use std::sync::Arc;

use app::fakes::{FakeTlsKeys, InMemoryIdentity};
use app::hub_tls::{HubTlsKey, ServedTlsKey, TlsKeyRefusal};
use app::ports::{Identity, TlsKeys};
use cucumber::{given, then, when};
use domain::fleet::{Fingerprint, TlsKey};

use super::Agent;

/// The hub's data folder, its TLS keys, the fingerprint it recorded at its
/// first start, and how its latest start went.
#[derive(Debug, Default)]
pub struct HubTlsKeyWorld {
    identity: Arc<InMemoryIdentity>,
    keys: Arc<FakeTlsKeys>,
    recorded: Option<Fingerprint>,
    /// The keys made before the latest start.
    made_before: u64,
    /// What the key file held before the latest start, when a step put it.
    planted: Option<Vec<u8>>,
    started: Option<Result<ServedTlsKey, TlsKeyRefusal>>,
}

impl HubTlsKeyWorld {
    async fn start(&mut self) {
        self.made_before = self.keys.made();
        let use_case = HubTlsKey::new(self.identity.clone(), self.keys.clone());
        self.started = Some(use_case.take(self.recorded).await);
    }

    fn served(&self) -> &ServedTlsKey {
        match self.started.as_ref().expect("the hub has started") {
            Ok(served) => served,
            Err(refusal) => panic!("the start was refused: {refusal:?}"),
        }
    }

    fn refusal(&self) -> &TlsKeyRefusal {
        match self.started.as_ref().expect("the hub has started") {
            Ok(served) => panic!("the start served {served:?}"),
            Err(refusal) => refusal,
        }
    }

    async fn kept(&self) -> Option<TlsKey> {
        self.identity
            .tls_key()
            .await
            .expect("the TLS key file reads")
    }
}

#[given("a hub that has never started with built-in TLS")]
fn never_started(_agent: &mut Agent) {}

#[given("a hub that made its TLS key on its first start")]
async fn made_its_key(agent: &mut Agent) {
    let world = &mut agent.hub_tls_key;
    world.start().await;
    world.recorded = Some(world.served().fingerprint);
}

#[given("the hub's TLS key is gone from its data folder")]
fn key_is_gone(agent: &mut Agent) {
    agent.hub_tls_key.identity = Arc::default();
}

#[given("the hub's data folder holds another TLS key in its place")]
async fn another_key(agent: &mut Agent) {
    let world = &mut agent.hub_tls_key;
    world.identity = Arc::default();
    let other = world.keys.generate().expect("another key");
    world.identity.keep_tls_key(other).await.expect("kept");
}

#[given("the hub's TLS key file holds something that isn't a key")]
async fn not_a_key(agent: &mut Agent) {
    let world = &mut agent.hub_tls_key;
    let bytes = b"canary, not a key".to_vec();
    let planted = TlsKey::from_der(bytes.clone()).expect("bytes of a key's size");
    world.identity.keep_tls_key(planted).await.expect("kept");
    world.planted = Some(bytes);
}

#[when(regex = r"^the hub starts with built-in TLS( again)?$")]
async fn starts(agent: &mut Agent) {
    agent.hub_tls_key.start().await;
}

#[then("the hub keeps a new TLS key in its data folder")]
async fn keeps_a_new_key(agent: &mut Agent) {
    let world = &agent.hub_tls_key;
    assert_eq!(world.keys.made(), world.made_before + 1, "one key was made");
    assert!(world.kept().await.is_some(), "the key is kept");
}

#[then("the hub serves the TLS key it keeps, under that key's fingerprint")]
async fn serves_the_kept_key(agent: &mut Agent) {
    let world = &agent.hub_tls_key;
    let kept = world.kept().await.expect("a kept key");
    let served = world.served();
    assert_eq!(served.key.der(), kept.der());
    assert_eq!(
        served.fingerprint,
        world
            .keys
            .fingerprint(&kept)
            .expect("the kept key's fingerprint")
    );
}

#[then("the hub serves the TLS key under the fingerprint it recorded at its first start")]
async fn serves_the_recorded_key(agent: &mut Agent) {
    let world = &agent.hub_tls_key;
    let kept = world.kept().await.expect("a kept key");
    let served = world.served();
    assert_eq!(Some(served.fingerprint), world.recorded);
    assert_eq!(served.key.der(), kept.der());
}

#[then("no new TLS key was made")]
fn no_new_key(agent: &mut Agent) {
    let world = &agent.hub_tls_key;
    assert_eq!(world.keys.made(), world.made_before);
}

#[then("the start is refused because the hub's TLS key is missing")]
async fn refused_as_missing(agent: &mut Agent) {
    let world = &agent.hub_tls_key;
    assert_eq!(world.refusal(), &TlsKeyRefusal::Missing);
    assert!(
        world.kept().await.is_none(),
        "nothing was kept in its place"
    );
}

#[then("the start is refused because the TLS key isn't the one the hub recorded")]
fn refused_as_another(agent: &mut Agent) {
    assert_eq!(agent.hub_tls_key.refusal(), &TlsKeyRefusal::AnotherKey);
}

#[then("the start is refused because the TLS key file holds no key")]
fn refused_as_not_a_key(agent: &mut Agent) {
    assert_eq!(agent.hub_tls_key.refusal(), &TlsKeyRefusal::NotAKey);
}

#[then("the TLS key file still holds what it held")]
async fn left_as_it_was(agent: &mut Agent) {
    let world = &agent.hub_tls_key;
    let kept = world.kept().await.expect("the key file is there");
    assert_eq!(Some(kept.der().to_vec()), world.planted);
}
