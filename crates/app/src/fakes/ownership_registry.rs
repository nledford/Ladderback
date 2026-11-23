use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::maintenance::{
    LeaseEpoch, ReleaseRefused, RepositoryOwnership, RepositoryReport, RepositoryReports,
};
use domain::{AgentId, RepositoryId, Timestamp};

use crate::ports::{ClaimDecision, OwnershipRegistry, StoreError};

/// The hub's registry of maintenance owners in memory, for use-case tests
/// and acceptance scenarios: one instance stands for one database. One lock
/// covers a whole report, so reports that race are decided one after the
/// other, as the store's transactions are.
#[derive(Debug, Default)]
pub struct InMemoryOwnershipRegistry {
    kept: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    owners: BTreeMap<RepositoryId, RepositoryOwnership>,
    /// Each agent's last report, and when the hub took it.
    reports: BTreeMap<AgentId, (Vec<RepositoryReport>, Timestamp)>,
}

impl InMemoryOwnershipRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl OwnershipRegistry for InMemoryOwnershipRegistry {
    async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        now: Timestamp,
    ) -> Result<Vec<ClaimDecision>, StoreError> {
        let _ = (self.kept(), agent, reports, now);
        todo!()
    }

    async fn release(
        &self,
        agent: AgentId,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<Result<(), ReleaseRefused>, StoreError> {
        let _ = (agent, repository, epoch);
        todo!()
    }

    async fn ownership(
        &self,
        repository: &RepositoryId,
    ) -> Result<RepositoryOwnership, StoreError> {
        let _ = repository;
        todo!()
    }
}
