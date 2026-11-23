use std::sync::Arc;

use domain::fleet::{HubAddress, HubPin, KeyFile, PublicKey, TokenTls};
use domain::maintenance::{EpochFloor, Grant, HeldLeases};
use domain::{SignedDuration, Timestamp};

use super::*;
use crate::fakes::{FakeClock, InMemoryIdentity, InMemoryLeaseCache};
use crate::ports::IdentityError;

fn repository(digit: char) -> RepositoryId {
    RepositoryId::parse(&digit.to_string().repeat(64)).unwrap()
}

fn start() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).unwrap()
}

async fn paired() -> Arc<InMemoryIdentity> {
    let identity = Arc::new(InMemoryIdentity::default());
    let key = |byte| PublicKey::from_bytes([byte; 32]).unwrap();
    let pin = HubPin::new(
        key(7),
        TokenTls::Ca,
        key(9),
        HubAddress::parse("https://hub.example:9898").unwrap(),
    );
    identity.write_pin(&pin).await.unwrap();
    identity
}

/// The hub's grant of `photos` at epoch 1 for 7 days, signed at `signed_at`.
fn grants(signed_at: Timestamp) -> RepoGrants {
    RepoGrants {
        generation: 1,
        signed_at,
        grants: vec![Grant {
            repository: repository('a'),
            epoch: epoch(1),
            expires_at: signed_at
                .checked_add(SignedDuration::from_hours(7 * 24))
                .unwrap(),
        }],
    }
}

struct Machine {
    leases: MaintenanceLeases,
    cache: Arc<InMemoryLeaseCache>,
    clock: Arc<FakeClock>,
}

fn machine(identity: Arc<InMemoryIdentity>) -> Machine {
    let cache = Arc::new(InMemoryLeaseCache::default());
    let clock = Arc::new(FakeClock::new(start()));
    Machine {
        leases: MaintenanceLeases::new(identity, cache.clone(), clock.clone()),
        cache,
        clock,
    }
}

#[tokio::test]
async fn a_machine_that_isnt_paired_maintains_without_a_lease() {
    let machine = machine(Arc::new(InMemoryIdentity::default()));
    assert_eq!(machine.leases.permits(Some(&repository('a'))).await, Ok(()));
    assert_eq!(machine.leases.permits(None).await, Ok(()));
}

#[tokio::test]
async fn i1_a_paired_machine_with_no_grant_maintains_nothing() {
    let machine = machine(paired().await);
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::NoneHeld)
    );
}

#[tokio::test]
async fn i1_a_paired_machine_maintains_only_while_its_lease_holds() {
    let machine = machine(paired().await);
    let refused = machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    assert_eq!(refused, vec![]);
    assert_eq!(machine.leases.permits(Some(&repository('a'))).await, Ok(()));
    assert_eq!(
        machine.leases.permits(Some(&repository('b'))).await,
        Err(NoLease::NoneHeld)
    );
    // A repository whose ID its entry never recorded can't hold a lease.
    assert_eq!(machine.leases.permits(None).await, Err(NoLease::NoneHeld));
    machine.clock.advance(SignedDuration::from_hours(7 * 24));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::RanOut)
    );
}

#[tokio::test]
async fn i1_a_pin_that_cant_be_read_maintains_nothing() {
    let machine = machine(Arc::new(InMemoryIdentity::unreadable(
        IdentityError::Damaged {
            file: KeyFile::HubPin,
        },
    )));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::NoneHeld)
    );
}

#[tokio::test]
async fn grants_and_floors_are_kept_and_released() {
    let machine = machine(paired().await);
    machine
        .leases
        .take(&grants(start()), 0, start())
        .await
        .unwrap();
    assert_eq!(
        machine.leases.held().await.unwrap(),
        vec![(repository('a'), epoch(1))]
    );
    let kept = machine.cache.load().await.unwrap();
    assert_eq!(kept.holds(&repository('a'), start()), Ok(epoch(1)));
    machine
        .leases
        .released(&repository('a'), epoch(1))
        .await
        .unwrap();
    assert_eq!(machine.leases.held().await.unwrap(), vec![]);
    let floors: Vec<EpochFloor> = machine
        .cache
        .load()
        .await
        .unwrap()
        .floors()
        .map(|(_, floor)| *floor)
        .collect();
    assert_eq!(floors.len(), 1);
    assert_ne!(machine.cache.load().await.unwrap(), HeldLeases::default());
}

/// A wall clock set back after a grant doesn't lengthen the lease: the
/// machine counts at least the monotonic time that passed since it took
/// the grant, so its lease still runs out on the hub's time (I1).
#[tokio::test]
async fn i1_a_clock_set_back_doesnt_lengthen_a_lease() {
    let machine = machine(paired().await);
    machine.clock.advance(SignedDuration::from_hours(24));
    let asked_at = machine.clock.now();
    machine
        .leases
        .take(&grants(asked_at), 0, asked_at)
        .await
        .unwrap();
    machine.clock.advance(SignedDuration::from_hours(6 * 24));
    machine.clock.set_back(SignedDuration::from_hours(3 * 24));
    assert_eq!(machine.leases.permits(Some(&repository('a'))).await, Ok(()));
    machine.clock.advance(SignedDuration::from_hours(24));
    assert_eq!(
        machine.leases.permits(Some(&repository('a'))).await,
        Err(NoLease::RanOut)
    );
}
