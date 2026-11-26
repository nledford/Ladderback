//! Steps for the untagged scenarios of `features/fleet/leaving_a_hub.feature`:
//! a machine pinned to one hub stops at another hub's token, and leaves its
//! hub, dropping the leases and epoch floors it took (§7.1, §7.3; §16,
//! decision 169).

use std::sync::Arc;

use app::fakes::{FakeClock, InMemoryLeaseCache};
use app::leases::MaintenanceLeases;
use app::ports::{Identity as _, LeaseCache as _};
use cucumber::{given, then, when};
use domain::maintenance::{EpochFloor, HeldLease, HeldLeases, LeaseEpoch};
use domain::{RepositoryId, Timestamp};

use super::{Agent, Hub, PairingWorld};

fn repository() -> RepositoryId {
    RepositoryId::parse(&"a".repeat(64)).unwrap()
}

impl PairingWorld {
    /// The first agent's maintenance leases, judged on a clock of their own.
    fn first_leases(&self) -> (MaintenanceLeases, Arc<InMemoryLeaseCache>) {
        let first = self.first();
        let leases = MaintenanceLeases::new(
            first.identity.clone(),
            first.leases.clone(),
            Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).unwrap(),
            )),
        );
        (leases, first.leases.clone())
    }
}

#[given("the agent holds a lease its hub granted")]
async fn holds_a_lease(agent: &mut Agent) {
    let epoch = LeaseEpoch::new(3).unwrap();
    let held = HeldLeases::from_parts(
        [(
            repository(),
            HeldLease {
                epoch,
                ends_at: Timestamp::from_second(1_900_000_000).unwrap(),
            },
        )],
        [(
            repository(),
            EpochFloor {
                recovery_number: 0,
                generation: 1,
                epoch,
            },
        )],
        [],
    );
    let (leases, cache) = agent.pairing.first_leases();
    cache.save(&held).await.unwrap();
    assert_eq!(
        leases.permits(Some(&repository())).await,
        Ok(Some(epoch)),
        "the lease binds before the agent leaves"
    );
}

#[when("the agent pairs with another hub's token")]
async fn pairs_with_another_hub(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let other = Hub::new(500).await;
    world.token = Some(other.token().await);
    let first = world.first().clone();
    world.outcome = world.pair(&first, Some(&other), false).await;
    world.other_hub = Some(other);
}

#[when("the agent leaves its hub")]
async fn leaves(agent: &mut Agent) {
    let first = agent.pairing.first().clone();
    let left = first.pairing().leave().await;
    agent.pairing.left = Some(left);
}

#[given("the agent left its hub")]
async fn left(agent: &mut Agent) {
    let first = agent.pairing.first().clone();
    assert_eq!(first.pairing().leave().await, Ok(()));
}

#[then("the agent holds no lease and no epoch floor")]
async fn holds_nothing(agent: &mut Agent) {
    let world = &agent.pairing;
    assert_eq!(world.left, Some(Ok(())));
    let held = world.first().leases.load().await.unwrap();
    assert_eq!(held.leases().count(), 0, "{held:?}");
    assert_eq!(held.floors().count(), 0, "{held:?}");
}

#[then("the agent maintains its repositories on its own")]
async fn maintains_alone(agent: &mut Agent) {
    let (leases, _) = agent.pairing.first_leases();
    assert_eq!(leases.permits(Some(&repository())).await, Ok(None));
    assert!(!leases.prune_held(Some(&repository())).await);
}

#[then("the agent pins the other hub's key")]
async fn pins_other(agent: &mut Agent) {
    let world = &agent.pairing;
    assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
    let other = world.other_hub.as_ref().expect("another hub");
    let pin = world.first().identity.pin().await.unwrap().expect("a pin");
    assert_eq!(pin.hub_key, *other.keys.key());
}
