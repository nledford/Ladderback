//! A paired machine's own leases (technical design §7.3, I1; §16, decision
//! NN): [`HeldLeases`], the leases its hub granted it and the highest lease
//! it has seen for each repository, its epoch floor. A paired machine runs
//! a forget, a prune, a check or an adoption only while it holds a live
//! lease on the repository. Whether one is live is judged on the hub's
//! time: each grant carries the hub's clock when it was signed, and the
//! lease's end is moved onto the machine's own wall clock by the time
//! between the two, so a machine whose clock runs days off stops when the
//! hub counts the lease out, and time asleep still counts. Rules only:
//! every time is passed in.

use std::collections::BTreeMap;

use super::lease::LeaseEpoch;
use crate::{RepositoryId, Timestamp};

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

/// A paired machine's leases and epoch floors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldLeases {
    leases: BTreeMap<RepositoryId, HeldLease>,
    floors: BTreeMap<RepositoryId, EpochFloor>,
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
        let _ = (grants, recovery_number, asked_at);
        todo!()
    }

    /// The epoch of the lease the machine holds on `repository` at `now`,
    /// on its own clock.
    pub fn holds(&self, repository: &RepositoryId, now: Timestamp) -> Result<LeaseEpoch, NoLease> {
        let _ = (repository, now);
        todo!()
    }

    /// Lets go of the lease on `repository` at `epoch`, once the hub has
    /// its release (`ReleaseAck`); the floor stays.
    pub fn released(&mut self, repository: &RepositoryId, epoch: LeaseEpoch) {
        let _ = (repository, epoch);
        todo!()
    }
}

#[cfg(test)]
mod tests;
