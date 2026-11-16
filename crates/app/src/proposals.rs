//! Proposals (§16, decision 89): an AI agent, or another API client,
//! proposes configuration changes for a person to confirm, lists and reads
//! its proposals and withdraws one. Making a proposal checks its changes
//! against the configuration by the configuration's rules and nothing else:
//! no secret is resolved, no command runs and no location is contacted, and
//! the configuration is never written. Reading proposals brings each pending
//! one up to date first: expired from its expiry on, stale once a change of
//! it no longer applies to the configuration as it is.

use std::sync::Arc;

use domain::planning::{
    ConfigChange, ConfigRevision, MAX_ENDED, MAX_PENDING, Proposal, ProposalId, ProposalRefused,
    ProposalStatus, Proposer, Rationale,
};

use crate::ports::{
    ChangeCodec, Clock, CodecError, ConfigError, ConfigStore, ProposalStore, Random,
    RandomUnavailable, StoreError, StoredProposal,
};

/// Why a proposal was not made, read or withdrawn. Nothing was kept or
/// changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalError {
    /// The submission checks refused the proposal.
    Refused(ProposalRefused),
    /// Its proposer has [`MAX_PENDING`] proposals pending already.
    Full,
    /// No proposal by that ID is kept that the asker may see.
    Unknown,
    /// The proposal has ended, or is being applied: only a pending one is
    /// withdrawn.
    NotPending(ProposalStatus),
    /// The configuration couldn't be read. Boxed: the error is large.
    Config(Box<ConfigError>),
    /// The proposals couldn't be read or kept.
    Store(StoreError),
    /// Kept changes couldn't be written or read back.
    Codec(CodecError),
    /// The platform gave no random bytes for the proposal's ID.
    Random(RandomUnavailable),
}

/// Makes, reads and withdraws proposals, kept in `store`, checked against
/// the configuration `config`.
pub struct Proposals {
    store: Arc<dyn ProposalStore>,
    codec: Arc<dyn ChangeCodec>,
    config: Arc<dyn ConfigStore>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl Proposals {
    pub fn new(
        store: Arc<dyn ProposalStore>,
        codec: Arc<dyn ChangeCodec>,
        config: Arc<dyn ConfigStore>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            store,
            codec,
            config,
            clock,
            random,
        }
    }

    /// Keeps a pending proposal of `changes` by `proposer`, drafted against
    /// the configuration `base`, once they pass the submission checks
    /// ([`Proposal::draft`]) and the proposer has fewer than
    /// [`MAX_PENDING`] pending. The oldest ended proposals beyond
    /// [`MAX_ENDED`] are forgotten.
    pub async fn propose(
        &self,
        proposer: Proposer,
        base: ConfigRevision,
        changes: Vec<ConfigChange>,
        rationale: Rationale,
    ) -> Result<Proposal, ProposalError> {
        let _ = (proposer, base, changes, rationale, MAX_ENDED, MAX_PENDING);
        todo!()
    }

    /// The proposals `asker` may see, newest first, each brought up to
    /// date. A token sees the proposals it made; a client with no token
    /// sees them all.
    pub async fn list(&self, asker: &Proposer) -> Result<Vec<Proposal>, ProposalError> {
        let _ = asker;
        todo!()
    }

    /// The proposal `id`, brought up to date, if `asker` may see it.
    pub async fn get(&self, asker: &Proposer, id: &ProposalId) -> Result<Proposal, ProposalError> {
        let _ = (asker, id);
        todo!()
    }

    /// Withdraws the pending proposal `id`, if `asker` may see it, and
    /// answers it withdrawn.
    pub async fn withdraw(
        &self,
        asker: &Proposer,
        id: &ProposalId,
    ) -> Result<Proposal, ProposalError> {
        let _ = (asker, id);
        todo!()
    }

    fn stored(&self, proposal: &Proposal) -> Result<StoredProposal, ProposalError> {
        let _ = (proposal, &self.store, &self.codec, &self.config, &self.clock, &self.random);
        todo!()
    }
}

#[cfg(test)]
mod tests;
