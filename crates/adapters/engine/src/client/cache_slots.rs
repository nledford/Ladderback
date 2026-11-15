//! The cache slots of the running workers: each worker of a repository gets
//! a cache directory no other worker uses.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use domain::RepositoryId;

use super::MAX_CACHE_SLOTS;
use crate::cache_lock::CacheLock;

/// The cache directories of the running workers. Two workers of one
/// repository must not share a cache: rustic's cache isn't safe for
/// concurrent writers, so each gets the lowest slot nobody else uses.
///
/// The slots this daemon hands out are remembered here. A worker that
/// outlived a daemon that was restarted is not: it holds a lock on its
/// directory (see [`CacheLock`]), and its slot is skipped.
#[derive(Default)]
pub(super) struct CacheSlots {
    in_use: Mutex<HashMap<String, BTreeSet<u32>>>,
}

impl CacheSlots {
    /// `None` when every slot is taken: that worker runs without a cache.
    pub(super) fn acquire(
        self: &Arc<Self>,
        root: &Path,
        repository: &RepositoryId,
    ) -> Option<CacheSlot> {
        let directory = |number: u32| root.join(repository.as_str()).join(number.to_string());
        let mut in_use = self.in_use.lock().unwrap_or_else(PoisonError::into_inner);
        let taken = in_use.entry(repository.as_str().to_owned()).or_default();
        let number = (0..MAX_CACHE_SLOTS)
            .find(|number| !taken.contains(number) && !held_by_another(&directory(*number)))?;
        taken.insert(number);
        Some(CacheSlot {
            slots: Arc::clone(self),
            repository: repository.as_str().to_owned(),
            number,
            dir: directory(number),
        })
    }
}

/// Whether a worker that isn't one of this daemon's holds `dir`. A directory
/// that can't be looked at counts as free: the worker finds out for itself,
/// and runs without a cache if it must.
fn held_by_another(dir: &Path) -> bool {
    matches!(
        CacheLock::acquire(dir),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    )
}

/// A cache directory in use by one worker; free again once dropped.
pub(super) struct CacheSlot {
    slots: Arc<CacheSlots>,
    repository: String,
    number: u32,
    pub(super) dir: PathBuf,
}

impl Drop for CacheSlot {
    fn drop(&mut self) {
        let mut in_use = self
            .slots
            .in_use
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(taken) = in_use.get_mut(&self.repository) {
            taken.remove(&self.number);
            if taken.is_empty() {
                in_use.remove(&self.repository);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::client::test_support::{cache_dir, repository};

    #[test]
    fn each_worker_of_a_repository_gets_the_lowest_free_cache_slot() {
        let slots = Arc::new(CacheSlots::default());
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let id = repository("ab");
        let first = slots.acquire(root, &id).unwrap();
        let second = slots.acquire(root, &id).unwrap();
        let third = slots.acquire(root, &id).unwrap();
        assert_eq!(
            [&first.dir, &second.dir, &third.dir],
            [
                &cache_dir(root, &id, 0),
                &cache_dir(root, &id, 1),
                &cache_dir(root, &id, 2)
            ]
        );
        drop(second);
        let reused = slots.acquire(root, &id).unwrap();
        assert_eq!(reused.dir, cache_dir(root, &id, 1));
        let other = slots.acquire(root, &repository("cd")).unwrap();
        assert_eq!(other.dir.file_name().unwrap(), "0");
        drop((first, third, reused, other));
        assert!(slots.in_use.lock().unwrap().is_empty());
    }

    /// A worker outlives a daemon that was restarted: the new daemon knows
    /// nothing of the old worker's slot, but the old worker still holds a
    /// lock on its directory.
    #[test]
    fn a_slot_whose_directory_another_worker_has_locked_is_skipped() {
        let slots = Arc::new(CacheSlots::default());
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let id = repository("ab");
        std::fs::create_dir_all(cache_dir(root, &id, 0)).unwrap();
        let old_worker = std::fs::File::open(cache_dir(root, &id, 0)).unwrap();
        old_worker.try_lock().unwrap();
        let slot = slots.acquire(root, &id).unwrap();
        assert_eq!(slot.dir, cache_dir(root, &id, 1));
        // Once that worker is gone, its slot is free again. A process that a
        // test running in parallel forked meanwhile holds the lock until it
        // has executed its program, so give it a moment.
        drop(old_worker);
        let freed = (0..200).any(|_| {
            let next = slots.acquire(root, &id).unwrap();
            let freed = next.dir == cache_dir(root, &id, 0);
            drop(next);
            if !freed {
                std::thread::sleep(Duration::from_millis(10));
            }
            freed
        });
        assert!(freed, "the slot of a worker that is gone stayed taken");
    }

    /// A user who clears their caches while the daemon runs removes the
    /// cache folder too: the next job's slot makes it again, and every
    /// folder on the way, for the daemon's user alone.
    #[cfg(unix)]
    #[test]
    fn a_cache_folder_removed_while_the_daemon_runs_is_made_again_for_its_user_alone() {
        use std::os::unix::fs::PermissionsExt;

        let mode = |dir: &Path| std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        let slots = Arc::new(CacheSlots::default());
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join("cache");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let id = repository("ab");
        drop(slots.acquire(&root, &id).unwrap());

        std::fs::remove_dir_all(&root).unwrap();
        let slot = slots.acquire(&root, &id).unwrap();
        assert_eq!(slot.dir, cache_dir(&root, &id, 0));
        for dir in [root.clone(), root.join(id.as_str()), slot.dir.clone()] {
            assert_eq!(mode(&dir), 0o700, "{}", dir.display());
        }
    }
}
