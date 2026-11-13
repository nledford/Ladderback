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
//! It is the lock itself, so deleting it while a daemon serves would let a second
//! daemon take a fresh one.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
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
        let mut file = open(&data_dir.join(LOCK_FILE)).map_err(unavailable)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::InUse {
                    holder: holder_named_in(&mut file),
                });
            }
            Err(TryLockError::Error(error)) => return Err(unavailable(error)),
        }
        record_this_process(&mut file).map_err(unavailable)?;
        Ok(Self { _file: file })
    }
}

/// The most of the lock file that is read for a process ID: an ID is a short
/// number, and what the file holds is for another process to write.
const MOST_OF_AN_ID: u64 = 32;

fn unavailable(error: io::Error) -> LockError {
    LockError::Unavailable {
        detail: error.to_string(),
    }
}

/// Opens the lock file, making it when it isn't there. It is not truncated:
/// a process that is refused the lock must leave the holder's ID where it is.
fn open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The process ID the lock file names, when it names one.
fn holder_named_in(file: &mut File) -> Option<u32> {
    let mut text = String::new();
    Read::take(&mut *file, MOST_OF_AN_ID)
        .read_to_string(&mut text)
        .ok()?;
    text.trim().parse().ok()
}

/// Replaces what the lock file says with this process's ID.
fn record_this_process(file: &mut File) -> io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(file, "{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
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
        assert_matches!(second, LockError::InUse { .. });
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
        assert_matches!(error, LockError::Unavailable { .. });
        assert!(!missing.exists(), "locking doesn't make the data dir");
    }

    #[cfg(unix)]
    #[test]
    fn the_lock_file_is_the_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let _lock = DataDirLock::acquire(dir.path()).unwrap();
        let mode = fs::metadata(lock_file(dir.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the mode is {mode:o}");
    }
}
