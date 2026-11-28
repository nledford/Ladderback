//! The `OwnershipRegistry` contract (technical design §7.3, I1): each
//! repository's lease decided on the reports the hub keeps, one claim of
//! any that race granted, an agent's report replacing its last and counting
//! for its life only, a release ending the lease, the epochs rising across
//! releases and restarts of the hub, and the start window keeping a
//! repository from a new owner.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use domain::maintenance::{
    ClaimOutcome, ClaimRefused, LEASE_GRACE, LEASE_LIFE, Lease, LeaseEpoch, ProvenFloor,
    REPORT_LIFE, ReleaseRefused, RepositoryOwnership, RepositoryReport, RepositoryReports,
    StartWindow,
};
use domain::planning::Maintenance;
use domain::{AgentId, RepositoryId, SignedDuration, Timestamp, Uuid};

use crate::ports::{ClaimDecision, OwnershipRegistry};

/// Opens a registry: a new, empty one, or the one before opened again on
/// the same place.
#[async_trait]
pub trait OwnershipRegistryHarness: Send + Sync {
    /// A registry that holds nothing yet.
    async fn empty(&self) -> Arc<dyn OwnershipRegistry>;
    /// The registry [`empty`](Self::empty) last gave, opened again, as
    /// after a restart of the hub.
    async fn reopened(&self) -> Arc<dyn OwnershipRegistry>;
}

pub async fn run(harness: &dyn OwnershipRegistryHarness) {
    a_repository_nobody_reported_has_no_owner(harness).await;
    the_first_claim_is_granted_and_kept(harness).await;
    i1_the_second_claim_is_refused(harness).await;
    i1_of_claims_that_race_one_is_granted(harness).await;
    a_report_decides_every_repository_it_names(harness).await;
    another_agents_report_stops_the_renewal(harness).await;
    a_report_replaces_the_agents_last(harness).await;
    a_renewal_is_kept(harness).await;
    a_report_stops_counting_after_its_life(harness).await;
    a_report_of_another_agents_snapshots_is_refused(harness).await;
    a_release_ends_the_lease_and_epochs_keep_rising(harness).await;
    leases_and_reports_outlive_the_hub(harness).await;
    the_reported_uses_are_each_agents_last_report(harness).await;
    the_start_window_grants_no_new_owner(harness).await;
    a_floor_above_the_counter_starts_recovery_mode(harness).await;
    a_floor_at_the_counter_changes_nothing(harness).await;
    ending_recovery_mode_raises_the_counter_to_the_floor(harness).await;
}

/// The value of `result`, after a failed assertion if it is an error.
fn ok<T, E: fmt::Debug>(result: Result<T, E>, what: &str) -> Option<T> {
    let error = result.as_ref().err();
    assert!(error.is_none(), "{what}: {error:?}");
    result.ok()
}

fn agent(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

/// The machines the suite reports as. A harness whose registry keeps the
/// reports of registered machines only registers these in `empty`.
pub fn machines() -> impl Iterator<Item = AgentId> {
    (1..=8).map(agent)
}

fn repository(digit: char) -> Option<RepositoryId> {
    ok(
        RepositoryId::parse(&digit.to_string().repeat(64)),
        "a repository ID",
    )
}

/// A whole second, so a store that keeps milliseconds keeps it exactly.
fn at(seconds: i64) -> Option<Timestamp> {
    ok(
        Timestamp::from_second(1_800_000_000 + seconds),
        "a test time",
    )
}

fn after(time: Timestamp, by: SignedDuration) -> Option<Timestamp> {
    ok(time.checked_add(by), "a later time")
}

fn entry(digit: char, request: Maintenance) -> Option<RepositoryReport> {
    Some(RepositoryReport {
        repository: repository(digit)?,
        request,
        holds_other_agents_snapshots: false,
        floor: None,
    })
}

/// A report of `entries`.
fn reports(entries: &[Option<RepositoryReport>]) -> Option<RepositoryReports> {
    entries
        .iter()
        .cloned()
        .collect::<Option<Vec<_>>>()
        .map(RepositoryReports::new)
}

/// What `registry` decided of `agent`'s report of `entries` at `now`, with
/// the start window holding nothing.
async fn report(
    registry: &dyn OwnershipRegistry,
    agent: AgentId,
    entries: &[Option<RepositoryReport>],
    now: Option<Timestamp>,
) -> Option<Vec<ClaimDecision>> {
    report_in(registry, &StartWindow::default(), agent, entries, now).await
}

/// What `registry` decided of `agent`'s report of `entries` at `now`, in
/// `window`.
async fn report_in(
    registry: &dyn OwnershipRegistry,
    window: &StartWindow,
    agent: AgentId,
    entries: &[Option<RepositoryReport>],
    now: Option<Timestamp>,
) -> Option<Vec<ClaimDecision>> {
    let (Some(reports), Some(now)) = (reports(entries), now) else {
        return None;
    };
    ok(
        registry.report(agent, &reports, &[], now, window).await,
        "a report",
    )
}

/// The one outcome of `agent`'s claim of the repository `digit` at `now`.
async fn claim(
    registry: &dyn OwnershipRegistry,
    agent: AgentId,
    digit: char,
    now: Option<Timestamp>,
) -> Option<ClaimOutcome> {
    let decided = report(registry, agent, &[entry(digit, Maintenance::Local)], now).await?;
    assert_eq!(decided.len(), 1, "one repository reported: {decided:?}");
    decided.into_iter().next().map(|decision| decision.outcome)
}

async fn ownership(registry: &dyn OwnershipRegistry, digit: char) -> Option<RepositoryOwnership> {
    ok(
        registry.ownership(&repository(digit)?).await,
        "an ownership",
    )
}

fn epoch(number: u64) -> Option<LeaseEpoch> {
    LeaseEpoch::new(number)
}

/// `holder`'s lease at `number`, expiring [`LEASE_LIFE`] after `from`.
fn lease(holder: AgentId, number: u64, from: Option<Timestamp>) -> Option<Lease> {
    Some(Lease {
        holder,
        epoch: epoch(number)?,
        expires_at: after(from?, LEASE_LIFE)?,
    })
}

async fn a_repository_nobody_reported_has_no_owner(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let Some(id) = repository('a') else { return };
    assert_eq!(
        ownership(registry.as_ref(), 'a').await,
        Some(RepositoryOwnership::new(id))
    );
}

async fn the_first_claim_is_granted_and_kept(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let granted = lease(agent(1), 1, at(0));
    assert_eq!(
        claim(registry.as_ref(), agent(1), 'a', at(0)).await,
        granted.map(ClaimOutcome::Granted)
    );
    let kept = ownership(registry.as_ref(), 'a').await;
    assert_eq!(kept.as_ref().and_then(|kept| kept.lease), granted);
    assert_eq!(kept.and_then(|kept| kept.last_epoch), epoch(1));
}

/// I1's test (technical design §0): two agents request `local` for one
/// repository, and the second is rejected.
async fn i1_the_second_claim_is_refused(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    assert_eq!(
        claim(registry.as_ref(), agent(2), 'a', at(1)).await,
        Some(ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1))))
    );
    assert_eq!(
        ownership(registry.as_ref(), 'a')
            .await
            .and_then(|kept| kept.lease),
        lease(agent(1), 1, at(0))
    );
}

/// Agents that claim one repository at once: one is granted, and every
/// other is refused because that one holds it.
async fn i1_of_claims_that_race_one_is_granted(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let claims = (1..=8).map(|n| {
        let registry = Arc::clone(&registry);
        async move { (n, claim(registry.as_ref(), agent(n), 'a', at(0)).await) }
    });
    let outcomes = futures::future::join_all(claims).await;
    let granted: Vec<u128> = outcomes
        .iter()
        .filter(|(_, outcome)| matches!(outcome, Some(ClaimOutcome::Granted(_))))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(granted.len(), 1, "exactly one claim granted: {outcomes:?}");
    let Some(winner) = granted.first().copied().map(agent) else {
        return;
    };
    for (n, outcome) in &outcomes {
        if agent(*n) != winner {
            assert_eq!(
                *outcome,
                Some(ClaimOutcome::Refused(ClaimRefused::HeldByAnother(winner))),
                "agent {n}"
            );
        }
    }
    assert_eq!(
        ownership(registry.as_ref(), 'a')
            .await
            .and_then(|kept| kept.lease),
        lease(winner, 1, at(0))
    );
}

async fn a_report_decides_every_repository_it_names(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let decided = report(
        registry.as_ref(),
        agent(1),
        &[
            entry('b', Maintenance::Hub),
            entry('a', Maintenance::Local),
            entry('c', Maintenance::None),
        ],
        at(0),
    )
    .await;
    let expected = (|| {
        Some(vec![
            ClaimDecision {
                repository: repository('a')?,
                outcome: ClaimOutcome::Granted(lease(agent(1), 1, at(0))?),
            },
            ClaimDecision {
                repository: repository('b')?,
                outcome: ClaimOutcome::NoClaim,
            },
            ClaimDecision {
                repository: repository('c')?,
                outcome: ClaimOutcome::NoClaim,
            },
        ])
    })();
    assert_eq!(decided, expected);
}

/// The holder's lease isn't renewed once another agent reports the
/// repository, whatever that agent asks for.
async fn another_agents_report_stops_the_renewal(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    report(
        registry.as_ref(),
        agent(2),
        &[entry('a', Maintenance::None)],
        at(10),
    )
    .await;
    assert_eq!(
        claim(registry.as_ref(), agent(1), 'a', at(20)).await,
        lease(agent(1), 1, at(0)).map(ClaimOutcome::Unrenewed)
    );
}

async fn a_report_replaces_the_agents_last(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    report(
        registry.as_ref(),
        agent(2),
        &[entry('a', Maintenance::None)],
        at(10),
    )
    .await;
    report(registry.as_ref(), agent(2), &[], at(20)).await;
    assert_eq!(
        claim(registry.as_ref(), agent(1), 'a', at(30)).await,
        lease(agent(1), 1, at(30)).map(ClaimOutcome::Renewed)
    );
}

async fn a_renewal_is_kept(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    claim(registry.as_ref(), agent(1), 'a', at(100)).await;
    let renewed = lease(agent(1), 1, at(100));
    assert_eq!(
        ownership(registry.as_ref(), 'a')
            .await
            .and_then(|kept| kept.lease),
        renewed
    );
    let reopened = harness.reopened().await;
    assert_eq!(
        ownership(reopened.as_ref(), 'a')
            .await
            .and_then(|kept| kept.lease),
        renewed,
        "the renewal outlives the hub"
    );
    let first_lease_ends = at(0)
        .and_then(|start| after(start, LEASE_LIFE))
        .and_then(|expiry| after(expiry, LEASE_GRACE));
    assert_eq!(
        claim(reopened.as_ref(), agent(2), 'a', first_lease_ends).await,
        Some(ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1)))),
        "the renewed lease still binds once the first would have ended"
    );
}

async fn a_report_stops_counting_after_its_life(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    report(
        registry.as_ref(),
        agent(2),
        &[entry('a', Maintenance::None)],
        at(0),
    )
    .await;
    let Some(late) = at(0).and_then(|start| after(start, REPORT_LIFE)) else {
        return;
    };
    let Some(just_before) = after(late, SignedDuration::from_millis(-1)) else {
        return;
    };
    assert_eq!(
        claim(registry.as_ref(), agent(1), 'a', Some(just_before)).await,
        Some(ClaimOutcome::Refused(ClaimRefused::Shared))
    );
    assert_eq!(
        claim(registry.as_ref(), agent(1), 'a', Some(late)).await,
        lease(agent(1), 1, Some(late)).map(ClaimOutcome::Granted)
    );
}

async fn a_report_of_another_agents_snapshots_is_refused(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let tagged = entry('a', Maintenance::Local).map(|entry| RepositoryReport {
        holds_other_agents_snapshots: true,
        ..entry
    });
    let decided = report(registry.as_ref(), agent(1), &[tagged], at(0)).await;
    assert_eq!(
        decided.map(|decided| decided.into_iter().map(|d| d.outcome).collect::<Vec<_>>()),
        Some(vec![ClaimOutcome::Refused(ClaimRefused::Shared)])
    );
}

async fn a_release_ends_the_lease_and_epochs_keep_rising(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let (Some(id), Some(first)) = (repository('a'), epoch(1)) else {
        return;
    };
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    assert_eq!(
        ok(registry.release(agent(2), &id, first).await, "a release"),
        Some(Err(ReleaseRefused))
    );
    report(registry.as_ref(), agent(1), &[], at(10)).await;
    assert_eq!(
        claim(registry.as_ref(), agent(2), 'a', at(20)).await,
        Some(ClaimOutcome::Refused(ClaimRefused::HeldByAnother(agent(1))))
    );
    assert_eq!(
        ok(registry.release(agent(1), &id, first).await, "a release"),
        Some(Ok(()))
    );
    assert_eq!(
        claim(registry.as_ref(), agent(2), 'a', at(30)).await,
        lease(agent(2), 2, at(30)).map(ClaimOutcome::Granted)
    );
}

async fn leases_and_reports_outlive_the_hub(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    report(
        registry.as_ref(),
        agent(2),
        &[entry('b', Maintenance::Hub)],
        at(10),
    )
    .await;
    let (Some(id), Some(first)) = (repository('a'), epoch(1)) else {
        return;
    };
    report(registry.as_ref(), agent(1), &[], at(10)).await;
    ok(registry.release(agent(1), &id, first).await, "a release");
    let kept = ownership(registry.as_ref(), 'a').await;

    let reopened = harness.reopened().await;
    assert_eq!(ownership(reopened.as_ref(), 'a').await, kept);
    assert_eq!(
        claim(reopened.as_ref(), agent(3), 'a', at(20)).await,
        lease(agent(3), 2, at(20)).map(ClaimOutcome::Granted),
        "the epochs given before the restart stay given"
    );
    assert_eq!(
        claim(reopened.as_ref(), agent(3), 'b', at(30)).await,
        Some(ClaimOutcome::Refused(ClaimRefused::Shared)),
        "the reports taken before the restart still count"
    );
}

/// The sorted uses `registry` lists.
async fn uses(registry: &dyn OwnershipRegistry) -> Option<Vec<(AgentId, RepositoryId)>> {
    let mut uses = ok(registry.reported_uses().await, "the reported uses")?;
    uses.sort();
    Some(uses)
}

async fn the_reported_uses_are_each_agents_last_report(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    assert_eq!(uses(registry.as_ref()).await, Some(Vec::new()));
    let (Some(a), Some(b)) = (repository('a'), repository('b')) else {
        return;
    };
    let both = [
        entry('a', Maintenance::Local),
        entry('b', Maintenance::None),
    ];
    report(registry.as_ref(), agent(1), &both, at(0)).await;
    report(
        registry.as_ref(),
        agent(2),
        &[entry('a', Maintenance::Hub)],
        at(0),
    )
    .await;
    report(
        registry.as_ref(),
        agent(1),
        &[entry('b', Maintenance::Hub)],
        at(10),
    )
    .await;
    let expected = Some(vec![(agent(1), b), (agent(2), a)]);
    assert_eq!(
        uses(registry.as_ref()).await,
        expected,
        "a report replaces the agent's last"
    );
    let later = at(0).and_then(|start| after(start, REPORT_LIFE + REPORT_LIFE));
    report(registry.as_ref(), agent(3), &[], later).await;
    let reopened = harness.reopened().await;
    assert_eq!(
        uses(reopened.as_ref()).await,
        expected,
        "a report that no longer counts is still listed, and outlives the hub"
    );
}

async fn the_start_window_grants_no_new_owner(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let (Some(a), Some(b)) = (repository('a'), repository('b')) else {
        return;
    };
    let window = StartWindow::new([(agent(9), a), (agent(9), b)], []);
    let claim_in = |agent, digit, now| {
        let registry = registry.clone();
        let window = window.clone();
        async move {
            let decided = report_in(
                registry.as_ref(),
                &window,
                agent,
                &[entry(digit, Maintenance::Local)],
                now,
            )
            .await?;
            decided.into_iter().next().map(|decision| decision.outcome)
        }
    };
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    assert_eq!(
        claim_in(agent(1), 'a', at(10)).await,
        lease(agent(1), 1, at(10)).map(ClaimOutcome::Renewed),
        "the holder is renewed"
    );
    let lapsed = at(0).and_then(|start| after(start, LEASE_LIFE + LEASE_LIFE));
    assert_eq!(
        claim_in(agent(2), 'a', lapsed).await,
        Some(ClaimOutcome::Refused(ClaimRefused::Starting)),
        "a lease that ran out moves to nobody"
    );
    assert_eq!(
        claim_in(agent(2), 'b', at(0)).await,
        Some(ClaimOutcome::Refused(ClaimRefused::Starting)),
        "a repository nobody holds is granted to nobody"
    );
    assert_eq!(
        ownership(registry.as_ref(), 'b').await,
        repository('b').map(RepositoryOwnership::new),
        "no epoch was given"
    );
}

/// `holder`'s proven floor on the repository `digit` at `number`, its entry
/// running out a lease life after `at(0)`.
fn proven(holder: AgentId, digit: char, number: u64, held: bool) -> Option<ProvenFloor> {
    Some(ProvenFloor {
        repository: repository(digit)?,
        holder,
        epoch: epoch(number)?,
        expires_at: after(at(0)?, LEASE_LIFE)?,
        held,
    })
}

/// What `registry` decided of `agent`'s report of `entries` with the
/// floors `floors` proved, at `now`.
async fn report_proving(
    registry: &dyn OwnershipRegistry,
    agent: AgentId,
    entries: &[Option<RepositoryReport>],
    floors: &[Option<ProvenFloor>],
    now: Option<Timestamp>,
) -> Option<Vec<ClaimDecision>> {
    let (Some(reports), Some(floors), Some(now)) = (
        reports(entries),
        floors.iter().cloned().collect::<Option<Vec<_>>>(),
        now,
    ) else {
        return None;
    };
    ok(
        registry
            .report(agent, &reports, &floors, now, &StartWindow::default())
            .await,
        "a report",
    )
}

async fn floors(registry: &dyn OwnershipRegistry) -> Option<Vec<ProvenFloor>> {
    ok(registry.recovery().await, "the recovery")
        .map(|recovery| recovery.floors().cloned().collect())
}

async fn a_floor_above_the_counter_starts_recovery_mode(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    let decided = report_proving(
        registry.as_ref(),
        agent(1),
        &[entry('a', Maintenance::Local)],
        &[proven(agent(1), 'a', 3, true)],
        at(10),
    )
    .await;
    let outcomes: Option<Vec<_>> = decided.map(|d| d.into_iter().map(|d| d.outcome).collect());
    assert_eq!(
        outcomes,
        Some(vec![ClaimOutcome::Refused(ClaimRefused::Recovering)]),
        "the report that proved it is refused too"
    );
    assert_eq!(
        claim(registry.as_ref(), agent(2), 'b', at(20)).await,
        Some(ClaimOutcome::Refused(ClaimRefused::Recovering)),
        "nobody is granted anything"
    );
    assert_eq!(
        ownership(registry.as_ref(), 'b').await,
        repository('b').map(RepositoryOwnership::new),
        "no epoch was given"
    );
    let reopened = harness.reopened().await;
    assert_eq!(
        floors(reopened.as_ref()).await,
        proven(agent(1), 'a', 3, true).map(|floor| vec![floor]),
        "recovery mode outlives the hub"
    );
}

async fn a_floor_at_the_counter_changes_nothing(harness: &dyn OwnershipRegistryHarness) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(1), 'a', at(0)).await;
    let decided = report_proving(
        registry.as_ref(),
        agent(1),
        &[entry('a', Maintenance::Local)],
        &[proven(agent(1), 'a', 1, true)],
        at(10),
    )
    .await;
    assert_eq!(
        decided
            .and_then(|d| d.into_iter().next())
            .map(|d| d.outcome),
        lease(agent(1), 1, at(10)).map(ClaimOutcome::Renewed)
    );
    assert_eq!(floors(registry.as_ref()).await, Some(vec![]));
}

async fn ending_recovery_mode_raises_the_counter_to_the_floor(
    harness: &dyn OwnershipRegistryHarness,
) {
    let registry = harness.empty().await;
    claim(registry.as_ref(), agent(2), 'b', at(0)).await;
    report_proving(
        registry.as_ref(),
        agent(1),
        &[entry('a', Maintenance::Local), entry('b', Maintenance::Hub)],
        &[
            proven(agent(1), 'a', 3, true),
            proven(agent(1), 'b', 4, false),
        ],
        at(10),
    )
    .await;
    assert_eq!(
        ok(registry.end_recovery().await, "the end of recovery mode"),
        Some(true)
    );
    assert_eq!(floors(registry.as_ref()).await, Some(vec![]));
    let held = ownership(registry.as_ref(), 'a').await;
    assert_eq!(held.as_ref().and_then(|o| o.last_epoch), epoch(3));
    assert_eq!(
        held.and_then(|o| o.lease),
        lease(agent(1), 3, at(0)),
        "the held lease is the hub's again"
    );
    let released = ownership(registry.as_ref(), 'b').await;
    assert_eq!(released.as_ref().and_then(|o| o.last_epoch), epoch(4));
    assert_eq!(
        released.and_then(|o| o.lease),
        None,
        "the lease below the floor is dropped"
    );
    // The machine that proved the floor stops using the repository, so it
    // no longer counts as shared.
    report_proving(registry.as_ref(), agent(1), &[], &[], at(15)).await;
    assert_eq!(
        claim(registry.as_ref(), agent(2), 'b', at(20)).await,
        lease(agent(2), 5, at(20)).map(ClaimOutcome::Granted),
        "the next epoch lies above the floor"
    );
    assert_eq!(
        ok(registry.end_recovery().await, "a second end"),
        Some(false)
    );
}
