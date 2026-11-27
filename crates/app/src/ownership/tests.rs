use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domain::fleet::{AgentRecord, AgentStatus, Hostname, PublicKey};
use domain::maintenance::{ClaimRefused, RepositoryOwnership, RepositoryReport};
use domain::planning::Maintenance;
use domain::{Timestamp, Uuid};

use super::*;
use crate::fakes::{FakeClock, InMemoryAgentRegistry, InMemoryOwnershipRegistry};

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
        version: None,
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
    let hub = RepositoryOwners::new(agents, registry, clock);

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
