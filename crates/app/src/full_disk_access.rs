//! Full Disk Access on macOS (technical design §3.1, §16 decision 115): the
//! agent runs its Full Disk Access probe when it starts, before each backup
//! and when a person asks it to check again, and keeps what the last probe
//! found, which the start page reads. A backup takes the probe's answer from
//! just before it ([`crate::execution::RunOperation::with_full_disk_access`]),
//! so a grant given since the start counts from the next backup on.

use std::sync::{Arc, Mutex, PoisonError};

use domain::execution::full_disk_access::FullDiskAccess;

use crate::ports::FullDiskAccessProbe;

/// What the agent's log says of Full Disk Access when it starts: one line,
/// and only for a grant found missing. An unknown answer says nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartNote {
    FullDiskAccessMissing,
}

/// The agent's Full Disk Access probe, and what it found last.
pub struct FullDiskAccessCheck {
    probe: Arc<dyn FullDiskAccessProbe>,
    found: Mutex<Option<FullDiskAccess>>,
}

impl FullDiskAccessCheck {
    pub fn new(probe: Arc<dyn FullDiskAccessProbe>) -> Self {
        Self {
            probe,
            found: Mutex::new(None),
        }
    }

    /// Runs the probe as the agent starts, and keeps what it found: the line
    /// the log then says, if any.
    pub async fn at_start(&self) -> Option<StartNote> {
        match self.check().await {
            FullDiskAccess::Missing => Some(StartNote::FullDiskAccessMissing),
            FullDiskAccess::Granted | FullDiskAccess::Unknown => None,
        }
    }

    /// Runs the probe now, before a backup or when a person asks to check
    /// again, and keeps what it found.
    pub async fn check(&self) -> FullDiskAccess {
        let found = self.probe.probe().await;
        *self.found.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
        found
    }

    /// What the last probe found; `None` before the first.
    pub fn found(&self) -> Option<FullDiskAccess> {
        *self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
