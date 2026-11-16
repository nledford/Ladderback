//! The Full Disk Access probe (technical design §3.1, §16 decision NN): on
//! macOS, whether the person has granted the agent Full Disk Access, learned
//! by listing a folder only that grant opens.

use async_trait::async_trait;
use domain::execution::full_disk_access::FullDiskAccess;

/// Lists a folder only a Full Disk Access grant opens.
#[async_trait]
pub trait FullDiskAccessProbe: Send + Sync {
    /// [`FullDiskAccess::Granted`] when the folder lists,
    /// [`FullDiskAccess::Missing`] when the system refuses it for want of the
    /// grant, and [`FullDiskAccess::Unknown`] for any other answer: a folder
    /// that isn't there, or one refused for another reason. An answer that
    /// can't be had is unknown, never an error.
    async fn probe(&self) -> FullDiskAccess;
}
