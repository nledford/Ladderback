//! A restored hub's recovery mode (technical design §7.3, "A rewound hub";
//! §16, decision 137): [`HubRecovery`], the epoch floors machines proved
//! above the hub's own counters. A hub restored from a copy can count fewer
//! epochs than it gave, so a machine may hold a lease the hub doesn't know
//! of. Each machine reports its floor with the hub-signed lease entry that
//! set it; once one proves a floor above what the hub counts, the hub grants
//! and renews nothing until a person confirms the old hub is stopped or
//! gone. The confirmation raises each counter to the floor proved for it and
//! gives a held lease back to its holder. Rules only: the hub checks the
//! signatures before a floor reaches here.

use std::collections::BTreeMap;

use super::lease::{ClaimOutcome, ClaimRefused, Lease, LeaseEpoch, RepositoryOwnership};
use crate::fleet::Signature;
use crate::planning::Maintenance;
use crate::{AgentId, RepositoryId, Timestamp};

/// What a machine reports of its epoch floor on one repository: the epoch,
/// and the expiry and hub signature of the lease entry that set it, its
/// proof; `held` while the machine still holds that lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportedFloor {
    pub epoch: LeaseEpoch,
    /// The entry's expiry on the hub's clock, in whole seconds as signed.
    pub expires_at: Timestamp,
    pub signature: Signature,
    pub held: bool,
}

/// A floor whose proof the hub checked: its own signature over a lease it
/// granted `holder` on `repository`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenFloor {
    pub repository: RepositoryId,
    pub holder: AgentId,
    pub epoch: LeaseEpoch,
    pub expires_at: Timestamp,
    pub held: bool,
}

/// The floors a restored hub heard above its counters, at most one per
/// repository: the highest. The hub is in recovery mode while it holds any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HubRecovery {
    floors: BTreeMap<RepositoryId, ProvenFloor>,
}

impl HubRecovery {
    /// The floors a store kept.
    pub fn from_floors(floors: impl IntoIterator<Item = ProvenFloor>) -> Self {
        Self {
            floors: floors
                .into_iter()
                .map(|floor| (floor.repository.clone(), floor))
                .collect(),
        }
    }

    pub fn floors(&self) -> impl Iterator<Item = &ProvenFloor> {
        self.floors.values()
    }

    /// Whether the hub is in recovery mode: it grants and renews nothing.
    pub fn is_on(&self) -> bool {
        !self.floors.is_empty()
    }

    /// Hears `floor`, proved for the repository `ownership` records: kept
    /// when it lies above the hub's counter, or while the hub is already in
    /// recovery mode, unless a higher floor is kept for the repository.
    /// Its holder's newer report at the same epoch replaces its own. Whether
    /// anything changed.
    pub fn hear(&mut self, ownership: &RepositoryOwnership, floor: &ProvenFloor) -> bool {
        if !self.is_on() && !ownership.is_behind(floor) {
            return false;
        }
        let stands = self.floors.get(&floor.repository).is_some_and(|kept| {
            kept.epoch > floor.epoch
                || (kept.epoch == floor.epoch && (kept.holder != floor.holder || kept == floor))
        });
        if stands {
            return false;
        }
        self.floors.insert(floor.repository.clone(), floor.clone());
        true
    }

    /// Decides a report asking for `request` while the hub is in recovery
    /// mode: a claim (`local`) is refused, and nothing is renewed.
    pub fn decide(&self, request: Maintenance) -> ClaimOutcome {
        if request == Maintenance::Local {
            ClaimOutcome::Refused(ClaimRefused::Recovering)
        } else {
            ClaimOutcome::NoClaim
        }
    }
}

impl RepositoryOwnership {
    /// Whether `floor` lies above the epochs this hub counts as given.
    pub fn is_behind(&self, floor: &ProvenFloor) -> bool {
        self.last_epoch.is_none_or(|last| last < floor.epoch)
    }

    /// Takes `floor` once a person ends recovery mode: the counter rises to
    /// it, a lease below it is dropped, and a lease its holder still holds
    /// at it is the hub's again. A counter past the floor stands.
    pub fn recover(&mut self, floor: &ProvenFloor) {
        if !self.is_behind(floor) {
            return;
        }
        self.last_epoch = Some(floor.epoch);
        self.lease = floor.held.then_some(Lease {
            holder: floor.holder,
            epoch: floor.epoch,
            expires_at: floor.expires_at,
        });
    }
}

#[cfg(test)]
mod tests;
