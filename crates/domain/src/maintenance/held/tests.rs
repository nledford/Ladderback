use super::*;
use crate::SignedDuration;

fn repository(digit: char) -> RepositoryId {
    RepositoryId::parse(&digit.to_string().repeat(64)).unwrap()
}

/// A time on the hub's clock.
fn hub(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).unwrap()
}

const DAY: i64 = 24 * 3600;

/// The hub's grants of `photos` at `number`, signed at `signed_at` and
/// running out 7 days later.
fn grants(number: u64, signed_at: Timestamp) -> RepoGrants {
    RepoGrants {
        generation: 1,
        signed_at,
        grants: vec![Grant {
            repository: repository('a'),
            epoch: epoch(number),
            expires_at: signed_at
                .checked_add(SignedDuration::from_hours(7 * 24))
                .unwrap(),
        }],
    }
}

#[test]
fn a_new_machine_holds_no_lease() {
    let held = HeldLeases::default();
    assert_eq!(held.holds(&repository('a'), hub(0)), Err(NoLease::NoneHeld));
}

#[test]
fn a_lease_runs_out_on_the_hubs_time_whatever_the_machines_clock_says() {
    for skew in [-3 * DAY, 0, 2 * DAY] {
        let machine = |seconds: i64| hub(seconds + skew);
        let mut held = HeldLeases::default();
        assert_eq!(held.take(&grants(1, hub(0)), 0, machine(0)), vec![]);
        let last = machine(7 * DAY - 1);
        assert_eq!(
            held.holds(&repository('a'), last),
            Ok(epoch(1)),
            "skew {skew}"
        );
        assert_eq!(
            held.holds(&repository('a'), machine(7 * DAY)),
            Err(NoLease::RanOut),
            "skew {skew}"
        );
    }
}

#[test]
fn the_end_is_measured_from_when_the_request_left() {
    // The hub signed 30 s after the request left: the machine counts the 7
    // days from its request, so it stops 30 s before the hub's expiry.
    let mut held = HeldLeases::default();
    held.take(&grants(1, hub(30)), 0, hub(0));
    assert_eq!(held.holds(&repository('a'), hub(7 * DAY - 1)), Ok(epoch(1)));
    assert_eq!(
        held.holds(&repository('a'), hub(7 * DAY)),
        Err(NoLease::RanOut)
    );
}

#[test]
fn a_grant_already_expired_on_the_hubs_clock_holds_nothing() {
    let mut held = HeldLeases::default();
    let mut expired = grants(1, hub(0));
    expired.grants[0].expires_at = hub(-1);
    held.take(&expired, 0, hub(0));
    assert_eq!(held.holds(&repository('a'), hub(0)), Err(NoLease::RanOut));
}

#[test]
fn grants_that_no_longer_name_a_lease_let_it_go() {
    let mut held = HeldLeases::default();
    held.take(&grants(1, hub(0)), 0, hub(0));
    let none = RepoGrants {
        generation: 1,
        signed_at: hub(60),
        grants: vec![],
    };
    held.take(&none, 0, hub(60));
    assert_eq!(
        held.holds(&repository('a'), hub(61)),
        Err(NoLease::NoneHeld)
    );
}

#[test]
fn a_grant_below_the_floor_is_refused_and_ends_the_lease_held() {
    let mut held = HeldLeases::default();
    held.take(&grants(3, hub(0)), 0, hub(0));
    let refused = held.take(&grants(2, hub(60)), 0, hub(60));
    let floor = |number| EpochFloor {
        recovery_number: 0,
        generation: 1,
        epoch: epoch(number),
    };
    assert_eq!(
        refused,
        vec![EpochLower {
            repository: repository('a'),
            floor: floor(3),
            granted: floor(2),
        }]
    );
    assert_eq!(
        held.holds(&repository('a'), hub(61)),
        Err(NoLease::NoneHeld)
    );
    // The floor stays: the same grant again is refused again, and one at
    // the floor is taken.
    assert_eq!(held.take(&grants(2, hub(90)), 0, hub(90)).len(), 1);
    assert_eq!(held.take(&grants(3, hub(120)), 0, hub(120)), vec![]);
    assert_eq!(held.holds(&repository('a'), hub(121)), Ok(epoch(3)));
}

#[test]
fn the_floor_orders_recovery_number_then_generation_then_epoch() {
    let floor = |recovery_number, generation, number| EpochFloor {
        recovery_number,
        generation,
        epoch: epoch(number),
    };
    assert!(floor(0, 2, 1) > floor(0, 1, 9));
    assert!(floor(1, 1, 1) > floor(0, 9, 9));
    // A grant at a higher generation is taken over a higher epoch.
    let mut held = HeldLeases::default();
    held.take(&grants(5, hub(0)), 0, hub(0));
    let mut moved = grants(1, hub(60));
    moved.generation = 2;
    assert_eq!(held.take(&moved, 0, hub(60)), vec![]);
    assert_eq!(held.holds(&repository('a'), hub(61)), Ok(epoch(1)));
    // A lower generation is refused however high the epoch.
    assert_eq!(held.take(&grants(9, hub(90)), 0, hub(90)).len(), 1);
    // A higher recovery number is taken at any generation and epoch.
    let mut recovered = grants(1, hub(120));
    recovered.generation = 1;
    assert_eq!(held.take(&recovered, 1, hub(120)), vec![]);
    let floors: Vec<_> = held.floors().map(|(_, floor)| *floor).collect();
    assert_eq!(floors, vec![floor(1, 1, 1)]);
}

#[test]
fn a_release_lets_go_of_that_epoch_only() {
    let mut held = HeldLeases::default();
    held.take(&grants(2, hub(0)), 0, hub(0));
    held.released(&repository('a'), epoch(1));
    assert_eq!(held.holds(&repository('a'), hub(1)), Ok(epoch(2)));
    held.released(&repository('a'), epoch(2));
    assert_eq!(held.holds(&repository('a'), hub(1)), Err(NoLease::NoneHeld));
    assert_eq!(held.floors().count(), 1);
}

#[test]
fn what_a_cache_kept_reads_back() {
    let mut held = HeldLeases::default();
    held.take(&grants(2, hub(0)), 0, hub(0));
    let kept = HeldLeases::from_parts(
        held.leases()
            .map(|(repository, lease)| (repository.clone(), *lease)),
        held.floors()
            .map(|(repository, floor)| (repository.clone(), *floor)),
    );
    assert_eq!(kept, held);
}
