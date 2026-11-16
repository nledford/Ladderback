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
    AgentConfig, ConfigChange, ConfigRevision, MAX_ENDED, MAX_PENDING, Proposal, ProposalId,
    ProposalRefused, ProposalStatus, Proposer, Rationale,
};

use crate::ports::{
    Added, ChangeCodec, Clock, CodecError, ConfigError, ConfigStore, ProposalStore, Random,
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
    /// The changes couldn't be written to be kept.
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
    /// [`MAX_ENDED`] are forgotten. It is made at the millisecond, so it is
    /// answered as it is read back.
    pub async fn propose(
        &self,
        proposer: Proposer,
        base: ConfigRevision,
        changes: Vec<ConfigChange>,
        rationale: Rationale,
    ) -> Result<Proposal, ProposalError> {
        let config = self.load().await?;
        let bytes = self.random.token().map_err(ProposalError::Random)?;
        let proposal = Proposal::draft(
            ProposalId::from_bytes(&bytes),
            proposer,
            base,
            changes,
            rationale,
            &config,
            to_the_millisecond(self.clock.now()),
        )
        .map_err(ProposalError::Refused)?;
        let stored = self.stored(&proposal)?;
        match self
            .store
            .add(&stored, MAX_PENDING, MAX_ENDED)
            .await
            .map_err(ProposalError::Store)?
        {
            Added::Kept => Ok(proposal),
            Added::Full => Err(ProposalError::Full),
        }
    }

    /// The proposals `asker` may see, newest first, each brought up to
    /// date. A token sees the proposals it made; a client with no token
    /// sees them all.
    pub async fn list(&self, asker: &Proposer) -> Result<Vec<Proposal>, ProposalError> {
        let all = self.store.all().await.map_err(ProposalError::Store)?;
        let mut config = None;
        let mut listed = Vec::new();
        for stored in all {
            if !may_see(asker, &stored.proposer) {
                continue;
            }
            match self.up_to_date(stored, &mut config).await {
                Ok(proposal) => listed.push(proposal),
                // Forgotten since the list was read: it is left out, as a
                // list read a moment later would leave it.
                Err(ProposalError::Unknown) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(listed)
    }

    /// The proposal `id`, brought up to date, if `asker` may see it.
    pub async fn get(&self, asker: &Proposer, id: &ProposalId) -> Result<Proposal, ProposalError> {
        let stored = self
            .store
            .get(id)
            .await
            .map_err(ProposalError::Store)?
            .filter(|stored| may_see(asker, &stored.proposer))
            .ok_or(ProposalError::Unknown)?;
        self.up_to_date(stored, &mut None).await
    }

    /// Withdraws the pending proposal `id`, if `asker` may see it, and
    /// answers it withdrawn.
    pub async fn withdraw(
        &self,
        asker: &Proposer,
        id: &ProposalId,
    ) -> Result<Proposal, ProposalError> {
        let mut proposal = self.get(asker, id).await?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ProposalError::NotPending(proposal.status));
        }
        let withdrawn = self
            .store
            .change_status(id, ProposalStatus::Pending, ProposalStatus::Withdrawn)
            .await
            .map_err(ProposalError::Store)?;
        if !withdrawn {
            // It ended, or began to be applied, since it was read.
            let now = self.kept_status(id).await?;
            return Err(ProposalError::NotPending(now));
        }
        proposal.status = ProposalStatus::Withdrawn;
        Ok(proposal)
    }

    async fn load(&self) -> Result<AgentConfig, ProposalError> {
        self.config
            .load()
            .await
            .map_err(|error| ProposalError::Config(Box::new(error)))
    }

    /// The status the proposal `id` is kept with now.
    async fn kept_status(&self, id: &ProposalId) -> Result<ProposalStatus, ProposalError> {
        self.store
            .get(id)
            .await
            .map_err(ProposalError::Store)?
            .map(|kept| kept.status)
            .ok_or(ProposalError::Unknown)
    }

    /// The proposal `stored` keeps, brought up to date: a pending one that
    /// has expired, or gone stale against the configuration, is kept so.
    /// `config` is the configuration, loaded once, at the first pending one.
    /// Changes that can't be read back, as when a rule has tightened since
    /// they were kept, no longer apply: the proposal is answered with none,
    /// and a pending one goes stale, or expires, rather than taking every
    /// read of the proposals down with it.
    async fn up_to_date(
        &self,
        stored: StoredProposal,
        config: &mut Option<AgentConfig>,
    ) -> Result<Proposal, ProposalError> {
        let changes = self.codec.decode(&stored.changes).unwrap_or_default();
        let mut proposal = Proposal {
            id: stored.id,
            proposer: stored.proposer,
            base: stored.base,
            changes,
            rationale: stored.rationale,
            status: stored.status,
            created_at: stored.created_at,
            expires_at: stored.expires_at,
        };
        if proposal.status != ProposalStatus::Pending {
            return Ok(proposal);
        }
        let config = match config {
            Some(config) => config,
            None => config.insert(self.load().await?),
        };
        let now = proposal.status_at(self.clock.now(), config);
        if now == ProposalStatus::Pending {
            return Ok(proposal);
        }
        let changed = self
            .store
            .change_status(&proposal.id, ProposalStatus::Pending, now)
            .await
            .map_err(ProposalError::Store)?;
        proposal.status = if changed {
            now
        } else {
            // Another request moved it on first: it is as that one left it.
            self.kept_status(&proposal.id).await?
        };
        Ok(proposal)
    }

    fn stored(&self, proposal: &Proposal) -> Result<StoredProposal, ProposalError> {
        Ok(StoredProposal {
            id: proposal.id.clone(),
            proposer: proposal.proposer.clone(),
            base: proposal.base.clone(),
            changes: self
                .codec
                .encode(&proposal.changes)
                .map_err(ProposalError::Codec)?,
            rationale: proposal.rationale.clone(),
            status: proposal.status,
            created_at: proposal.created_at,
            expires_at: proposal.expires_at,
        })
    }
}

/// Whether `asker` may see a proposal `proposer` made: a token sees its
/// own, a client with no token sees every one.
fn may_see(asker: &Proposer, proposer: &Proposer) -> bool {
    match (asker, proposer) {
        (Proposer::NoToken, _) => true,
        (Proposer::Token { id: asker, .. }, Proposer::Token { id, .. }) => asker == id,
        (Proposer::Token { .. }, Proposer::NoToken) => false,
    }
}

/// `now` cut to the millisecond, as a [`ProposalStore`] keeps times.
fn to_the_millisecond(now: domain::Timestamp) -> domain::Timestamp {
    domain::Timestamp::from_millisecond(now.as_millisecond()).unwrap_or(now)
}

#[cfg(test)]
mod tests;
