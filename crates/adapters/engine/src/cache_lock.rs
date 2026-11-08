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
    /// Makes `dir` if it doesn't exist and locks it. Fails with
    /// [`io::ErrorKind::WouldBlock`] when another process holds the lock.
    pub(crate) fn acquire(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let directory = File::open(dir)?;
        match directory.try_lock() {
            Ok(()) => Ok(Self {
                _directory: directory,
            }),
            Err(TryLockError::WouldBlock) => Err(io::ErrorKind::WouldBlock.into()),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
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
        let dir = root.path().join("steading").join("ab").join("0");
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
}
