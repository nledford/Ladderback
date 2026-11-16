use std::cmp::Reverse;
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;
use domain::planning::{ProposalId, ProposalStatus, Proposer, ProposerRecord, RejectReason};

use crate::ports::{Admission, ProposalStore, StoreError, StoredProposal};

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

    /// Changes the proposal `id` from `from` by `change`; whether it did.
    fn change(
        &self,
        id: &ProposalId,
        from: ProposalStatus,
        change: impl FnOnce(&mut StoredProposal),
    ) -> bool {
        let mut kept = self.kept();
        let Some(one) = kept
            .iter_mut()
            .find(|one| one.id == *id && one.status == from)
        else {
            return false;
        };
        change(one);
        true
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
    ) -> Result<Admission, StoreError> {
        let mut kept = self.kept();
        let made_at = to_the_millisecond(proposal.created_at);
        let theirs: Vec<&StoredProposal> = kept
            .iter()
            .filter(|one| same_proposer(&one.proposer, &proposal.proposer))
            .collect();
        let record = ProposerRecord {
            pending: theirs
                .iter()
                .filter(|one| one.status == ProposalStatus::Pending && one.expires_at > made_at)
                .count(),
            made: theirs.iter().map(|one| one.created_at).collect(),
            last_rejected: theirs
                .iter()
                .filter(|one| one.status == ProposalStatus::Rejected)
                .filter_map(|one| one.decided_at)
                .max(),
        };
        if let Err(refused) = record.admits(made_at, pending_at_most) {
            return Ok(Admission::Refused(refused));
        }
        let mut proposal = proposal.clone();
        proposal.created_at = made_at;
        proposal.expires_at = to_the_millisecond(proposal.expires_at);
        proposal.decided_at = proposal.decided_at.map(to_the_millisecond);
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
        Ok(Admission::Kept)
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
        Ok(self.change(id, from, |one| one.status = to))
    }

    async fn reject(
        &self,
        id: &ProposalId,
        at: Timestamp,
        reasons: &[RejectReason],
    ) -> Result<bool, StoreError> {
        let _ = (id, at, reasons);
        todo!()
    }

    async fn approve(
        &self,
        id: &ProposalId,
        at: Timestamp,
        from: Option<IpAddr>,
    ) -> Result<bool, StoreError> {
        let _ = (id, at, from);
        todo!()
    }

    async fn applied(&self, id: &ProposalId, applied_as: Option<&str>) -> Result<bool, StoreError> {
        let _ = (id, applied_as);
        todo!()
    }
}
