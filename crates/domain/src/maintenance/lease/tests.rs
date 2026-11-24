use super::*;
use crate::Uuid;

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

fn plus(time: Timestamp, by: SignedDuration) -> Timestamp {
    time.checked_add(by).unwrap()
}

fn report(digit: char, request: Maintenance) -> RepositoryReport {
    RepositoryReport {
        repository: repository(digit),
        request,
        holds_other_agents_snapshots: false,
    }
}

/// `holder`'s lease at `epoch`, granted at `granted`.
fn held(holder: u128, number: u64, granted: Timestamp) -> RepositoryOwnership {
    RepositoryOwnership {
        repository: repository('a'),
        last_epoch: Some(epoch(number)),
        lease: Some(Lease {
            holder: agent(holder),
            epoch: epoch(number),
            expires_at: plus(granted, LEASE_LIFE),
        }),
    }
}

#[test]
fn epochs_start_at_1_and_only_rise() {
    assert_eq!(LeaseEpoch::new(0), None);
    assert_eq!(LeaseEpoch::new(1), Some(LeaseEpoch::FIRST));
    assert_eq!(epoch(7).get(), 7);
    assert_eq!(epoch(7).next(), Some(epoch(8)));
    assert_eq!(epoch(u64::MAX).next(), None);
}

#[test]
fn a_lease_binds_until_the_grace_past_its_expiry() {
    let lease = Lease {
        holder: agent(1),
        epoch: LeaseEpoch::FIRST,
        expires_at: at(0),
    };
    let end = plus(at(0), LEASE_GRACE);
    assert!(lease.binds(at(-1)));
    assert!(lease.binds(at(0)));
    assert!(lease.binds(plus(end, SignedDuration::from_nanos(-1))));
    assert!(!lease.binds(end));
    let last = Lease {
        expires_at: Timestamp::MAX,
        ..lease
    };
    assert!(
        last.binds(Timestamp::MAX),
        "a grace past the last time saturates"
    );
}

#[test]
fn a_report_counts_for_its_life() {
    let end = plus(at(0), REPORT_LIFE);
    assert!(report_counts(at(0), at(0)));
    assert!(report_counts(
        at(0),
        plus(end, SignedDuration::from_nanos(-1))
    ));
    assert!(!report_counts(at(0), end));
    assert!(report_counts(Timestamp::MAX, Timestamp::MAX));
}

#[test]
fn a_repository_is_shared_when_another_reports_it_or_holds_its_snapshots() {
    let alone = report('a', Maintenance::Local);
    assert!(!alone.shared(false));
    assert!(alone.shared(true));
    let tagged = RepositoryReport {
        holds_other_agents_snapshots: true,
        ..alone
    };
    assert!(tagged.shared(false));
}

#[test]
fn a_report_keeps_one_entry_per_repository_the_last_named() {
    let reports = RepositoryReports::new([
        report('b', Maintenance::Local),
        report('a', Maintenance::Hub),
        report('b', Maintenance::None),
    ]);
    assert_eq!(
        reports.entries(),
        [
            report('a', Maintenance::Hub),
            report('b', Maintenance::None)
        ]
    );
}

#[test]
fn the_first_claim_is_granted_a_lease_of_7_days_at_epoch_1() {
    let mut ownership = RepositoryOwnership::new(repository('a'));
    let lease = Lease {
        holder: agent(1),
        epoch: LeaseEpoch::FIRST,
        expires_at: plus(at(0), SignedDuration::from_hours(7 * 24)),
    };
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, at(0)),
        ClaimOutcome::Granted(lease)
    );
    assert_eq!(ownership.lease, Some(lease));
    assert_eq!(ownership.last_epoch, Some(LeaseEpoch::FIRST));
    assert_eq!(ownership.binding(at(0)), Some(&lease));
}

#[test]
fn a_lease_granted_near_the_last_time_runs_to_it_and_binds() {
    let near_end = plus(Timestamp::MAX, SignedDuration::from_hours(-24));
    let mut ownership = RepositoryOwnership::new(repository('a'));
    let lease = Lease {
        holder: agent(1),
        epoch: LeaseEpoch::FIRST,
        expires_at: Timestamp::MAX,
    };
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, near_end),
        ClaimOutcome::Granted(lease)
    );
    assert_eq!(
        ownership.claim(agent(2), Maintenance::Local, false, near_end),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1)))
    );
}

/// I1's test (technical design §0): two agents request `local` for one
/// repository, and the second is rejected.
#[test]
fn i1_two_agents_request_local_and_the_second_is_rejected() {
    let mut ownership = RepositoryOwnership::new(repository('a'));
    let granted = ownership.claim(agent(1), Maintenance::Local, false, at(0));
    assert!(matches!(granted, ClaimOutcome::Granted(_)), "{granted:?}");
    let before = ownership.clone();
    assert_eq!(
        ownership.claim(agent(2), Maintenance::Local, false, at(1)),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1)))
    );
    assert_eq!(ownership, before);
}

#[test]
fn the_holders_claim_renews_its_lease_at_the_same_epoch() {
    let mut ownership = held(1, 3, at(0));
    let renewed = Lease {
        holder: agent(1),
        epoch: epoch(3),
        expires_at: plus(at(100), LEASE_LIFE),
    };
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, at(100)),
        ClaimOutcome::Renewed(renewed)
    );
    assert_eq!(ownership.lease, Some(renewed));
    assert_eq!(ownership.last_epoch, Some(epoch(3)));
}

#[test]
fn a_holder_is_renewed_in_the_grace_past_its_expiry() {
    let mut ownership = held(1, 3, at(0));
    let late = plus(plus(at(0), LEASE_LIFE), SignedDuration::from_mins(30));
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, late),
        ClaimOutcome::Renewed(Lease {
            holder: agent(1),
            epoch: epoch(3),
            expires_at: plus(late, LEASE_LIFE),
        })
    );
}

#[test]
fn a_renewal_never_moves_an_expiry_earlier() {
    let mut ownership = held(1, 3, at(0));
    let before = ownership.lease;
    let set_back = at(-86_400);
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, set_back),
        ClaimOutcome::Renewed(before.unwrap())
    );
    assert_eq!(ownership.lease, before);
}

#[test]
fn another_agent_is_refused_until_the_grace_past_the_expiry() {
    let mut ownership = held(1, 3, at(0));
    let end = plus(plus(at(0), LEASE_LIFE), LEASE_GRACE);
    let just_before = plus(end, SignedDuration::from_nanos(-1));
    assert_eq!(
        ownership.claim(agent(2), Maintenance::Local, false, just_before),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1)))
    );
    assert_eq!(ownership.binding(end), None);
    let granted = Lease {
        holder: agent(2),
        epoch: epoch(4),
        expires_at: plus(end, LEASE_LIFE),
    };
    assert_eq!(
        ownership.claim(agent(2), Maintenance::Local, false, end),
        ClaimOutcome::Granted(granted)
    );
    assert_eq!(ownership.last_epoch, Some(epoch(4)));
}

#[test]
fn a_holder_whose_lease_lapsed_is_granted_a_new_epoch() {
    let mut ownership = held(1, 3, at(0));
    let end = plus(plus(at(0), LEASE_LIFE), LEASE_GRACE);
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, end),
        ClaimOutcome::Granted(Lease {
            holder: agent(1),
            epoch: epoch(4),
            expires_at: plus(end, LEASE_LIFE),
        })
    );
}

#[test]
fn a_shared_repository_is_granted_to_nobody_and_its_lease_is_not_renewed() {
    let mut free = RepositoryOwnership::new(repository('a'));
    assert_eq!(
        free.claim(agent(1), Maintenance::Local, true, at(0)),
        ClaimOutcome::Refused(ClaimRefused::Shared)
    );
    assert_eq!(free, RepositoryOwnership::new(repository('a')));

    let mut ownership = held(1, 3, at(0));
    let before = ownership.clone();
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, true, at(100)),
        ClaimOutcome::Unrenewed(before.lease.unwrap())
    );
    assert_eq!(ownership, before);
    assert_eq!(
        ownership.claim(agent(2), Maintenance::Local, true, at(100)),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1)))
    );
}

#[test]
fn a_request_for_the_hub_or_nobody_claims_nothing() {
    for request in [Maintenance::Hub, Maintenance::None] {
        let mut free = RepositoryOwnership::new(repository('a'));
        assert_eq!(
            free.claim(agent(1), request, false, at(0)),
            ClaimOutcome::NoClaim
        );
        assert_eq!(free, RepositoryOwnership::new(repository('a')));
        let mut ownership = held(1, 3, at(0));
        let before = ownership.clone();
        assert_eq!(
            ownership.claim(agent(1), request, false, at(100)),
            ClaimOutcome::NoClaim
        );
        assert_eq!(
            ownership, before,
            "a holder asking {request:?} isn't renewed"
        );
    }
}

#[test]
fn no_lease_is_granted_once_the_epochs_are_spent() {
    let mut ownership = RepositoryOwnership {
        repository: repository('a'),
        last_epoch: Some(epoch(u64::MAX)),
        lease: None,
    };
    let before = ownership.clone();
    assert_eq!(
        ownership.claim(agent(1), Maintenance::Local, false, at(0)),
        ClaimOutcome::Refused(ClaimRefused::EpochsSpent)
    );
    assert_eq!(ownership, before);
}

#[test]
fn the_holders_release_ends_its_lease_and_the_next_grant_is_a_later_epoch() {
    let mut ownership = held(1, 3, at(0));
    assert_eq!(ownership.release(agent(1), epoch(3)), Ok(()));
    assert_eq!(ownership.lease, None);
    assert_eq!(ownership.binding(at(1)), None);
    assert_eq!(ownership.last_epoch, Some(epoch(3)));
    let granted = ownership.claim(agent(2), Maintenance::Local, false, at(1));
    assert_eq!(
        granted,
        ClaimOutcome::Granted(Lease {
            holder: agent(2),
            epoch: epoch(4),
            expires_at: plus(at(1), LEASE_LIFE),
        })
    );
}

#[test]
fn a_release_by_another_agent_or_of_another_epoch_changes_nothing() {
    let mut ownership = held(1, 3, at(0));
    let before = ownership.clone();
    assert_eq!(ownership.release(agent(2), epoch(3)), Err(ReleaseRefused));
    assert_eq!(ownership.release(agent(1), epoch(2)), Err(ReleaseRefused));
    assert_eq!(ownership.release(agent(1), epoch(4)), Err(ReleaseRefused));
    assert_eq!(ownership, before);
    let mut free = RepositoryOwnership::new(repository('a'));
    assert_eq!(
        free.release(agent(1), LeaseEpoch::FIRST),
        Err(ReleaseRefused)
    );
}

/// A start window that holds repository `a`, used by agent 9, who hasn't
/// reported since the start.
fn holding_a() -> StartWindow {
    StartWindow::new([(agent(9), repository('a'))])
}

#[test]
fn the_start_window_grants_nobody_a_repository_with_no_lease_binding() {
    let mut free = RepositoryOwnership::new(repository('a'));
    assert_eq!(
        free.claim_after_start(&holding_a(), agent(1), Maintenance::Local, false, at(0)),
        ClaimOutcome::Refused(ClaimRefused::Starting)
    );
    assert_eq!(free, RepositoryOwnership::new(repository('a')));
    assert_eq!(
        free.claim_after_start(&holding_a(), agent(1), Maintenance::Hub, false, at(0)),
        ClaimOutcome::NoClaim,
        "a report that claims nothing is refused nothing"
    );

    let end = plus(plus(at(0), LEASE_LIFE), LEASE_GRACE);
    let mut lapsed = held(1, 3, at(0));
    let before = lapsed.clone();
    for claimant in [1, 2] {
        assert_eq!(
            lapsed.claim_after_start(
                &holding_a(),
                agent(claimant),
                Maintenance::Local,
                false,
                end
            ),
            ClaimOutcome::Refused(ClaimRefused::Starting),
            "agent {claimant} is granted no lease that ran out"
        );
    }
    assert_eq!(lapsed, before);
}

#[test]
fn the_start_window_renews_the_holders_lease_and_refuses_another() {
    let mut ownership = held(1, 3, at(0));
    assert_eq!(
        ownership.claim_after_start(&holding_a(), agent(1), Maintenance::Local, false, at(100)),
        ClaimOutcome::Renewed(Lease {
            holder: agent(1),
            epoch: epoch(3),
            expires_at: plus(at(100), LEASE_LIFE),
        })
    );
    assert_eq!(
        ownership.claim_after_start(&holding_a(), agent(2), Maintenance::Local, false, at(100)),
        ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1)))
    );
    assert_eq!(
        ownership.claim_after_start(&holding_a(), agent(2), Maintenance::None, false, at(100)),
        ClaimOutcome::NoClaim
    );
}

#[test]
fn a_repository_the_start_window_doesnt_hold_is_decided_as_ever() {
    let mut window = StartWindow::new([(agent(9), repository('a')), (agent(8), repository('b'))]);
    window.heard(agent(8));
    assert!(window.holds(&repository('a')));
    let mut ownership = RepositoryOwnership::new(repository('b'));
    assert_eq!(
        ownership.claim_after_start(&window, agent(1), Maintenance::Local, false, at(0)),
        ClaimOutcome::Granted(Lease {
            holder: agent(1),
            epoch: LeaseEpoch::FIRST,
            expires_at: plus(at(0), LEASE_LIFE),
        })
    );
}
