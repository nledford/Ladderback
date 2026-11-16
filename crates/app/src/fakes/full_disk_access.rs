use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        let _ = (
            found,
            Mutex::new(()),
            AtomicUsize::new(0),
            Ordering::Relaxed,
        );
        todo!()
    }

    /// What every later probe finds.
    pub fn set(&self, found: FullDiskAccess) {
        let _ = found;
        todo!()
    }

    /// How many times the probe ran.
    pub fn asked(&self) -> usize {
        todo!()
    }
}

#[async_trait]
impl FullDiskAccessProbe for FakeFullDiskAccessProbe {
    async fn probe(&self) -> FullDiskAccess {
        let _ = (&self.found, &self.asked);
        todo!()
    }
}
