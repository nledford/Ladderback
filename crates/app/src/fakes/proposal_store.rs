use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::planning::{ProposalId, ProposalStatus};

use crate::ports::{Added, ProposalStore, StoreError, StoredProposal};

/// The proposals, kept in memory, for use-case tests and acceptance
/// scenarios. It keeps times to the millisecond, as the real store does.
#[derive(Default)]
pub struct InMemoryProposalStore {
    kept: Mutex<Vec<StoredProposal>>,
}

impl InMemoryProposalStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Vec<StoredProposal>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl ProposalStore for InMemoryProposalStore {
    async fn add(
        &self,
        proposal: &StoredProposal,
        pending_at_most: usize,
        ended_at_most: usize,
    ) -> Result<Added, StoreError> {
        let _ = (self.kept(), proposal, pending_at_most, ended_at_most);
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
