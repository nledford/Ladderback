use super::*;
use crate::SignedDuration;
use crate::fleet::{KEY_BYTES, SIGNATURE_BYTES};

/// A canonical signature standing for the hub's over the entry at `number`.
fn signature(number: u64) -> Signature {
    let mut bytes = [0; SIGNATURE_BYTES];
    bytes[..KEY_BYTES].fill(u8::try_from(number).unwrap());
    Signature::from_bytes(bytes).unwrap()
}

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
            signature: signature(number),
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
        held.proofs()
            .map(|(repository, proof)| (repository.clone(), *proof)),
    );
    assert_eq!(kept, held);
    assert_eq!(kept.proofs().count(), 1);
}

/// The floor `photos` reports at `number`, from `grants(number, hub(0))`.
fn reported(number: u64, held: bool) -> ReportedFloor {
    ReportedFloor {
        epoch: epoch(number),
        expires_at: hub(7 * DAY),
        signature: signature(number),
        held,
    }
}

#[test]
fn a_grant_is_kept_as_its_floors_proof() {
    let mut held = HeldLeases::default();
    held.take(&grants(2, hub(0)), 0, hub(0));
    assert_eq!(
        held.reported_floor(&repository('a'), hub(1)),
        Some(reported(2, true))
    );
}

#[test]
fn a_floor_whose_lease_ended_is_reported_as_not_held() {
    let mut held = HeldLeases::default();
    held.take(&grants(2, hub(0)), 0, hub(0));
    assert_eq!(
        held.reported_floor(&repository('a'), hub(7 * DAY)),
        Some(reported(2, false))
    );
    held.released(&repository('a'), epoch(2));
    assert_eq!(
        held.reported_floor(&repository('a'), hub(1)),
        Some(reported(2, false))
    );
}

#[test]
fn a_grant_below_the_floor_leaves_the_floors_proof() {
    let mut held = HeldLeases::default();
    held.take(&grants(3, hub(0)), 0, hub(0));
    held.take(&grants(2, hub(0)), 0, hub(0));
    assert_eq!(
        held.reported_floor(&repository('a'), hub(1)),
        Some(reported(3, false))
    );
}

#[test]
fn a_floor_kept_with_no_proof_is_not_reported() {
    let floor = EpochFloor {
        recovery_number: 0,
        generation: 1,
        epoch: epoch(2),
    };
    let held = HeldLeases::from_parts([], [(repository('a'), floor)], []);
    assert_eq!(held.reported_floor(&repository('a'), hub(1)), None);
    assert_eq!(held.reported_floor(&repository('b'), hub(1)), None);
}

/// A machine on the hub's time that took the grant of `photos` at hub(0).
fn granted() -> HeldLeases {
    let mut held = HeldLeases::default();
    assert_eq!(held.take(&grants(1, hub(0)), 0, hub(0)), vec![]);
    held
}

fn ms(by: i64) -> SignedDuration {
    SignedDuration::from_millis(by)
}

#[test]
fn maintenance_isnt_paused_while_the_lease_holds_and_is_paused_from_the_moment_it_runs_out() {
    let held = granted();
    let end = hub(7 * DAY);
    let before = end.checked_sub(ms(1)).unwrap();
    assert_eq!(held.paused_since(&repository('a'), before), None);
    assert_eq!(held.paused_since(&repository('a'), end), Some(end));
    assert_eq!(held.paused_since(&repository('a'), hub(9 * DAY)), Some(end));
}

#[test]
fn a_machine_that_holds_no_lease_and_noted_no_pause_says_none_until_it_watches() {
    let mut held = HeldLeases::default();
    assert_eq!(held.paused_since(&repository('a'), hub(0)), None);
    assert_eq!(held.watch([&repository('a')], hub(0)), vec![]);
    assert_eq!(held.paused_since(&repository('a'), hub(DAY)), Some(hub(0)));
}

#[test]
fn a_watch_notes_a_pause_since_the_lease_ran_out_and_keeps_its_date() {
    let mut held = granted();
    assert_eq!(held.watch([&repository('a')], hub(9 * DAY)), vec![]);
    assert_eq!(
        held.pauses().collect::<Vec<_>>(),
        vec![(
            &repository('a'),
            &Pause {
                since: hub(7 * DAY),
                alerted: false
            }
        )]
    );
    // The hub lets the lease go: the pause keeps the date it had.
    let none = RepoGrants {
        generation: 1,
        signed_at: hub(10 * DAY),
        grants: vec![],
    };
    assert_eq!(held.take(&none, 0, hub(10 * DAY)), vec![]);
    assert_eq!(held.watch([&repository('a')], hub(11 * DAY)), vec![]);
    assert_eq!(
        held.paused_since(&repository('a'), hub(11 * DAY)),
        Some(hub(7 * DAY))
    );
}

#[test]
fn a_watch_while_the_lease_holds_notes_no_pause() {
    let mut held = granted();
    assert_eq!(held.watch([&repository('a')], hub(DAY)), vec![]);
    assert_eq!(held.pauses().count(), 0);
}

#[test]
fn the_next_grant_clears_a_pause_and_the_next_watch_drops_it() {
    let mut held = granted();
    assert_eq!(held.watch([&repository('a')], hub(9 * DAY)), vec![]);
    assert_eq!(held.take(&grants(1, hub(9 * DAY)), 0, hub(9 * DAY)), vec![]);
    assert_eq!(held.paused_since(&repository('a'), hub(9 * DAY)), None);
    assert_eq!(held.watch([&repository('a')], hub(9 * DAY)), vec![]);
    assert_eq!(held.pauses().count(), 0);
    // Once the renewed lease runs out, the pause starts again from then.
    assert_eq!(held.watch([&repository('a')], hub(17 * DAY)), vec![]);
    assert_eq!(
        held.paused_since(&repository('a'), hub(17 * DAY)),
        Some(hub(16 * DAY))
    );
}

#[test]
fn a_watch_drops_the_pause_of_a_repository_the_machine_no_longer_asks_to_maintain() {
    let mut held = granted();
    assert_eq!(held.watch([&repository('a')], hub(9 * DAY)), vec![]);
    assert_eq!(held.watch([], hub(9 * DAY)), vec![]);
    assert_eq!(held.pauses().count(), 0);
    // What the lease says still stands until a watch notes a pause again.
    assert_eq!(
        held.paused_since(&repository('a'), hub(9 * DAY)),
        Some(hub(7 * DAY))
    );
}

#[test]
fn a_pause_is_returned_once_it_has_lasted_14_days_until_it_is_told() {
    let mut held = granted();
    let since = hub(7 * DAY);
    let due = since.checked_add(PAUSE_ALERT_AFTER).unwrap();
    assert_eq!(PAUSE_ALERT_AFTER, SignedDuration::from_hours(14 * 24));
    assert_eq!(
        held.watch([&repository('a')], due.checked_sub(ms(1)).unwrap()),
        vec![]
    );
    assert_eq!(
        held.watch([&repository('a')], due),
        vec![(repository('a'), since)]
    );
    // Not told yet, as when every channel failed: the next watch returns
    // it again.
    assert_eq!(
        held.watch([&repository('a')], due.checked_add(ms(1)).unwrap()),
        vec![(repository('a'), since)]
    );
    held.told(&repository('a'), hub(8 * DAY));
    assert_eq!(
        held.watch([&repository('a')], hub(25 * DAY)),
        vec![(repository('a'), since)]
    );
    held.told(&repository('a'), since);
    assert_eq!(
        held.pauses().collect::<Vec<_>>(),
        vec![(
            &repository('a'),
            &Pause {
                since,
                alerted: true
            }
        )]
    );
    assert_eq!(held.watch([&repository('a')], hub(30 * DAY)), vec![]);
}

#[test]
fn a_pause_first_noted_after_14_days_is_returned_only_by_the_next_watch() {
    let mut held = granted();
    assert_eq!(held.watch([&repository('a')], hub(30 * DAY)), vec![]);
    assert_eq!(
        held.watch([&repository('a')], hub(30 * DAY + 15 * 60)),
        vec![(repository('a'), hub(7 * DAY))]
    );
}

#[test]
fn each_repository_is_paused_and_told_on_its_own() {
    let mut held = granted();
    assert_eq!(
        held.watch([&repository('a'), &repository('b')], hub(14 * DAY)),
        vec![]
    );
    assert_eq!(
        held.watch([&repository('a'), &repository('b')], hub(21 * DAY)),
        vec![(repository('a'), hub(7 * DAY))]
    );
    held.told(&repository('a'), hub(7 * DAY));
    assert_eq!(
        held.watch([&repository('a'), &repository('b')], hub(28 * DAY)),
        vec![(repository('b'), hub(14 * DAY))]
    );
}

#[test]
fn kept_pauses_read_back_as_they_were() {
    let pause = Pause {
        since: hub(3),
        alerted: true,
    };
    let held = HeldLeases::default().with_pauses([(repository('a'), pause)]);
    assert_eq!(
        held.pauses().collect::<Vec<_>>(),
        vec![(&repository('a'), &pause)]
    );
    assert_eq!(held.paused_since(&repository('a'), hub(4)), Some(hub(3)));
}

#[test]
fn a_floors_proof_is_found_by_its_repository() {
    let mut held = HeldLeases::default();
    held.take(&grants(2, hub(0)), 0, hub(0));
    let (_, kept) = held.proofs().next().unwrap();
    assert_eq!(held.proof(&repository('a')), Some(kept));
    assert_eq!(held.proof(&repository('b')), None);
}

#[test]
fn turning_maintenance_on_without_a_lease_notes_a_pause_since_then() {
    let mut held = HeldLeases::default();
    held.turned(&repository('a'), true, hub(DAY));
    assert_eq!(
        held.paused_since(&repository('a'), hub(2 * DAY)),
        Some(hub(DAY))
    );
    // Turning it on again keeps the date first noted.
    held.turned(&repository('a'), true, hub(2 * DAY));
    assert_eq!(
        held.paused_since(&repository('a'), hub(2 * DAY)),
        Some(hub(DAY))
    );
    // A watch keeps that date too, and tells it once it has lasted 14 days.
    let due = hub(DAY).checked_add(PAUSE_ALERT_AFTER).unwrap();
    assert_eq!(
        held.watch([&repository('a')], due),
        vec![(repository('a'), hub(DAY))]
    );
}

#[test]
fn turning_maintenance_on_after_a_lease_ran_out_notes_a_pause_since_it_ran_out() {
    let mut held = granted();
    held.turned(&repository('a'), true, hub(9 * DAY));
    assert_eq!(held.pauses().count(), 1);
    assert_eq!(
        held.paused_since(&repository('a'), hub(9 * DAY)),
        Some(hub(7 * DAY))
    );
}

#[test]
fn turning_maintenance_on_while_the_lease_holds_notes_no_pause() {
    let mut held = granted();
    held.turned(&repository('a'), true, hub(DAY));
    assert_eq!(held.pauses().count(), 0);
}

#[test]
fn turning_maintenance_off_drops_the_pause() {
    let mut held = HeldLeases::default();
    held.turned(&repository('a'), true, hub(DAY));
    held.turned(&repository('b'), true, hub(DAY));
    held.turned(&repository('a'), false, hub(2 * DAY));
    let left: Vec<&RepositoryId> = held.pauses().map(|(repository, _)| repository).collect();
    assert_eq!(left, vec![&repository('b')]);
}
