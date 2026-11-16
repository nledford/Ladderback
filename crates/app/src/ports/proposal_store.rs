use async_trait::async_trait;
use domain::Timestamp;
use domain::planning::{ConfigRevision, ProposalId, ProposalStatus, Proposer, Rationale};

use crate::ports::{Added, StoreError};

/// A proposal as a [`ProposalStore`] keeps it: everything the
/// [`Proposal`](domain::planning::Proposal) holds, its changes as the text
/// a [`ChangeCodec`](crate::ports::ChangeCodec) made of them, so the store
/// knows no configuration format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredProposal {
    pub id: ProposalId,
    pub proposer: Proposer,
    pub base: ConfigRevision,
    pub changes: String,
    pub rationale: Rationale,
    pub status: ProposalStatus,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// Keeps the proposals (technical design §9 `proposals`; decision 89),
/// pending and ended, between the daemon's starts.
///
/// Contract (checked by `contract::proposal_store`):
/// - [`add`](Self::add) keeps a proposal unless its proposer has
///   `pending_at_most` pending already, deciding and keeping in one step, so
///   of adds racing for the last place exactly one is kept. Proposers are
///   one by their token's ID; every proposer with no token is one. A
///   proposal counts as pending while its status is pending and its expiry
///   is after the new proposal's `created_at`, whether or not anything has
///   marked it expired. After a proposal is kept, the oldest ended ones
///   beyond `ended_at_most` are forgotten.
/// - [`get`](Self::get) returns a proposal as it was kept, its times cut to
///   the millisecond; an ID never kept, or forgotten, is none.
/// - [`all`](Self::all) returns every proposal kept, newest first.
/// - [`change_status`](Self::change_status) changes a proposal's status
///   only from `from`, in one step, so of changes racing from one status
///   exactly one is made, and says whether it made it.
/// - What is kept outlives the handle: a store opened again on the same
///   place returns it.
#[async_trait]
pub trait ProposalStore: Send + Sync {
    /// Keeps `proposal` unless its proposer has `pending_at_most` pending
    /// that expire after `proposal.created_at`, then forgets the oldest
    /// ended proposals beyond `ended_at_most`.
    async fn add(
        &self,
        proposal: &StoredProposal,
        pending_at_most: usize,
        ended_at_most: usize,
    ) -> Result<Added, StoreError>;

    /// The proposal `id`, if it is kept.
    async fn get(&self, id: &ProposalId) -> Result<Option<StoredProposal>, StoreError>;

    /// Every proposal kept, newest first.
    async fn all(&self) -> Result<Vec<StoredProposal>, StoreError>;

    /// Gives the proposal `id` the status `to` if its status is `from`;
    /// whether it did.
    async fn change_status(
        &self,
        id: &ProposalId,
        from: ProposalStatus,
        to: ProposalStatus,
    ) -> Result<bool, StoreError>;
}
