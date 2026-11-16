use std::sync::Arc;

use domain::execution::full_disk_access::FullDiskAccess;

use crate::ports::FullDiskAccessProbe;

/// Sets up a Full Disk Access probe in a known state: a fake told what to
/// find, or the real adapter pointed at a folder the harness made. Whether
/// the system refuses a folder for want of the grant can't be arranged on a
/// test machine, so the real adapter tests that answer on its own.
pub trait FullDiskAccessHarness: Send + Sync {
    /// A probe whose folder lists.
    fn folder_lists(&self) -> Arc<dyn FullDiskAccessProbe>;
    /// A probe whose folder isn't there.
    fn folder_missing(&self) -> Arc<dyn FullDiskAccessProbe>;
    /// A probe whose folder is a file.
    fn not_a_folder(&self) -> Arc<dyn FullDiskAccessProbe>;
}

pub async fn run(harness: &dyn FullDiskAccessHarness) {
    a_folder_that_lists_is_the_grant(harness).await;
    a_folder_that_is_not_there_is_unknown(harness).await;
    a_file_in_the_folder_s_place_is_unknown(harness).await;
}

async fn a_folder_that_lists_is_the_grant(harness: &dyn FullDiskAccessHarness) {
    assert_eq!(
        harness.folder_lists().probe().await,
        FullDiskAccess::Granted
    );
}

async fn a_folder_that_is_not_there_is_unknown(harness: &dyn FullDiskAccessHarness) {
    assert_eq!(
        harness.folder_missing().probe().await,
        FullDiskAccess::Unknown,
        "a folder that isn't there says nothing of the grant; never a false alarm"
    );
}

async fn a_file_in_the_folder_s_place_is_unknown(harness: &dyn FullDiskAccessHarness) {
    assert_eq!(
        harness.not_a_folder().probe().await,
        FullDiskAccess::Unknown
    );
}

/// A probe whose folder its own permissions refuse (`EACCES`), which no
/// grant opens, finds the grant unknown, never missing. Run apart from
/// [`run`]: a harness can make such a folder only where permissions hold,
/// not as root.
pub async fn a_folder_its_permissions_refuse_is_unknown(probe: &dyn FullDiskAccessProbe) {
    assert_eq!(probe.probe().await, FullDiskAccess::Unknown);
}
