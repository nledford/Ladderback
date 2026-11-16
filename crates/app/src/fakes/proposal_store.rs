use std::cmp::Reverse;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;
use domain::planning::{ProposalId, ProposalStatus, Proposer};

use crate::ports::{Added, ProposalStore, StoreError, StoredProposal};

/// The proposals, kept in memory, for use-case tests and acceptance
/// scenarios. It keeps times to the millisecond, as the real store does.
#[derive(Default)]
pub struct InMemoryProposalStore {
    /// In the order they were kept.
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

/// `at` cut to the millisecond, as the real store keeps it.
fn to_the_millisecond(at: Timestamp) -> Timestamp {
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
}

/// Whether `a` and `b` are one proposer: one token, or both no token.
fn same_proposer(a: &Proposer, b: &Proposer) -> bool {
    match (a, b) {
        (Proposer::Token { id: a, .. }, Proposer::Token { id: b, .. }) => a == b,
        (Proposer::NoToken, Proposer::NoToken) => true,
        _ => false,
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
        let mut kept = self.kept();
        let made_at = to_the_millisecond(proposal.created_at);
        let pending = kept
            .iter()
            .filter(|one| {
                one.status == ProposalStatus::Pending
                    && one.expires_at > made_at
                    && same_proposer(&one.proposer, &proposal.proposer)
            })
            .count();
        if pending >= pending_at_most {
            return Ok(Added::Full);
        }
        let mut proposal = proposal.clone();
        proposal.created_at = to_the_millisecond(proposal.created_at);
        proposal.expires_at = to_the_millisecond(proposal.expires_at);
        kept.push(proposal);
        let mut ended: Vec<(Timestamp, ProposalId)> = kept
            .iter()
            .filter(|one| one.status.has_ended())
            .map(|one| (one.created_at, one.id.clone()))
            .collect();
        ended.sort();
        let beyond = ended.len().saturating_sub(ended_at_most);
        let forgotten: Vec<ProposalId> = ended.into_iter().take(beyond).map(|(_, id)| id).collect();
        kept.retain(|one| !forgotten.contains(&one.id));
        Ok(Added::Kept)
    }

    async fn get(&self, id: &ProposalId) -> Result<Option<StoredProposal>, StoreError> {
        Ok(self.kept().iter().find(|one| one.id == *id).cloned())
    }

    async fn all(&self) -> Result<Vec<StoredProposal>, StoreError> {
        let mut all: Vec<StoredProposal> = self.kept().iter().rev().cloned().collect();
        all.sort_by_key(|one| Reverse(one.created_at));
        Ok(all)
    }

    async fn change_status(
        &self,
        id: &ProposalId,
        from: ProposalStatus,
        to: ProposalStatus,
    ) -> Result<bool, StoreError> {
        let mut kept = self.kept();
        let Some(one) = kept
            .iter_mut()
            .find(|one| one.id == *id && one.status == from)
        else {
            return Ok(false);
        };
        one.status = to;
        Ok(true)
    }
}
