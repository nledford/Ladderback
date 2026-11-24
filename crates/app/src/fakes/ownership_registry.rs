use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::maintenance::{
    LeaseEpoch, ReleaseRefused, RepositoryOwnership, RepositoryReport, RepositoryReports,
    StartWindow, report_counts,
};
use domain::{AgentId, RepositoryId, Timestamp};

use crate::ports::{ClaimDecision, OwnershipRegistry, StoreError};

/// The hub's registry of maintenance owners in memory, for use-case tests
/// and acceptance scenarios: one instance stands for one database. One lock
/// covers a whole report, so reports that race are decided one after the
/// other, as the store's transactions are.
#[derive(Debug, Default)]
pub struct InMemoryOwnershipRegistry {
    kept: Mutex<Kept>,
}

#[derive(Clone, Debug, Default)]
struct Kept {
    owners: BTreeMap<RepositoryId, RepositoryOwnership>,
    /// Each agent's last report, and when the hub took it.
    reports: BTreeMap<AgentId, (Vec<RepositoryReport>, Timestamp)>,
}

impl InMemoryOwnershipRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A copy of everything this registry holds now, as a backup of the
    /// hub's database holds it: restoring the backup is opening the copy.
    pub fn backup(&self) -> Self {
        Self {
            kept: Mutex::new(self.kept().clone()),
        }
    }
}

#[async_trait]
impl OwnershipRegistry for InMemoryOwnershipRegistry {
    async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        now: Timestamp,
        window: &StartWindow,
    ) -> Result<Vec<ClaimDecision>, StoreError> {
        let mut kept = self.kept();
        let Kept {
            owners,
            reports: taken,
        } = &mut *kept;
        taken.insert(agent, (reports.entries().to_vec(), now));
        let decided = reports
            .entries()
            .iter()
            .map(|entry| {
                let others_report = taken.iter().any(|(other, (entries, at))| {
                    *other != agent
                        && report_counts(*at, now)
                        && entries.iter().any(|e| e.repository == entry.repository)
                });
                let outcome = owners
                    .entry(entry.repository.clone())
                    .or_insert_with(|| RepositoryOwnership::new(entry.repository.clone()))
                    .claim_after_start(
                        window,
                        agent,
                        entry.request,
                        entry.shared(others_report),
                        now,
                    );
                ClaimDecision {
                    repository: entry.repository.clone(),
                    outcome,
                }
            })
            .collect();
        Ok(decided)
    }

    async fn reported_uses(&self) -> Result<Vec<(AgentId, RepositoryId)>, StoreError> {
        Ok(self
            .kept()
            .reports
            .iter()
            .flat_map(|(agent, (entries, _))| {
                entries
                    .iter()
                    .map(|entry| (*agent, entry.repository.clone()))
            })
            .collect())
    }

    async fn release(
        &self,
        agent: AgentId,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<Result<(), ReleaseRefused>, StoreError> {
        Ok(self
            .kept()
            .owners
            .get_mut(repository)
            .map_or(Err(ReleaseRefused), |owner| owner.release(agent, epoch)))
    }

    async fn ownership(
        &self,
        repository: &RepositoryId,
    ) -> Result<RepositoryOwnership, StoreError> {
        Ok(self
            .kept()
            .owners
            .get(repository)
            .cloned()
            .unwrap_or_else(|| RepositoryOwnership::new(repository.clone())))
    }
}
