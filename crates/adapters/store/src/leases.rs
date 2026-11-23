//! A paired machine's leases from its hub and its epoch floors (technical
//! design §7.3, I1; migration `0024_agent_leases`): the `LeaseCache` port on
//! the store's own file, built only with the hub (decision 122).

use app::ports::{LeaseCache, StoreError};
use async_trait::async_trait;
use domain::maintenance::HeldLeases;

use crate::SqliteStore;

#[async_trait]
impl LeaseCache for SqliteStore {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        todo!()
    }

    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        let _ = leases;
        todo!()
    }
}
