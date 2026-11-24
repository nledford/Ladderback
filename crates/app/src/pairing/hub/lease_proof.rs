//! The hub's signature over each lease it grants, apart from the message
//! that carries it (`lease-entry`), and its check of the floors machines
//! show back with it (§7.3; §16, decision 137, "A report counts only with
//! its proof").

use domain::fleet::{GENERATION, SignedMessage};
use domain::maintenance::{Grant, Lease, ProvenFloor, RepoGrants, RepositoryReports};
use domain::{AgentId, RepositoryId, Timestamp};

use super::{HubKeys, logged};
use crate::pairing::wire::entry_expiry;
use crate::ports::{Signing, SigningError};

/// The hub's grants of `leases` to `agent` at `generation`, decided at
/// `signed_at` on its clock: each entry signed apart by the hub key, so the
/// agent can show it back as the proof of its floor.
pub fn signed_grants(
    signing: &dyn Signing,
    keys: &HubKeys,
    agent: AgentId,
    generation: u64,
    signed_at: Timestamp,
    leases: Vec<(RepositoryId, Lease)>,
) -> Result<RepoGrants, SigningError> {
    let grants = leases
        .into_iter()
        .map(|(repository, lease)| {
            let entry = SignedMessage::LeaseEntry {
                hub_key: keys.key,
                agent_id: agent,
                repository: &repository,
                generation,
                epoch: lease.epoch.get(),
                expires_at: entry_expiry(lease.expires_at),
            };
            let signature = signing.sign(&keys.seed, &entry.to_bytes())?;
            Ok(Grant {
                repository,
                epoch: lease.epoch,
                expires_at: lease.expires_at,
                signature,
            })
        })
        .collect::<Result<Vec<_>, SigningError>>()?;
    Ok(RepoGrants {
        generation,
        signed_at,
        grants,
    })
}

/// The floors in `agent`'s `reports` whose proof is a lease entry the hub
/// key `keys` holds signed for `agent` at the hub's generation. Each other
/// floor doesn't count, and is logged: a machine's word alone can't hold
/// the hub in recovery mode.
pub fn proven_floors(
    signing: &dyn Signing,
    keys: &HubKeys,
    agent: AgentId,
    reports: &RepositoryReports,
) -> Vec<ProvenFloor> {
    reports
        .entries()
        .iter()
        .filter_map(|report| {
            let floor = report.floor?;
            let entry = SignedMessage::LeaseEntry {
                hub_key: keys.key,
                agent_id: agent,
                repository: &report.repository,
                generation: GENERATION,
                epoch: floor.epoch.get(),
                expires_at: entry_expiry(floor.expires_at),
            };
            if signing.verify(&keys.key, &entry.to_bytes(), &floor.signature) {
                Some(ProvenFloor {
                    repository: report.repository.clone(),
                    holder: agent,
                    epoch: floor.epoch,
                    expires_at: floor.expires_at,
                    held: floor.held,
                })
            } else {
                log::warn!(
                    "{}",
                    logged::floor_unproven(agent, &report.repository, floor.epoch.get())
                );
                None
            }
        })
        .collect()
}
