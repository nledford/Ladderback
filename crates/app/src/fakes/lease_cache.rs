use std::sync::{Mutex, PoisonError};

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
        Ok(self
            .kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    async fn save(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        *self.kept.lock().unwrap_or_else(PoisonError::into_inner) = leases.clone();
        Ok(())
    }

    async fn save_pauses(&self, leases: &HeldLeases) -> Result<(), StoreError> {
        todo!("{leases:?}")
    }
}
