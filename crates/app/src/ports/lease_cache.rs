use async_trait::async_trait;
use domain::maintenance::HeldLeases;

use super::StoreError;

/// Where a paired machine keeps the leases its hub granted it and its epoch
/// floors (technical design §7.3, I1), in its database, so they outlive a
/// restart: a lease still holds while the hub can't be reached, and a floor
/// still refuses a rewound hub's grant.
#[async_trait]
pub trait LeaseCache: Send + Sync {
    /// What was last kept: none held and no floor before anything was.
    async fn load(&self) -> Result<HeldLeases, StoreError>;

    /// Keeps `leases` in place of what was kept, in one step.
    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError>;
}
