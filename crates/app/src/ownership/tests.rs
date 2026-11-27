use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domain::fleet::{AgentRecord, AgentStatus, Hostname, PublicKey};
use domain::maintenance::{ClaimRefused, RepositoryOwnership, RepositoryReport};
use domain::planning::Maintenance;
use domain::{Timestamp, Uuid};

use super::*;
use crate::fakes::{
    FakeClock, InMemoryAgentRegistry, InMemoryDriveFlush, InMemoryOwnershipRegistry,
};

/// The in-memory registry, whose next report fails once told to.
struct FailingReport {
    registry: InMemoryOwnershipRegistry,
    fail: AtomicBool,
}

#[async_trait]
impl OwnershipRegistry for FailingReport {
    async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        proven: &[ProvenFloor],
        now: domain::Timestamp,
        window: &StartWindow,
    ) -> Result<Vec<ClaimDecision>, StoreError> {
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(StoreError::Unavailable {
                detail: "the database is locked".to_owned(),
            });
        }
        self.registry
            .report(agent, reports, proven, now, window)
            .await
    }

    async fn recovery(&self) -> Result<domain::maintenance::HubRecovery, StoreError> {
        self.registry.recovery().await
    }

    async fn end_recovery(&self) -> Result<bool, StoreError> {
        self.registry.end_recovery().await
    }

    async fn reported_uses(&self) -> Result<Vec<(AgentId, RepositoryId)>, StoreError> {
        self.registry.reported_uses().await
    }

    async fn release(
        &self,
        agent: AgentId,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<Result<(), ReleaseRefused>, StoreError> {
        self.registry.release(agent, repository, epoch).await
    }

    async fn ownership(
        &self,
        repository: &RepositoryId,
    ) -> Result<RepositoryOwnership, StoreError> {
        self.registry.ownership(repository).await
    }
}

fn machine(agents: &InMemoryAgentRegistry, n: u8, now: Timestamp) -> AgentId {
    let agent_id = AgentId::from_uuid(Uuid::from_u128(u128::from(n)));
    agents.hold(AgentRecord {
        agent_id,
        key: PublicKey::from_bytes([n; 32]).unwrap(),
        hostname: Hostname::parse(&format!("machine-{n}")).unwrap(),
        status: AgentStatus::Approved { at: now },
        counter: 1,
        last_seen: now,
        zone: None,
    });
    agent_id
}

fn photos() -> RepositoryId {
    RepositoryId::parse(&"a".repeat(64)).unwrap()
}

fn reports(request: Maintenance) -> RepositoryReports {
    RepositoryReports::new([RepositoryReport {
        repository: photos(),
        request,
        holds_other_agents_snapshots: false,
        floor: None,
    }])
}

#[tokio::test]
async fn a_report_that_wasnt_stored_doesnt_end_the_start_window() {
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let agents = Arc::new(InMemoryAgentRegistry::new());
    let laptop = machine(&agents, 1, clock.now());
    let nas = machine(&agents, 2, clock.now());
    let registry = InMemoryOwnershipRegistry::new();
    registry
        .report(
            laptop,
            &reports(Maintenance::None),
            &[],
            clock.now(),
            &StartWindow::default(),
        )
        .await
        .unwrap();
    // Past the report's life, so only the start window keeps "photos".
    clock.advance(SignedDuration::from_hours(8 * 24));
    let registry = Arc::new(FailingReport {
        registry,
        fail: AtomicBool::new(true),
    });
    let hub = RepositoryOwners::new(
        agents,
        registry,
        clock,
        Arc::new(InMemoryDriveFlush::default()),
    );

    assert!(
        hub.report(laptop, &reports(Maintenance::None), &[])
            .await
            .is_err()
    );
    let refused = hub
        .report(nas, &reports(Maintenance::Local), &[])
        .await
        .unwrap();

    assert_eq!(
        refused,
        Reported::Decided(vec![ClaimDecision {
            repository: photos(),
            outcome: ClaimOutcome::Refused(ClaimRefused::Starting),
        }])
    );
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ))
}

/// How many leases the hub gives to be signed.
fn given(granting: Granting) -> usize {
    match granting {
        Granting::Leases { leases, .. } => leases.len(),
        Granting::Withheld | Granting::NotApproved => 0,
    }
}

/// The hub opens its start window when it starts: it waits for every
/// machine approved before then, which may hold a lease the hub lost with
/// its last commits, and for none approved since, which can't (§16,
/// decision 178).
#[tokio::test]
async fn the_start_window_waits_for_every_machine_approved_before_the_start_and_none_since() {
    let clock = clock();
    let agents = Arc::new(InMemoryAgentRegistry::new());
    let laptop = machine(&agents, 1, clock.now());
    let nas = machine(&agents, 2, clock.now());
    let hub = RepositoryOwners::new(
        agents.clone(),
        Arc::new(InMemoryOwnershipRegistry::new()),
        clock.clone(),
        Arc::new(InMemoryDriveFlush::default()),
    );
    hub.open().await.unwrap();
    let _tablet = machine(&agents, 3, clock.now());

    let refused = hub
        .report(laptop, &reports(Maintenance::Local), &[])
        .await
        .unwrap();
    assert_eq!(
        refused,
        Reported::Decided(vec![ClaimDecision {
            repository: photos(),
            outcome: ClaimOutcome::Refused(ClaimRefused::Starting),
        }]),
        "the NAS hasn't reported since the start"
    );
    hub.report(nas, &RepositoryReports::new([]), &[])
        .await
        .unwrap();
    let granted = hub
        .report(laptop, &reports(Maintenance::Local), &[])
        .await
        .unwrap();
    assert!(
        matches!(
            &granted,
            Reported::Decided(decided)
                if matches!(decided.as_slice(), [ClaimDecision { outcome: ClaimOutcome::Granted(_), .. }])
        ),
        "the tablet, approved since the start, isn't waited for: {granted:?}"
    );
}

/// A new grant is on the drive's permanent storage before the hub gives
/// it to be signed and sent, so a power cut can't take back a grant a
/// machine holds; a renewal isn't flushed, and a grant whose flush fails
/// isn't given (§16, decision 178).
#[tokio::test]
async fn a_new_grant_is_flushed_before_it_is_given_to_be_signed_and_a_renewal_isnt() {
    let clock = clock();
    let agents = Arc::new(InMemoryAgentRegistry::new());
    let drive = Arc::new(InMemoryDriveFlush::default());
    let hub = RepositoryOwners::new(
        agents.clone(),
        Arc::new(InMemoryOwnershipRegistry::new()),
        clock.clone(),
        drive.clone(),
    );
    hub.open().await.unwrap();
    let laptop = machine(&agents, 1, clock.now());

    drive.fail(StoreError::Unavailable {
        detail: "the drive".to_owned(),
    });
    assert!(
        hub.grant(laptop, &reports(Maintenance::Local), &[])
            .await
            .is_err(),
        "a grant whose flush failed isn't given to be signed"
    );
    drive.recover();
    let renewed = hub
        .grant(laptop, &reports(Maintenance::Local), &[])
        .await
        .unwrap();
    assert_eq!(given(renewed), 1);
    assert_eq!(
        drive.flushed(),
        1,
        "a grant whose flush failed is flushed before it goes out as a renewal"
    );
    hub.grant(laptop, &reports(Maintenance::Local), &[])
        .await
        .unwrap();
    assert_eq!(drive.flushed(), 1, "a renewal");

    let tablet = machine(&agents, 2, clock.now());
    let other = RepositoryReports::new([RepositoryReport {
        repository: RepositoryId::parse(&"b".repeat(64)).unwrap(),
        request: Maintenance::Local,
        holds_other_agents_snapshots: false,
        floor: None,
    }]);
    let granted = hub.grant(tablet, &other, &[]).await.unwrap();
    assert_eq!(given(granted), 1);
    assert_eq!(drive.flushed(), 2, "a new grant");
}
