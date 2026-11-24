//! A paired machine's own leases (technical design §7.3, I1; §16, decision
//! 151): [`HeldLeases`], the leases its hub granted it and the highest lease
//! it has seen for each repository, its epoch floor. A paired machine runs
//! a forget, a prune, a check or an adoption only while it holds a live
//! lease on the repository. Whether one is live is judged on the hub's
//! time: each grant carries the hub's clock when it was signed, and the
//! lease's end is moved onto the machine's own wall clock by the time
//! between the two, so a machine whose clock runs days off stops when the
//! hub counts the lease out, and time asleep still counts. Rules only:
//! every time is passed in. While it holds no live lease on a repository it
//! asks to maintain, its maintenance of it is paused ([`Pause`]), and a
//! pause that has lasted [`PAUSE_ALERT_AFTER`] is told once.

use std::collections::BTreeMap;

use super::lease::LeaseEpoch;
use crate::{RepositoryId, SignedDuration, Timestamp};

/// How long a paired machine's maintenance of a repository stays paused
/// before the machine tells its notification rules
/// (`repo.maintenance_paused`, §11): 14 days, two lease lives, so a laptop
/// away from its hub for a week or a hub down a few days stays quiet.
pub const PAUSE_ALERT_AFTER: SignedDuration = SignedDuration::from_hours(14 * 24);

/// The machine's maintenance of a repository, paused for want of a lease:
/// since when, and whether its notification rules were told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pause {
    pub since: Timestamp,
    pub alerted: bool,
}

/// The highest lease a machine has seen for one repository, ordered by the
/// hub's recovery number, its generation, then the lease's epoch (§7.3, "A
/// rewound hub"). A grant below it is refused (`epoch-lower`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpochFloor {
    pub recovery_number: u64,
    pub generation: u64,
    pub epoch: LeaseEpoch,
}

/// One lease in the hub's grants: until `expires_at` on the hub's clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub repository: RepositoryId,
    pub epoch: LeaseEpoch,
    pub expires_at: Timestamp,
}

/// The hub's signed grants to one machine (`RepoGrants`, §7.2): every lease
/// the hub counts it as holding, at the hub's `generation`, with the hub's
/// clock when it signed them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoGrants {
    pub generation: u64,
    pub signed_at: Timestamp,
    pub grants: Vec<Grant>,
}

/// A lease the machine holds: its epoch, and when it runs out on the
/// machine's own wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldLease {
    pub epoch: LeaseEpoch,
    pub ends_at: Timestamp,
}

/// Why the machine may not maintain a repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoLease {
    /// The hub granted it no lease on the repository, or none the machine
    /// took.
    NoneHeld,
    /// Its lease ran out on the hub's time.
    RanOut,
}

/// A grant refused because it lies below the epoch floor (`epoch-lower`):
/// the hub was rewound, or restored from an older copy. The machine holds
/// no lease on the repository until a grant at or above the floor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochLower {
    pub repository: RepositoryId,
    pub floor: EpochFloor,
    pub granted: EpochFloor,
}

/// A paired machine's leases, epoch floors and paused maintenance.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldLeases {
    leases: BTreeMap<RepositoryId, HeldLease>,
    floors: BTreeMap<RepositoryId, EpochFloor>,
    paused: BTreeMap<RepositoryId, Pause>,
}

impl HeldLeases {
    /// The leases and floors a cache kept.
    pub fn from_parts(
        leases: impl IntoIterator<Item = (RepositoryId, HeldLease)>,
        floors: impl IntoIterator<Item = (RepositoryId, EpochFloor)>,
    ) -> Self {
        Self {
            leases: leases.into_iter().collect(),
            floors: floors.into_iter().collect(),
            paused: BTreeMap::new(),
        }
    }

    /// `self` with the pauses a cache kept in place of its own.
    #[must_use]
    pub fn with_pauses(self, pauses: impl IntoIterator<Item = (RepositoryId, Pause)>) -> Self {
        Self {
            paused: pauses.into_iter().collect(),
            ..self
        }
    }

    pub fn pauses(&self) -> impl Iterator<Item = (&RepositoryId, &Pause)> {
        self.paused.iter()
    }

    /// Since when the machine's maintenance of `repository` is paused at
    /// `now`, on its own clock: never while it holds a live lease on it;
    /// else since the pause it noted, or, when it noted none yet, since its
    /// lease ran out. `None` when it holds no lease and noted no pause: the
    /// next [`watch`](Self::watch) notes one.
    pub fn paused_since(&self, repository: &RepositoryId, now: Timestamp) -> Option<Timestamp> {
        if self.holds(repository, now).is_ok() {
            return None;
        }
        self.paused
            .get(repository)
            .map(|pause| pause.since)
            .or_else(|| self.leases.get(repository).map(|lease| lease.ends_at))
    }

    /// Notes at `now` the pause of each repository of `maintained`, those
    /// the machine asks to maintain, on which it holds no live lease: since
    /// its lease ran out, or since `now` when it holds none, a pause noted
    /// before keeping its date. Every other pause is dropped, so the next
    /// grant clears one. Returns each pause a watch before this one noted
    /// that has now lasted [`PAUSE_ALERT_AFTER`] and wasn't
    /// [`told`](Self::told) yet, by repository and since when. A pause
    /// this watch first notes isn't returned, however old: a machine that
    /// starts or wakes long after its lease ran out asks its hub before the
    /// next watch, and the hub's grant may end the pause first.
    pub fn watch<'a>(
        &mut self,
        maintained: impl IntoIterator<Item = &'a RepositoryId>,
        now: Timestamp,
    ) -> Vec<(RepositoryId, Timestamp)> {
        let mut paused = BTreeMap::new();
        let mut due = Vec::new();
        for repository in maintained {
            if self.holds(repository, now).is_ok() {
                continue;
            }
            let kept = self.paused.get(repository).copied();
            let pause = kept.unwrap_or(Pause {
                since: self
                    .leases
                    .get(repository)
                    .map_or(now, |lease| lease.ends_at),
                alerted: false,
            });
            // A date no clock can hold is never due.
            let lasted = pause
                .since
                .checked_add(PAUSE_ALERT_AFTER)
                .is_ok_and(|at| now >= at);
            if kept.is_some() && lasted && !pause.alerted {
                due.push((repository.clone(), pause.since));
            }
            paused.insert(repository.clone(), pause);
        }
        self.paused = paused;
        due
    }

    /// Notes that the notification rules were told of the pause of
    /// `repository` since `since`, so no watch returns it again. A pause
    /// that ended, or one since another date, is left as it is.
    pub fn told(&mut self, repository: &RepositoryId, since: Timestamp) {
        if let Some(pause) = self
            .paused
            .get_mut(repository)
            .filter(|pause| pause.since == since)
        {
            pause.alerted = true;
        }
    }

    pub fn leases(&self) -> impl Iterator<Item = (&RepositoryId, &HeldLease)> {
        self.leases.iter()
    }

    pub fn floors(&self) -> impl Iterator<Item = (&RepositoryId, &EpochFloor)> {
        self.floors.iter()
    }

    /// Takes `grants`, the hub's answer to a request the machine sent at
    /// `asked_at` on its own clock, under the hub's recovery number
    /// `recovery_number`, as every lease it holds: a lease it held and the
    /// grants don't name is let go. Each grant's end is moved onto the
    /// machine's clock from `asked_at` by the time between the hub's
    /// signing and its expiry; the hub signed after the request left, so
    /// the machine's view of the hub's time runs ahead of it, never behind.
    /// A grant below the repository's floor is refused and returned, and
    /// the machine holds no lease on that repository; the others raise it.
    pub fn take(
        &mut self,
        grants: &RepoGrants,
        recovery_number: u64,
        asked_at: Timestamp,
    ) -> Vec<EpochLower> {
        let mut refused = Vec::new();
        let mut leases = BTreeMap::new();
        for grant in &grants.grants {
            let granted = EpochFloor {
                recovery_number,
                generation: grants.generation,
                epoch: grant.epoch,
            };
            let floor = self
                .floors
                .entry(grant.repository.clone())
                .or_insert(granted);
            if granted < *floor {
                refused.push(EpochLower {
                    repository: grant.repository.clone(),
                    floor: *floor,
                    granted,
                });
                continue;
            }
            *floor = granted;
            // A life no clock can hold comes from no hub this version
            // speaks to: the machine holds nothing on it.
            let life = grants.signed_at.duration_until(grant.expires_at);
            let Ok(ends_at) = asked_at.checked_add(life) else {
                continue;
            };
            leases.insert(
                grant.repository.clone(),
                HeldLease {
                    epoch: grant.epoch,
                    ends_at,
                },
            );
        }
        self.leases = leases;
        refused
    }

    /// The epoch of the lease the machine holds on `repository` at `now`,
    /// on its own clock.
    pub fn holds(&self, repository: &RepositoryId, now: Timestamp) -> Result<LeaseEpoch, NoLease> {
        let lease = self.leases.get(repository).ok_or(NoLease::NoneHeld)?;
        if now < lease.ends_at {
            Ok(lease.epoch)
        } else {
            Err(NoLease::RanOut)
        }
    }

    /// Lets go of the lease on `repository` at `epoch`, once the hub has
    /// its release (`ReleaseAck`); the floor stays.
    pub fn released(&mut self, repository: &RepositoryId, epoch: LeaseEpoch) {
        if self
            .leases
            .get(repository)
            .is_some_and(|lease| lease.epoch == epoch)
        {
            self.leases.remove(repository);
        }
    }
}

#[cfg(test)]
mod tests;
