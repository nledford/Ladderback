//! The hub's registry of repository maintenance owners (technical design
//! §7.3, I1; §16, decision 147): an approved agent's report of the
//! repositories it uses is decided at the hub's time, granting, renewing or
//! refusing each lease, and its holder's release ends one early. A pending
//! agent's report changes nothing, so it holds no lease and refuses no other
//! agent's claim (decision 120, "What pending means"). From its start, the
//! hub moves no repository to a new owner until every agent that used it
//! has reported, or the start window has passed on its monotonic clock
//! (decisions 137 and 157).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domain::fleet::GENERATION;
use domain::maintenance::{
    ClaimOutcome, Lease, LeaseEpoch, ProvenFloor, ReleaseRefused, RepositoryReports, StartWindow,
};
use domain::{AgentId, RepositoryId, SignedDuration, Timestamp};

use crate::ports::{
    AgentRegistry, ClaimDecision, Clock, DriveFlush, OwnershipRegistry, StoreError,
};

/// What became of an agent's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reported {
    /// The agent is approved: what the hub decided of each repository.
    Decided(Vec<ClaimDecision>),
    /// The agent is pending, or the hub has no record of it: nothing was
    /// kept or decided.
    NotApproved,
}

/// What the hub answers an agent's `ReposSnapshot` with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Granting {
    /// Every lease the agent holds and claimed, at the hub's `generation`,
    /// decided at `signed_at` on its clock: `RepoGrants`, once signed.
    Leases {
        generation: u64,
        signed_at: Timestamp,
        leases: Vec<(RepositoryId, Lease)>,
    },
    /// The hub is in recovery mode: it grants and renews nothing, and the
    /// agent keeps what it holds until it runs out (`GrantsWithheld`).
    Withheld,
    /// The agent is pending, or the hub has no record of it.
    NotApproved,
}

/// The hub's side of repository ownership. Made once at the hub's start,
/// which opens its start window.
pub struct RepositoryOwners {
    agents: Arc<dyn AgentRegistry>,
    owners: Arc<dyn OwnershipRegistry>,
    clock: Arc<dyn Clock>,
    /// The drive the registry is on, flushed before a new grant goes out.
    drive: Arc<dyn DriveFlush>,
    /// The hub's monotonic time at its start.
    started: SignedDuration,
    /// The start window, once [`open`](Self::open) or the first report
    /// since the start read it.
    window: Mutex<Option<StartWindow>>,
}

impl RepositoryOwners {
    pub fn new(
        agents: Arc<dyn AgentRegistry>,
        owners: Arc<dyn OwnershipRegistry>,
        clock: Arc<dyn Clock>,
        drive: Arc<dyn DriveFlush>,
    ) -> Self {
        Self {
            started: clock.monotonic(),
            agents,
            owners,
            clock,
            drive,
            window: Mutex::new(None),
        }
    }

    /// Reads the start window now, at the hub's start, so it waits for
    /// every agent approved before the start and none approved since.
    pub async fn open(&self) -> Result<(), StoreError> {
        Ok(())
    }

    /// Takes `reports` as every repository `agent` uses, with `proven`, the
    /// floors among them whose proof the hub checked, and decides each at
    /// the hub's now when the agent is approved.
    pub async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        proven: &[ProvenFloor],
    ) -> Result<Reported, StoreError> {
        let decided = self
            .decide(agent, reports, proven, self.clock.now())
            .await?;
        Ok(decided.map_or(Reported::NotApproved, Reported::Decided))
    }

    /// Decides `reports` as [`report`](Self::report) does, and gives every
    /// lease `agent` then holds and claimed (granted, renewed or left to
    /// run out), decided at the hub's now, for the hub to sign; none for an
    /// agent that isn't approved. A lease it holds on a repository it no
    /// longer claims is left out: it ends at the agent's release or its
    /// expiry. In recovery mode it gives nothing, and the agent keeps what
    /// it holds until it runs out (decision 137).
    pub async fn grant(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        proven: &[ProvenFloor],
    ) -> Result<Granting, StoreError> {
        let now = self.clock.now();
        let Some(decided) = self.decide(agent, reports, proven, now).await? else {
            return Ok(Granting::NotApproved);
        };
        if self.recovering().await? {
            return Ok(Granting::Withheld);
        }
        let leases = decided
            .into_iter()
            .filter_map(|decision| match decision.outcome {
                ClaimOutcome::Granted(lease)
                | ClaimOutcome::Renewed(lease)
                | ClaimOutcome::Unrenewed(lease) => Some((decision.repository, lease)),
                ClaimOutcome::Refused(_) | ClaimOutcome::NoClaim => None,
            })
            .collect();
        Ok(Granting::Leases {
            generation: GENERATION,
            signed_at: now,
            leases,
        })
    }

    /// Whether the hub is in recovery mode (decision 137).
    pub async fn recovering(&self) -> Result<bool, StoreError> {
        Ok(self.owners.recovery().await?.is_on())
    }

    /// Ends recovery mode, at a person's confirmation that the old hub is
    /// stopped or gone: each counter rises to the floor proved for it, and
    /// a lease still held at it is its holder's again. Whether the hub was
    /// in recovery mode.
    pub async fn end_recovery(&self) -> Result<bool, StoreError> {
        self.owners.end_recovery().await
    }

    /// What the hub decided of `reports` at `now`, when `agent` is approved.
    async fn decide(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        proven: &[ProvenFloor],
        now: domain::Timestamp,
    ) -> Result<Option<Vec<ClaimDecision>>, StoreError> {
        match self.agents.agent(agent).await? {
            Some(record) if !record.is_pending() => {
                let window = self.window(agent).await?;
                let decided = self
                    .owners
                    .report(agent, reports, proven, now, &window)
                    .await?;
                self.heard(agent);
                Ok(Some(decided))
            }
            _ => Ok(None),
        }
    }

    /// The start window as `agent`'s report finds it: read from the
    /// registry at the first report since the start, holding nothing once
    /// its time has passed, and waiting for `agent` no more in this copy
    /// only. The kept window stops waiting for it once its report is
    /// stored, so a report that fails, or is dropped, opens nothing.
    async fn window(&self, agent: AgentId) -> Result<StartWindow, StoreError> {
        let read = if self.kept_window().is_none() {
            Some(StartWindow::new(self.owners.reported_uses().await?, []))
        } else {
            None
        };
        let since_start = self.clock.monotonic().saturating_sub(self.started);
        let mut window = {
            let mut kept = self.kept_window();
            let window = kept.get_or_insert_with(|| read.unwrap_or_default());
            window.pass(since_start);
            window.clone()
        };
        window.heard(agent);
        Ok(window)
    }

    /// `agent`'s report is stored: the kept window waits for it no more.
    fn heard(&self, agent: AgentId) {
        if let Some(window) = self.kept_window().as_mut() {
            window.heard(agent);
        }
    }

    fn kept_window(&self) -> MutexGuard<'_, Option<StartWindow>> {
        self.window.lock().unwrap_or_else(PoisonError::into_inner)
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
