//! Steps for `features/fleet/removed_new_id.feature`: an agent its hub
//! removed or rejected takes a new machine ID and a new agent key, starts
//! again under them and pairs again, waiting for approval under the new ID,
//! while its old ID and its old key stay refused (technical design §7.1;
//! §16, decisions 150 and 154).

use std::sync::Arc;

use app::fakes::{FakeClock, InMemoryIdentity, InMemoryLeaseCache};
use app::leases::MaintenanceLeases;
use app::pairing::ConnectFailure;
use app::ports::{AgentRegistry as _, Clock as _, Identity as _};
use app::sync::SyncFailure;
use cucumber::{then, when};
use domain::fleet::SeedFile;
use domain::maintenance::NoLease;
use domain::{AgentId, Timestamp, Uuid};

use super::Agent;
use super::sync_replay::first;

#[then("the agent's sync is refused because its hub removed it")]
async fn sync_refused_removed(agent: &mut Agent) {
    let side = first(agent);
    let Some(Err(SyncFailure::Connect(ConnectFailure::Refused(refused)))) = side.synced.last()
    else {
        panic!("{:?}", side.synced.last());
    };
    assert_eq!(refused.code.as_str(), "agent-removed");
}

/// Its standing alone would let it maintain, as a machine its hub refused
/// may: what stops it is that a paired machine maintains only under a lease
/// from its hub (decision 151), and its hub granted it none.
#[then("the agent maintains only under a lease from its hub, and holds none")]
async fn holds_no_lease(agent: &mut Agent) {
    assert!(agent.machine.standing().maintains());
    let leases = MaintenanceLeases::new(
        agent.pairing.first().identity.clone(),
        Arc::new(InMemoryLeaseCache::default()),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        )),
    );
    assert_eq!(
        leases.permits(Some(agent.forget.repository_id())).await,
        Err(NoLease::NoneHeld)
    );
}

/// The daemon starts again: the new ID is its machine ID from now on, and
/// it keeps its pin and the new agent key the new ID made.
#[when("the agent starts again under its new machine ID")]
async fn starts_under_new_id(agent: &mut Agent) {
    let id = agent.machine.new_id();
    agent.machine.start_again();
    let world = &mut agent.pairing;
    let machine = world.first.as_mut().expect("the first agent");
    world.old_id = Some(machine.agent_id);
    machine.agent_id = id;
}

#[then("the hub holds the agent under a key other than the one it removed")]
async fn holds_another_key(agent: &mut Agent) {
    let world = &agent.pairing;
    let hub = world.hub();
    let old_id = world.old_id.expect("a new machine ID was taken");
    let removed = hub.registry.tombstone(old_id).await.unwrap();
    let removed = removed.expect("the removed agent's tombstone");
    let waiting = hub.registry.agent(world.first().agent_id).await.unwrap();
    assert_ne!(waiting.expect("the agent").key, removed.key);
}

#[when("a person approves the agent on the hub")]
async fn approves(agent: &mut Agent) {
    let world = &agent.pairing;
    let hub = world.hub();
    let id = world.first().agent_id;
    let waiting = hub.registry.agent(id).await.unwrap().expect("the agent");
    hub.registry
        .approve(id, waiting.key, hub.clock.now())
        .await
        .unwrap()
        .unwrap();
}

/// Something that holds the agent's old key, as a copy of its data folder
/// from before the new ID would, connects under its old machine ID.
#[when("the agent's old machine ID connects to its hub")]
async fn old_id_connects(agent: &mut Agent) {
    let old_key = agent.machine.key_before();
    let world = &mut agent.pairing;
    let mut machine = world.first().clone();
    let identity = Arc::new(InMemoryIdentity::default());
    let pin = machine.identity.pin().await.unwrap().expect("a pin");
    identity.write_pin(&pin).await.unwrap();
    identity
        .keep_seed(SeedFile::AgentKey, old_key)
        .await
        .unwrap();
    machine.identity = identity;
    machine.agent_id = world.old_id.expect("a new machine ID was taken");
    world.connect(&machine, None).await;
}

/// A copy of the removed machine's key files, under a machine ID of its
/// own, asks to pair with a token the hub's owner made.
#[when("a machine holding the removed agent's key pairs under a new machine ID with a new token")]
async fn removed_key_pairs(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.new_token().await;
    let mut machine = world.first().clone();
    machine.agent_id = AgentId::from_uuid(Uuid::from_u128(0x1489));
    world.pair_and_keep(&machine).await;
}
