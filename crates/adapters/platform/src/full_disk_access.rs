//! The Full Disk Access probe (technical design §3.1, §16 decision NN): on
//! macOS, whether the person has granted the agent Full Disk Access, learned
//! by listing a folder only that grant opens, `~/Library/Safari`.

use std::io;
use std::path::PathBuf;

use app::ports::FullDiskAccessProbe;
use async_trait::async_trait;
use domain::execution::full_disk_access::FullDiskAccess;

/// Lists one folder: a listing is the grant, a refusal with `EPERM` its
/// absence, and any other answer unknown.
pub struct FolderProbe {
    folder: PathBuf,
}

impl FolderProbe {
    pub fn new(folder: PathBuf) -> Self {
        let _ = folder;
        todo!()
    }
}

/// What listing the folder answered says of the grant.
fn found_from(listed: io::Result<()>) -> FullDiskAccess {
    let _ = listed;
    todo!()
}

#[async_trait]
impl FullDiskAccessProbe for FolderProbe {
    async fn probe(&self) -> FullDiskAccess {
        let _ = (&self.folder, found_from);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;

    use app::contract;
    use app::contract::full_disk_access::FullDiskAccessHarness;

    use super::*;

    struct Folders(tempfile::TempDir);

    impl Folders {
        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    impl FullDiskAccessHarness for Folders {
        fn folder_lists(&self) -> Arc<dyn FullDiskAccessProbe> {
            let folder = self.path().join("Safari");
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("History.db"), b"demo").unwrap();
            Arc::new(FolderProbe::new(folder))
        }

        fn folder_missing(&self) -> Arc<dyn FullDiskAccessProbe> {
            Arc::new(FolderProbe::new(self.path().join("missing")))
        }

        fn not_a_folder(&self) -> Arc<dyn FullDiskAccessProbe> {
            let file = self.path().join("a-file");
            fs::write(&file, b"demo").unwrap();
            Arc::new(FolderProbe::new(file))
        }
    }

    #[tokio::test]
    async fn the_folder_probe_meets_the_full_disk_access_contract() {
        contract::full_disk_access::run(&Folders(tempfile::tempdir().unwrap())).await;
    }

    #[tokio::test]
    async fn a_folder_its_permissions_refuse_says_nothing_of_the_grant() {
        let root = tempfile::tempdir().unwrap();
        let closed = root.path().join("closed");
        fs::create_dir(&closed).unwrap();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
        let lists_anyway = fs::read_dir(&closed).is_ok();
        let probe = FolderProbe::new(closed.clone());
        if lists_anyway {
            fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
            testkit::skip!("running as root: permissions don't stop listing a folder");
        }
        contract::full_disk_access::a_folder_its_permissions_refuse_is_unknown(&probe).await;
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
    }

    /// A test machine can't make the system refuse a folder for want of the
    /// grant, so the answer it gives then, `EPERM`, is given here.
    #[test]
    fn a_refusal_with_eperm_is_the_grant_missing() {
        assert_eq!(
            found_from(Err(io::Error::from_raw_os_error(1))),
            FullDiskAccess::Missing
        );
    }

    #[test]
    fn a_listing_is_the_grant_and_any_other_refusal_unknown() {
        assert_eq!(found_from(Ok(())), FullDiskAccess::Granted);
        for errno in [2, 13, 20] {
            assert_eq!(
                found_from(Err(io::Error::from_raw_os_error(errno))),
                FullDiskAccess::Unknown,
                "os error {errno}"
            );
        }
    }
}
