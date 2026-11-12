//! The hold a worker keeps on its cache directory for as long as it runs.
//!
//! rustic's cache has no locking of its own, so no two workers may share a
//! directory (technical design §4.1). The daemon remembers which slots its
//! own workers use, but a worker can outlive a daemon that was restarted.
//! That worker keeps an exclusive lock on its directory, and a daemon looking
//! for a free slot skips the ones whose lock is held.

use std::fs::{self, File, TryLockError};
use std::io;
use std::path::Path;

/// An exclusive advisory lock on a directory. The lock belongs to the open
/// file, so it ends when this is dropped or the process dies, however it
/// dies. Where a directory can't be opened as a file the lock fails, and the
/// worker runs without a cache.
#[derive(Debug)]
pub(crate) struct CacheLock {
    _directory: File,
}

impl CacheLock {
    /// Makes `dir`, and every folder missing above it, if it doesn't exist
    /// (see [`create_private_dir`]) and locks it. Fails with
    /// [`io::ErrorKind::WouldBlock`] when another process holds the lock.
    pub(crate) fn acquire(dir: &Path) -> io::Result<Self> {
        create_private_dir(dir)?;
        Self::hold(dir)
    }

    /// Locks `dir`, which must be there already: nothing is made. Fails with
    /// [`io::ErrorKind::WouldBlock`] when another process holds the lock.
    pub(crate) fn hold(dir: &Path) -> io::Result<Self> {
        Self::lock(dir, File::open(dir)?)
    }

    /// Locks `directory`, the folder opened at `dir`.
    fn lock(dir: &Path, directory: File) -> io::Result<Self> {
        match directory.try_lock() {
            Ok(()) => Ok(Self {
                _directory: directory,
            }),
            Err(TryLockError::WouldBlock) => Err(io::ErrorKind::WouldBlock.into()),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

/// Makes `dir`, and every folder missing above it, readable and writable by
/// the daemon's user alone (mode 0700): rustic's files then sit under a
/// private folder even when the cache root is wider, as a systemd
/// `CacheDirectory=` is (0755). A folder already there is left as it is.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Acquires `dir`, allowing for a lock that was just let go of. A process
    /// another thread forked meanwhile holds the lock until it has executed
    /// its program, and tests run in parallel.
    fn acquire_soon(dir: &Path) -> CacheLock {
        for _ in 0..200 {
            match CacheLock::acquire(dir) {
                Ok(lock) => return lock,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        }
        panic!("the lock on {} stayed held", dir.display());
    }

    #[test]
    fn a_directory_is_held_by_one_lock_at_a_time() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ladderback").join("ab").join("0");
        let first = CacheLock::acquire(&dir).unwrap();
        assert!(dir.is_dir(), "the directory is made");
        let busy = CacheLock::acquire(&dir).expect_err("a second lock on a held directory");
        assert_eq!(busy.kind(), io::ErrorKind::WouldBlock);
        drop(first);
        acquire_soon(&dir);
    }

    #[test]
    fn locks_on_different_directories_do_not_meet() {
        let root = tempfile::tempdir().unwrap();
        let _first = CacheLock::acquire(&root.path().join("0")).unwrap();
        let _second = CacheLock::acquire(&root.path().join("1")).unwrap();
    }

    /// Another process may remove a folder while it holds its lock, as
    /// tidying the cache removes a spare slot, after this one opened the
    /// folder and before it locks it. The lock would then be on a folder no
    /// path names, and the folder made again at the path nobody's: it is
    /// refused as held, and the folder made again is free.
    #[test]
    fn a_folder_removed_after_it_was_opened_is_not_held() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("0");
        fs::create_dir(&dir).unwrap();
        let opened = File::open(&dir).unwrap();
        fs::remove_dir(&dir).unwrap();

        let gone = CacheLock::lock(&dir, opened).expect_err("the folder is gone");

        assert_eq!(gone.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn a_folder_made_again_after_the_old_one_was_opened_is_not_held_through_it() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("0");
        fs::create_dir(&dir).unwrap();
        let opened = File::open(&dir).unwrap();
        fs::remove_dir(&dir).unwrap();
        fs::create_dir(&dir).unwrap();

        let moved = CacheLock::lock(&dir, opened).expect_err("another folder is at the path");

        assert_eq!(moved.kind(), io::ErrorKind::WouldBlock);
        CacheLock::hold(&dir).expect("the folder made again is free");
    }

    /// A link put where the folder was is not the folder, even one to it.
    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_it_was_opened_is_not_held() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("0");
        let elsewhere = root.path().join("elsewhere");
        fs::create_dir(&dir).unwrap();
        let opened = File::open(&dir).unwrap();
        fs::rename(&dir, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &dir).unwrap();

        let swapped = CacheLock::lock(&dir, opened).expect_err("a link is at the path");

        assert_eq!(swapped.kind(), io::ErrorKind::WouldBlock);
    }

    #[cfg(unix)]
    #[test]
    fn the_folders_a_lock_makes_are_the_owners_alone_under_a_wider_root() {
        use std::os::unix::fs::PermissionsExt;

        let mode = |dir: &Path| fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let repository = root.path().join("ab");
        let slot = repository.join("0");
        let _lock = CacheLock::acquire(&slot).unwrap();
        assert_eq!(mode(&repository), 0o700, "the repository's folder");
        assert_eq!(mode(&slot), 0o700, "the slot");
        assert_eq!(mode(root.path()), 0o755, "the root is left as it was");
    }
}
