use async_trait::async_trait;
use domain::maintenance::HeldLeases;

use super::StoreError;

/// Where a paired machine keeps the leases its hub granted it, its epoch
/// floors and its paused maintenance (technical design §7.3, I1), in its
/// database, so they outlive a restart: a lease still holds while the hub
/// can't be reached, a floor still refuses a rewound hub's grant, and a
/// pause keeps its date and whether it was told.
#[async_trait]
pub trait LeaseCache: Send + Sync {
    /// What was last kept: none held, no floor and no pause before anything
    /// was.
    async fn load(&self) -> Result<HeldLeases, StoreError>;

    /// Keeps the leases and floors of `leases` in place of those kept, in
    /// one step. The pauses kept stay as they were.
    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError>;

    /// Keeps the pauses of `leases` in place of those kept, in one step.
    /// The leases and floors kept stay as they were, so a watch of the
    /// pauses never writes back a lease a grant taken meanwhile let go
    /// (I1).
    async fn save_pauses(&self, leases: &HeldLeases) -> Result<(), StoreError>;
}
