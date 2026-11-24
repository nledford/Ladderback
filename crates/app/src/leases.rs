//! A paired machine's side of repository ownership (technical design §7.3,
//! I1; §16, decision 151): the leases its hub grants it, kept in its
//! database, and whether it may maintain a repository now. A paired machine
//! runs a forget, a prune, a check or an adoption only while it holds a
//! live lease on the repository, judged on the hub's time, from the moment
//! it is paired: one that hasn't heard a grant yet, or can't reach its hub
//! once its lease ran out, maintains nothing. A machine that isn't paired
//! maintains what it asks for, as before. Backups and previews never ask.

use std::sync::{Arc, Mutex, PoisonError};

use domain::maintenance::{EpochLower, LeaseEpoch, NoLease, RepoGrants};
use domain::{RepositoryId, SignedDuration, Timestamp};

use crate::notifications::{Notify, Report};
use crate::ports::{Clock, ConfigStore, Identity, LeaseCache, StoreError};

/// The machine's leases.
pub struct MaintenanceLeases {
    identity: Arc<dyn Identity>,
    cache: Arc<dyn LeaseCache>,
    clock: Arc<dyn Clock>,
    /// When this process last took its hub's grants, on the machine's wall
    /// clock and on its monotonic clock.
    taken: Mutex<Option<(Timestamp, SignedDuration)>>,
}

impl MaintenanceLeases {
    pub fn new(
        identity: Arc<dyn Identity>,
        cache: Arc<dyn LeaseCache>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            identity,
            cache,
            clock,
            taken: Mutex::new(None),
        }
    }

    /// The machine's time a lease is judged at: its wall clock, but never
    /// less than the monotonic time that passed since this process last
    /// took its hub's grants, so a wall clock set back since doesn't
    /// lengthen a lease (I1). One set back while the daemon was stopped
    /// is caught only once a grant is taken again.
    fn now(&self) -> Timestamp {
        let wall = self.clock.now();
        let taken = *self.taken.lock().unwrap_or_else(PoisonError::into_inner);
        taken
            .and_then(|(at, monotonic)| at.checked_add(self.clock.monotonic() - monotonic).ok())
            .map_or(wall, |counted| wall.max(counted))
    }

    /// Whether this machine may maintain `repository`, known by its ID when
    /// its entry recorded one, now: always when it isn't paired with a hub;
    /// when it is, only while it holds a live lease on it. A repository
    /// whose ID isn't known can't be granted one, and a pin or a cache that
    /// can't be read refuses, as no lease would.
    pub async fn permits(&self, repository: Option<&RepositoryId>) -> Result<(), NoLease> {
        if let Ok(None) = self.identity.pin().await {
            return Ok(());
        }
        let repository = repository.ok_or(NoLease::NoneHeld)?;
        let held = self.cache.load().await.map_err(|_| NoLease::NoneHeld)?;
        held.holds(repository, self.now()).map(|_| ())
    }

    /// Takes the hub's `grants`, its answer to a request the machine sent
    /// at `asked_at` on its own clock under the hub's `recovery_number`, as
    /// every lease the machine holds, and keeps them. The grants refused as
    /// below a floor are returned.
    pub async fn take(
        &self,
        grants: &RepoGrants,
        recovery_number: u64,
        asked_at: Timestamp,
    ) -> Result<Vec<EpochLower>, StoreError> {
        let monotonic = self.clock.monotonic();
        let mut held = self.cache.load().await?;
        let refused = held.take(grants, recovery_number, asked_at);
        self.cache.save(&held).await?;
        *self.taken.lock().unwrap_or_else(PoisonError::into_inner) = Some((asked_at, monotonic));
        Ok(refused)
    }

    /// Every lease the machine holds, live or not, by repository.
    pub async fn held(&self) -> Result<Vec<(RepositoryId, LeaseEpoch)>, StoreError> {
        Ok(self
            .cache
            .load()
            .await?
            .leases()
            .map(|(repository, lease)| (repository.clone(), lease.epoch))
            .collect())
    }

    /// Lets go of the lease on `repository` at `epoch`, once the hub has the
    /// machine's release.
    pub async fn released(
        &self,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<(), StoreError> {
        let mut held = self.cache.load().await?;
        held.released(repository, epoch);
        self.cache.save(&held).await
    }

    /// Since when this machine's maintenance of `repository`, known by its
    /// ID when its entry recorded one, is paused for want of a lease, now
    /// (decision 158): `None` while it isn't paired, holds a live lease on
    /// it, or hasn't noted the pause yet. A repository whose ID isn't
    /// known, a pin or a cache that can't be read says none: the page
    /// shows nothing it can't tell, and a run is refused regardless.
    pub async fn paused(&self, repository: Option<&RepositoryId>) -> Option<Timestamp> {
        todo!("{repository:?}")
    }

    /// Notes, now, which of `maintained`, the repositories the
    /// configuration asks this machine to maintain, are paused for want of
    /// a lease, and returns each pause that has lasted
    /// [`PAUSE_ALERT_AFTER`](domain::maintenance::PAUSE_ALERT_AFTER) and
    /// wasn't told yet, by repository and since when, noted as told. A
    /// machine that isn't paired drops every pause; one whose pin can't be
    /// read notes nothing. Only the pauses are saved, so a grant taken
    /// meanwhile stands (I1).
    pub async fn watch(
        &self,
        maintained: &[RepositoryId],
    ) -> Result<Vec<(RepositoryId, Timestamp)>, StoreError> {
        todo!("{maintained:?}")
    }
}

/// Tells the notification rules, once, of each repository whose
/// maintenance on this machine has been paused for want of a lease for
/// [`PAUSE_ALERT_AFTER`](domain::maintenance::PAUSE_ALERT_AFTER), with
/// `repo.maintenance_paused` (§11, decision 158). The daemon watches every
/// 15 minutes, as it asks its hub for grants.
pub struct WatchPauses {
    config: Arc<dyn ConfigStore>,
    leases: Arc<MaintenanceLeases>,
    notify: Arc<Notify>,
}

impl WatchPauses {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        leases: Arc<MaintenanceLeases>,
        notify: Arc<Notify>,
    ) -> Self {
        Self {
            config,
            leases,
            notify,
        }
    }

    /// Notes the pause of each repository this machine is asked to
    /// maintain and knows the ID of, and tells the rules of each that has
    /// lasted long enough. Returns what telling them left to report. A
    /// configuration or a cache that can't be read notes nothing and tells
    /// nothing; the next watch tries again.
    pub async fn watch(&self) -> Vec<Report> {
        let _ = (&self.config, &self.leases, &self.notify);
        todo!()
    }
}

#[cfg(test)]
mod tests;
