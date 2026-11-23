//! The hub's registry of repository maintenance owners (technical design
//! §7.3, I1; §16, decision 147): an approved agent's report of the
//! repositories it uses is decided at the hub's time, granting, renewing or
//! refusing each lease, and its holder's release ends one early. A pending
//! agent's report changes nothing, so it holds no lease and refuses no other
//! agent's claim (decision 120, "What pending means").

use std::sync::Arc;

use domain::fleet::GENERATION;
use domain::maintenance::{LeaseEpoch, ReleaseRefused, RepoGrants, RepositoryReports};
use domain::{AgentId, RepositoryId};

use crate::ports::{AgentRegistry, ClaimDecision, Clock, OwnershipRegistry, StoreError};

/// What became of an agent's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reported {
    /// The agent is approved: what the hub decided of each repository.
    Decided(Vec<ClaimDecision>),
    /// The agent is pending, or the hub has no record of it: nothing was
    /// kept or decided.
    NotApproved,
}

/// The hub's side of repository ownership.
pub struct RepositoryOwners {
    agents: Arc<dyn AgentRegistry>,
    owners: Arc<dyn OwnershipRegistry>,
    clock: Arc<dyn Clock>,
}

impl RepositoryOwners {
    pub fn new(
        agents: Arc<dyn AgentRegistry>,
        owners: Arc<dyn OwnershipRegistry>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            agents,
            owners,
            clock,
        }
    }

    /// Takes `reports` as every repository `agent` uses, and decides each
    /// at the hub's now when the agent is approved.
    pub async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
    ) -> Result<Reported, StoreError> {
        match self.agents.agent(agent).await? {
            Some(record) if !record.is_pending() => {
                let decided = self.owners.report(agent, reports, self.clock.now()).await?;
                Ok(Reported::Decided(decided))
            }
            _ => Ok(Reported::NotApproved),
        }
    }

    /// Decides `reports` as [`report`](Self::report) does, and gives every
    /// lease `agent` then holds and claimed (granted, renewed or left to
    /// run out), signed at the hub's now; none for an agent that isn't
    /// approved. A lease it holds on a repository it no longer claims is
    /// left out: it ends at the agent's release or its expiry.
    pub async fn grant(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
    ) -> Result<Option<RepoGrants>, StoreError> {
        let _ = (agent, reports, GENERATION);
        todo!()
    }

    /// Ends the lease `agent` holds on `repository` at `epoch`, at its
    /// signed release.
    pub async fn release(
        &self,
        agent: AgentId,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<Result<(), ReleaseRefused>, StoreError> {
        self.owners.release(agent, repository, epoch).await
    }
}
