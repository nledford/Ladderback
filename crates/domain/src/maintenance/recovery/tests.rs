use super::*;
use crate::Uuid;
use crate::maintenance::Lease;

fn agent(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn repository(digit: char) -> RepositoryId {
    RepositoryId::parse(&digit.to_string().repeat(64)).unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).unwrap()
}

/// The hub's record of `photos` with `last` epochs given and no lease.
fn counted(last: Option<u64>) -> RepositoryOwnership {
    RepositoryOwnership {
        repository: repository('a'),
        last_epoch: last.map(epoch),
        lease: None,
    }
}

/// `holder`'s proven floor on `photos` at `number`, running out at 600.
fn floor(holder: u128, number: u64, held: bool) -> ProvenFloor {
    ProvenFloor {
        repository: repository('a'),
        holder: agent(holder),
        epoch: epoch(number),
        expires_at: at(600),
        held,
    }
}

#[test]
fn a_hub_with_no_floor_is_not_in_recovery_mode() {
    assert!(!HubRecovery::default().is_on());
}

#[test]
fn a_floor_above_the_counter_puts_the_hub_in_recovery_mode() {
    let mut recovery = HubRecovery::default();
    assert!(recovery.hear(&counted(Some(2)), &floor(1, 3, true)));
    assert!(recovery.is_on());
    assert_eq!(recovery.floors().collect::<Vec<_>>(), [&floor(1, 3, true)]);
}

#[test]
fn a_floor_on_a_repository_the_hub_never_counted_is_above_it() {
    let mut recovery = HubRecovery::default();
    assert!(recovery.hear(&counted(None), &floor(1, 1, false)));
    assert!(recovery.is_on());
}

#[test]
fn a_floor_at_or_below_the_counter_changes_nothing() {
    let mut recovery = HubRecovery::default();
    assert!(!recovery.hear(&counted(Some(3)), &floor(1, 3, true)));
    assert!(!recovery.hear(&counted(Some(3)), &floor(1, 2, false)));
    assert!(!recovery.is_on());
}

#[test]
fn in_recovery_mode_every_proven_floor_is_kept() {
    let mut recovery = HubRecovery::default();
    recovery.hear(&counted(None), &floor(1, 1, true));
    let other = ProvenFloor {
        repository: repository('b'),
        ..floor(2, 4, false)
    };
    let ownership = RepositoryOwnership {
        repository: repository('b'),
        ..counted(Some(9))
    };
    assert!(recovery.hear(&ownership, &other));
    assert_eq!(recovery.floors().count(), 2);
}

#[test]
fn the_highest_floor_of_a_repository_is_kept() {
    let mut recovery = HubRecovery::default();
    recovery.hear(&counted(None), &floor(1, 3, true));
    assert!(!recovery.hear(&counted(None), &floor(2, 2, true)));
    assert!(recovery.hear(&counted(None), &floor(2, 4, false)));
    assert_eq!(recovery.floors().collect::<Vec<_>>(), [&floor(2, 4, false)]);
}

#[test]
fn a_holders_newer_report_at_the_same_epoch_replaces_its_own() {
    let mut recovery = HubRecovery::default();
    recovery.hear(&counted(None), &floor(1, 3, true));
    assert!(recovery.hear(&counted(None), &floor(1, 3, false)));
    assert!(!recovery.hear(&counted(None), &floor(2, 3, true)));
    assert_eq!(recovery.floors().collect::<Vec<_>>(), [&floor(1, 3, false)]);
}

#[test]
fn in_recovery_mode_a_claim_is_refused_and_nothing_else_is_claimed() {
    let recovery = HubRecovery::from_floors([floor(1, 3, true)]);
    assert_eq!(
        recovery.decide(Maintenance::Local),
        ClaimOutcome::Refused(ClaimRefused::Recovering)
    );
    assert_eq!(recovery.decide(Maintenance::Hub), ClaimOutcome::NoClaim);
    assert_eq!(recovery.decide(Maintenance::None), ClaimOutcome::NoClaim);
}

#[test]
fn whether_a_floor_is_above_the_counter() {
    assert!(counted(None).is_behind(&floor(1, 1, false)));
    assert!(counted(Some(2)).is_behind(&floor(1, 3, false)));
    assert!(!counted(Some(3)).is_behind(&floor(1, 3, false)));
    assert!(!counted(Some(4)).is_behind(&floor(1, 3, false)));
}

#[test]
fn recovering_gives_a_held_lease_back_to_its_holder() {
    let mut ownership = counted(Some(1));
    ownership.lease = Some(Lease {
        holder: agent(2),
        epoch: epoch(1),
        expires_at: at(900),
    });
    ownership.recover(&floor(1, 3, true));
    assert_eq!(ownership.last_epoch, Some(epoch(3)));
    assert_eq!(
        ownership.lease,
        Some(Lease {
            holder: agent(1),
            epoch: epoch(3),
            expires_at: at(600),
        })
    );
}

#[test]
fn recovering_from_a_released_floor_drops_the_lease_below_it() {
    let mut ownership = counted(Some(1));
    ownership.lease = Some(Lease {
        holder: agent(2),
        epoch: epoch(1),
        expires_at: at(900),
    });
    ownership.recover(&floor(1, 3, false));
    assert_eq!(ownership.last_epoch, Some(epoch(3)));
    assert_eq!(ownership.lease, None);
}

#[test]
fn recovering_keeps_the_hubs_own_lease_at_the_floor() {
    let lease = Lease {
        holder: agent(1),
        epoch: epoch(3),
        expires_at: at(900),
    };
    let mut ownership = counted(Some(3));
    ownership.lease = Some(lease);
    ownership.recover(&floor(1, 3, true));
    assert_eq!(ownership.lease, Some(lease));
}

#[test]
fn recovering_leaves_a_counter_past_the_floor() {
    let lease = Lease {
        holder: agent(2),
        epoch: epoch(5),
        expires_at: at(900),
    };
    let mut ownership = counted(Some(5));
    ownership.lease = Some(lease);
    ownership.recover(&floor(1, 3, true));
    assert_eq!(ownership.last_epoch, Some(epoch(5)));
    assert_eq!(ownership.lease, Some(lease));
}

#[test]
fn the_floors_a_store_kept_are_held_again() {
    let recovery = HubRecovery::from_floors([floor(1, 3, true)]);
    assert!(recovery.is_on());
    assert_eq!(recovery.floors().collect::<Vec<_>>(), [&floor(1, 3, true)]);
}
