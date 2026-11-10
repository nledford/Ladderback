//! The lock that keeps one process on a data dir (technical design §9, "One
//! process per data dir").
//!
//! SQLite's own locking lets several processes write one database, so without
//! this two daemons would each run a scheduler over one operation log, and
//! each would take the other's running operations for the leftovers of a
//! crash when it recovered them. The daemon takes an exclusive `flock` on
//! `<data>/ladderback.lock` before it opens the database, and keeps it until
//! it exits. The operating system lets go of the lock when the process ends,
//! however it ends, so a crash never leaves it stale. The file stays, naming
//! the process that held it, until the next holder writes its own ID into it.

use std::fs::File;
use std::path::Path;

/// The lock file's name inside the data dir.
pub const LOCK_FILE: &str = "ladderback.lock";

/// An exclusive hold on a data dir, kept for as long as this value lives.
#[derive(Debug)]
pub struct DataDirLock {
    _file: File,
}

/// Why a data dir could not be locked.
#[derive(Debug, PartialEq, Eq)]
pub enum LockError {
    /// Another process holds the lock. `holder` is its process ID, when the
    /// lock file names one.
    InUse { holder: Option<u32> },
    /// The lock file could not be opened, locked or written. `detail` is the
    /// operating system's own message.
    Unavailable { detail: String },
}

impl DataDirLock {
    /// Locks `data_dir`, which must exist, and records this process's ID in
    /// the lock file for whoever is refused next.
    pub fn acquire(data_dir: &Path) -> Result<Self, LockError> {
        let _ = data_dir;
        todo!("DataDirLock::acquire")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn lock_file(dir: &Path) -> PathBuf {
        dir.join(LOCK_FILE)
    }

    #[test]
    fn a_data_dir_is_held_by_one_process_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let _first = DataDirLock::acquire(dir.path()).unwrap();
        let second =
            DataDirLock::acquire(dir.path()).expect_err("a second lock on a held data dir");
        assert!(matches!(second, LockError::InUse { .. }), "{second:?}");
    }

    #[test]
    fn the_holder_is_named_by_its_process_id() {
        let dir = tempfile::tempdir().unwrap();
        let _first = DataDirLock::acquire(dir.path()).unwrap();
        let second = DataDirLock::acquire(dir.path()).expect_err("a held data dir");
        assert_eq!(
            second,
            LockError::InUse {
                holder: Some(std::process::id())
            }
        );
    }

    #[test]
    fn a_refused_attempt_leaves_the_holders_process_id_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let _first = DataDirLock::acquire(dir.path()).unwrap();
        let _refused = DataDirLock::acquire(dir.path()).expect_err("a held data dir");
        let named = fs::read_to_string(lock_file(dir.path())).unwrap();
        assert_eq!(named.trim(), std::process::id().to_string());
    }

    #[test]
    fn the_lock_is_let_go_when_the_holder_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let first = DataDirLock::acquire(dir.path()).unwrap();
        drop(first);
        DataDirLock::acquire(dir.path()).expect("the data dir is free again");
    }

    #[test]
    fn a_lock_file_a_dead_holder_left_behind_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        // A process that was killed leaves its ID in the file and no lock.
        fs::write(lock_file(dir.path()), "4194999\n").unwrap();
        let _lock = DataDirLock::acquire(dir.path()).expect("a stale file holds nothing");
        let named = fs::read_to_string(lock_file(dir.path())).unwrap();
        assert_eq!(
            named.trim(),
            std::process::id().to_string(),
            "the new holder names itself"
        );
    }

    #[test]
    fn a_holder_that_named_no_process_is_still_refused() {
        let dir = tempfile::tempdir().unwrap();
        // Another process holds the lock the same way: through its own open
        // file. Nothing is written yet, as in the moment after it took it.
        let holder = File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_file(dir.path()))
            .unwrap();
        holder.try_lock().unwrap();
        let refused = DataDirLock::acquire(dir.path()).expect_err("a held data dir");
        assert_eq!(refused, LockError::InUse { holder: None });

        // Text that is not a process ID names nobody either.
        fs::write(lock_file(dir.path()), "not a process\n").unwrap();
        let refused = DataDirLock::acquire(dir.path()).expect_err("a held data dir");
        assert_eq!(refused, LockError::InUse { holder: None });
    }

    #[test]
    fn a_data_dir_that_does_not_exist_cannot_be_locked() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let error = DataDirLock::acquire(&missing).expect_err("no data dir");
        assert!(matches!(error, LockError::Unavailable { .. }), "{error:?}");
        assert!(!missing.exists(), "locking doesn't make the data dir");
    }
}
