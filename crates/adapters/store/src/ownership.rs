//! The hub's registry of maintenance owners (technical design §7.3, I1;
//! migration `0022_repository_ownership`): the `OwnershipRegistry` port on
//! the store's own file, built only with the hub (decision 122).

use app::ports::{ClaimDecision, OwnershipRegistry, StoreError};
use async_trait::async_trait;
use domain::maintenance::{LeaseEpoch, ReleaseRefused, RepositoryOwnership, RepositoryReports};
use domain::{AgentId, RepositoryId, Timestamp};

use crate::SqliteStore;

#[async_trait]
impl OwnershipRegistry for SqliteStore {
    async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        now: Timestamp,
    ) -> Result<Vec<ClaimDecision>, StoreError> {
        let _ = (agent, reports, now);
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
