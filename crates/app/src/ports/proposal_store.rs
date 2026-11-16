use std::net::IpAddr;

use async_trait::async_trait;
use domain::Timestamp;
use domain::planning::{
    ConfigRevision, ProposalId, ProposalStatus, Proposer, Rationale, RejectReason, TooSoon,
};

use crate::ports::StoreError;

/// A proposal as a [`ProposalStore`] keeps it: everything the
/// [`Proposal`](domain::planning::Proposal) holds, its changes as the text
/// a [`ChangeCodec`](crate::ports::ChangeCodec) made of them, so the store
/// knows no configuration format, and where it was approved from.
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
    /// When a person rejected or approved it.
    pub decided_at: Option<Timestamp>,
    /// A person's reasons, one per change, once they rejected it.
    pub reasons: Vec<RejectReason>,
    /// The address of the browser a person approved it from.
    pub approved_from: Option<IpAddr>,
    /// The configuration history's version its apply kept.
    pub applied_as: Option<String>,
}

/// Whether [`ProposalStore::add`] kept the proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Kept,
    /// Its proposer may not propose now; nothing changed.
    Refused(TooSoon),
}

/// Keeps the proposals (technical design §9 `proposals`; decision 89),
/// pending and ended, between the daemon's starts.
///
/// Contract (checked by `contract::proposal_store`):
/// - [`add`](Self::add) keeps a proposal unless its proposer's record
///   refuses it ([`ProposerRecord::admits`](domain::planning::ProposerRecord::admits)
///   at the new proposal's `created_at`), deciding and keeping in one step,
///   so of adds racing for the last place exactly one is kept. Proposers
///   are one by their token's ID; every proposer with no token is one. A
///   proposal counts as pending while its status is pending and its expiry
///   is after the new proposal's `created_at`, whether or not anything has
///   marked it expired; every proposal kept counts as made when it was, and
///   the last rejection is the latest `decided_at` of a rejected one. After
///   a proposal is kept, the oldest ended ones beyond `ended_at_most` are
///   forgotten.
/// - [`get`](Self::get) returns a proposal as it was kept, its times cut to
///   the millisecond; an ID never kept, or forgotten, is none.
/// - [`all`](Self::all) returns every proposal kept, newest first.
/// - [`change_status`](Self::change_status), [`reject`](Self::reject),
///   [`approve`](Self::approve) and [`applied`](Self::applied) each change
///   a proposal's status only from the one they name, with what they record,
///   in one step, so of changes racing from one status exactly one is made,
///   and say whether it was.
/// - What is kept outlives the handle: a store opened again on the same
///   place returns it.
#[async_trait]
pub trait ProposalStore: Send + Sync {
    /// Keeps `proposal` unless its proposer's record refuses it, with
    /// `pending_at_most` pending at most, then forgets the oldest ended
    /// proposals beyond `ended_at_most`.
    async fn add(
        &self,
        proposal: &StoredProposal,
        pending_at_most: usize,
        ended_at_most: usize,
    ) -> Result<Admission, StoreError>;

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

    /// Rejects the pending proposal `id` at `at` for `reasons`; whether it
    /// did.
    async fn reject(
        &self,
        id: &ProposalId,
        at: Timestamp,
        reasons: &[RejectReason],
    ) -> Result<bool, StoreError>;

    /// Starts applying the pending proposal `id`, approved at `at` from
    /// `from`; whether it did.
    async fn approve(
        &self,
        id: &ProposalId,
        at: Timestamp,
        from: Option<IpAddr>,
    ) -> Result<bool, StoreError>;

    /// Marks the proposal `id`, being applied, applied, its apply having
    /// kept the history's version `applied_as`; whether it did.
    async fn applied(&self, id: &ProposalId, applied_as: Option<&str>) -> Result<bool, StoreError>;
}
