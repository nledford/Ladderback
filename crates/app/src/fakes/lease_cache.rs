use std::sync::Mutex;

use async_trait::async_trait;
use domain::maintenance::HeldLeases;

use crate::ports::{LeaseCache, StoreError};

/// A machine's lease cache in memory, for use-case tests and acceptance
/// scenarios: one instance stands for one database.
#[derive(Debug, Default)]
pub struct InMemoryLeaseCache {
    kept: Mutex<HeldLeases>,
}

#[async_trait]
impl LeaseCache for InMemoryLeaseCache {
    async fn load(&self) -> Result<HeldLeases, StoreError> {
        let _ = &self.kept;
        todo!()
    }

    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        let _ = leases;
        todo!()
    }
}
