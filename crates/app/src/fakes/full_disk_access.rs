use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::execution::full_disk_access::FullDiskAccess;

use crate::ports::FullDiskAccessProbe;

/// A Full Disk Access probe that finds what it was told, and counts how
/// often it was run.
pub struct FakeFullDiskAccessProbe {
    found: Mutex<FullDiskAccess>,
    asked: AtomicUsize,
}

impl FakeFullDiskAccessProbe {
    /// A probe that finds `found` until told otherwise.
    pub fn new(found: FullDiskAccess) -> Self {
        Self {
            found: Mutex::new(found),
            asked: AtomicUsize::new(0),
        }
    }

    /// What every later probe finds.
    pub fn set(&self, found: FullDiskAccess) {
        *self.found.lock().unwrap_or_else(PoisonError::into_inner) = found;
    }

    /// How many times the probe ran.
    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl FullDiskAccessProbe for FakeFullDiskAccessProbe {
    async fn probe(&self) -> FullDiskAccess {
        self.asked.fetch_add(1, Ordering::SeqCst);
        *self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
