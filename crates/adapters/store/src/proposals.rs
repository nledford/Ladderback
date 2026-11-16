//! The proposals (technical design §9 `proposals`; decision 89): the
//! `ProposalStore` port on the store's own file.

use app::ports::{Added, ProposalStore, StoreError, StoredProposal};
use async_trait::async_trait;
use domain::planning::{ProposalId, ProposalStatus};

use crate::SqliteStore;

#[async_trait]
impl ProposalStore for SqliteStore {
    async fn add(
        &self,
        proposal: &StoredProposal,
        pending_at_most: usize,
        ended_at_most: usize,
    ) -> Result<Added, StoreError> {
        let _ = (proposal, pending_at_most, ended_at_most);
        todo!()
    }

    async fn get(&self, id: &ProposalId) -> Result<Option<StoredProposal>, StoreError> {
        let _ = id;
        todo!()
    }

    async fn all(&self) -> Result<Vec<StoredProposal>, StoreError> {
        todo!()
    }

    async fn change_status(
        &self,
        id: &ProposalId,
        from: ProposalStatus,
        to: ProposalStatus,
    ) -> Result<bool, StoreError> {
        let _ = (id, from, to);
        todo!()
    }
}
