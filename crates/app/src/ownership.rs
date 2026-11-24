//! The hub's registry of repository maintenance owners (technical design
//! §7.3, I1; §16, decision 147): an approved agent's report of the
//! repositories it uses is decided at the hub's time, granting, renewing or
//! refusing each lease, and its holder's release ends one early. A pending
//! agent's report changes nothing, so it holds no lease and refuses no other
//! agent's claim (decision 120, "What pending means"). From its start, the
//! hub moves no repository to a new owner until every agent that used it
//! has reported, or the start window has passed on its monotonic clock
//! (decisions 137 and 157).

use std::sync::{Arc, Mutex};

use domain::fleet::GENERATION;
use domain::maintenance::{
    ClaimOutcome, Grant, LeaseEpoch, ReleaseRefused, RepoGrants, RepositoryReports, StartWindow,
};
use domain::{AgentId, RepositoryId, SignedDuration};

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

/// The hub's side of repository ownership. Made once at the hub's start,
/// which opens its start window.
pub struct RepositoryOwners {
    agents: Arc<dyn AgentRegistry>,
    owners: Arc<dyn OwnershipRegistry>,
    clock: Arc<dyn Clock>,
    /// The hub's monotonic time at its start.
    started: SignedDuration,
    /// The start window, once the first report since the start read it.
    window: Mutex<Option<StartWindow>>,
}

impl RepositoryOwners {
    pub fn new(
        agents: Arc<dyn AgentRegistry>,
        owners: Arc<dyn OwnershipRegistry>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            started: clock.monotonic(),
            agents,
            owners,
            clock,
            window: Mutex::new(None),
        }
    }

    /// Takes `reports` as every repository `agent` uses, and decides each
    /// at the hub's now when the agent is approved.
    pub async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
    ) -> Result<Reported, StoreError> {
        let decided = self.decide(agent, reports, self.clock.now()).await?;
        Ok(decided.map_or(Reported::NotApproved, Reported::Decided))
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
        let now = self.clock.now();
        let Some(decided) = self.decide(agent, reports, now).await? else {
            return Ok(None);
        };
        let grants = decided
            .into_iter()
            .filter_map(|decision| match decision.outcome {
                ClaimOutcome::Granted(lease)
                | ClaimOutcome::Renewed(lease)
                | ClaimOutcome::Unrenewed(lease) => Some(Grant {
                    repository: decision.repository,
                    epoch: lease.epoch,
                    expires_at: lease.expires_at,
                }),
                ClaimOutcome::Refused(_) | ClaimOutcome::NoClaim => None,
            })
            .collect();
        Ok(Some(RepoGrants {
            generation: GENERATION,
            signed_at: now,
            grants,
        }))
    }

    /// What the hub decided of `reports` at `now`, when `agent` is approved.
    async fn decide(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        now: domain::Timestamp,
    ) -> Result<Option<Vec<ClaimDecision>>, StoreError> {
        match self.agents.agent(agent).await? {
            Some(record) if !record.is_pending() => {
                let window = self.window(agent).await?;
                Ok(Some(
                    self.owners.report(agent, reports, now, &window).await?,
                ))
            }
            _ => Ok(None),
        }
    }

    /// The start window as `agent`'s report finds it: read from the
    /// registry at the first report since the start, waiting for `agent`
    /// no more, and holding nothing once its time has passed.
    async fn window(&self, agent: AgentId) -> Result<StartWindow, StoreError> {
        let _ = (agent, self.started, &self.window);
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

#[cfg(test)]
mod tests;
