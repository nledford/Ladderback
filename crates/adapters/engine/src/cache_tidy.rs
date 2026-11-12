//! Tidying the workers' cache folder when the daemon starts (technical
//! design §4.1, §16 decision 61).
//!
//! Nothing in the cache is needed, but nothing else removes it: a repository
//! no entry records any more keeps its folder, a moment of many workers at
//! once leaves as many cache slots behind, and a worker killed in the middle
//! of a write leaves rustic's unfinished file. Tidying removes those, and
//! only those: a folder named exactly as a repository ID, a cache slot in it
//! named by its number, and in a slot whatever it holds. Anything else in the
//! cache folder is left alone, a link is never followed, and a slot is only
//! touched while this process holds its lock, so a running worker's slot
//! never is.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use domain::RepositoryId;

use crate::worker::tmp_files::STALE_AFTER;

/// What tidying the cache folder removed, and what it couldn't.
#[derive(Debug, Default)]
pub struct CacheTidying {
    /// What was removed, by repository, in the order of their IDs.
    pub removed: Vec<Removed>,
    /// What was to be removed and couldn't be, with the OS's reason.
    pub failed: Vec<NotRemoved>,
}

/// Something removed from one repository's folder of the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub repository: RepositoryId,
    pub what: RemovedKind,
    /// How many cache slots or files.
    pub count: u64,
    /// The size of the files removed, links not followed.
    pub bytes: u64,
}

/// Why something was removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovedKind {
    /// The cache slots of a repository no entry records, and its folder
    /// once nothing else is left in it.
    Unrecorded,
    /// Cache slots above the highest one in use.
    SpareSlots,
    /// Files rustic was writing when its worker died.
    UnfinishedFiles,
}

/// A file or folder tidying couldn't remove, or a folder it couldn't read.
#[derive(Debug)]
pub struct NotRemoved {
    pub path: PathBuf,
    pub error: io::Error,
}

/// The repositories the configuration's entries record, as far as tidying
/// can tell which folders of the cache are theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// Every entry records its repository's ID, and these are those IDs.
    Exactly(BTreeSet<RepositoryId>),
    /// An entry records no ID yet: any repository's folder may be its
    /// repository's, until its next job records the ID.
    NotAllKnown,
}

/// Tidies the cache folder `root`, where `recorded` are the repositories
/// the configuration's entries record and `now` is the time it is:
///
/// - a repository no entry records loses every cache slot no worker holds,
///   and then its folder if that is empty; while an entry records no ID,
///   no repository counts as one no entry records;
/// - every other repository loses the slots above the highest one in use:
///   a slot another process's worker holds, and slot 0, which a lone job
///   always uses;
/// - every slot a repository keeps loses the unfinished files rustic left
///   (`<id>-tmp-`) more than a day old.
pub fn tidy_cache(root: &Path, recorded: &Recorded, now: SystemTime) -> CacheTidying {
    let _ = (root, recorded, now);
    todo!("tidy the cache folder")
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::time::Duration;

    use super::*;

    const DAY: Duration = STALE_AFTER;

    fn id(byte: &str) -> RepositoryId {
        RepositoryId::parse(&byte.repeat(32)).unwrap()
    }

    fn recorded(ids: &[&RepositoryId]) -> Recorded {
        Recorded::Exactly(ids.iter().map(|id| (*id).clone()).collect())
    }

    fn slot(root: &Path, id: &RepositoryId, number: u32) -> PathBuf {
        root.join(id.as_str()).join(number.to_string())
    }

    /// Writes `bytes` bytes at `path`, making the folders above it.
    fn file(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7_u8; bytes]).unwrap();
    }

    /// Fills `dir` as rustic fills a slot: its tag, and an index file of
    /// `bytes` bytes under the repository's ID. Returns the bytes written.
    fn cache(dir: &Path, bytes: usize) -> u64 {
        file(&dir.join("CACHEDIR.TAG"), 43);
        file(
            &dir.join("ab".repeat(32))
                .join("index")
                .join("cd")
                .join("cd".repeat(32)),
            bytes,
        );
        u64::try_from(43 + bytes).unwrap()
    }

    /// An unfinished file rustic left in the slot `dir`, last written `age`
    /// before `now`, of `bytes` bytes.
    fn unfinished(dir: &Path, name: &str, bytes: usize, now: SystemTime, age: Duration) -> PathBuf {
        let path = dir
            .join("ab".repeat(32))
            .join("snapshots")
            .join(format!("{}-tmp-", name.repeat(32)));
        file(&path, bytes);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(now - age)
            .unwrap();
        path
    }

    /// Holds `dir`'s lock as another daemon's worker would. The lock belongs
    /// to the open file, so tidying's own open of the folder finds it held.
    fn other_worker_holds(dir: &Path) -> File {
        let folder = File::open(dir).unwrap();
        folder.try_lock().unwrap();
        folder
    }

    #[test]
    fn the_cache_of_a_repository_no_entry_records_is_removed_with_its_size() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let gone = id("0f");
        let bytes = cache(&slot(root, &gone, 0), 1000) + cache(&slot(root, &gone, 1), 500);
        let kept = id("1e");
        cache(&slot(root, &kept, 0), 10);

        let tidying = tidy_cache(root, &recorded(&[&kept]), SystemTime::now());

        assert!(!root.join(gone.as_str()).exists(), "the folder is gone");
        assert!(slot(root, &kept, 0).join("CACHEDIR.TAG").exists());
        assert_eq!(
            tidying.removed,
            [Removed {
                repository: gone,
                what: RemovedKind::Unrecorded,
                count: 2,
                bytes,
            }]
        );
        assert!(tidying.failed.is_empty(), "{:?}", tidying.failed);
    }

    #[test]
    fn while_an_entry_records_no_id_every_repository_keeps_its_folder() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let now = SystemTime::now();
        let unknown = id("0f");
        cache(&slot(root, &unknown, 0), 1000);
        let old = unfinished(&slot(root, &unknown, 0), "aa", 70, now, DAY * 2);
        let spare = cache(&slot(root, &unknown, 1), 500);

        let tidying = tidy_cache(root, &Recorded::NotAllKnown, now);

        assert!(slot(root, &unknown, 0).join("CACHEDIR.TAG").exists());
        assert!(!slot(root, &unknown, 1).exists(), "a spare slot still goes");
        assert!(!old.exists(), "an old unfinished file still goes");
        assert_eq!(
            tidying.removed,
            [
                Removed {
                    repository: unknown.clone(),
                    what: RemovedKind::SpareSlots,
                    count: 1,
                    bytes: spare,
                },
                Removed {
                    repository: unknown,
                    what: RemovedKind::UnfinishedFiles,
                    count: 1,
                    bytes: 70,
                },
            ]
        );
        assert!(tidying.failed.is_empty(), "{:?}", tidying.failed);
    }

    #[test]
    fn slots_above_slot_0_are_spare_when_no_worker_holds_one() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let repository = id("2d");
        cache(&slot(root, &repository, 0), 10);
        let bytes = cache(&slot(root, &repository, 1), 20) + cache(&slot(root, &repository, 7), 30);

        let tidying = tidy_cache(root, &recorded(&[&repository]), SystemTime::now());

        assert!(slot(root, &repository, 0).join("CACHEDIR.TAG").exists());
        assert!(!slot(root, &repository, 1).exists());
        assert!(!slot(root, &repository, 7).exists());
        assert_eq!(
            tidying.removed,
            [Removed {
                repository,
                what: RemovedKind::SpareSlots,
                count: 2,
                bytes,
            }]
        );
    }

    #[test]
    fn a_recorded_repository_with_one_slot_loses_nothing() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let repository = id("3c");
        cache(&slot(root, &repository, 0), 10);

        let tidying = tidy_cache(root, &recorded(&[&repository]), SystemTime::now());

        assert!(slot(root, &repository, 0).join("CACHEDIR.TAG").exists());
        assert!(tidying.removed.is_empty(), "{:?}", tidying.removed);
    }

    /// A worker that outlived a restarted daemon, or one of another daemon
    /// sharing the cache folder, holds its slot: that slot and every one
    /// below it are in use, and only those above are spare.
    #[test]
    fn a_slot_another_worker_holds_is_kept_with_every_slot_below_it() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let now = SystemTime::now();
        let repository = id("4b");
        for number in 0..=3 {
            cache(&slot(root, &repository, number), 10);
        }
        let left = unfinished(&slot(root, &repository, 2), "ee", 5, now, 2 * DAY);
        let _worker = other_worker_holds(&slot(root, &repository, 2));

        let tidying = tidy_cache(root, &recorded(&[&repository]), now);

        for number in 0..=2 {
            assert!(
                slot(root, &repository, number)
                    .join("CACHEDIR.TAG")
                    .exists(),
                "slot {number} is kept"
            );
        }
        assert!(left.exists(), "the held slot is not touched");
        assert!(!slot(root, &repository, 3).exists(), "slot 3 is spare");
        assert_eq!(
            tidying
                .removed
                .iter()
                .map(|removed| (removed.what, removed.count))
                .collect::<Vec<_>>(),
            [(RemovedKind::SpareSlots, 1)]
        );
    }

    #[test]
    fn a_slot_another_worker_holds_keeps_the_folder_of_a_repository_no_entry_records() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let gone = id("5a");
        let bytes = cache(&slot(root, &gone, 0), 10);
        cache(&slot(root, &gone, 1), 10);
        let _worker = other_worker_holds(&slot(root, &gone, 1));

        let tidying = tidy_cache(root, &recorded(&[]), SystemTime::now());

        assert!(!slot(root, &gone, 0).exists());
        assert!(slot(root, &gone, 1).join("CACHEDIR.TAG").exists());
        assert_eq!(
            tidying.removed,
            [Removed {
                repository: gone,
                what: RemovedKind::Unrecorded,
                count: 1,
                bytes,
            }]
        );
        assert!(tidying.failed.is_empty(), "{:?}", tidying.failed);
    }

    /// A slot whose lock can't be looked at may be a running worker's: it
    /// counts as in use with every slot below it, and the reason is noted.
    #[cfg(unix)]
    #[test]
    fn a_slot_that_cannot_be_looked_at_counts_as_in_use() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let repository = id("78");
        for number in 0..=3 {
            cache(&slot(root, &repository, number), 10);
        }
        let closed = slot(root, &repository, 2);
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
        let opens_anyway = File::open(&closed).is_ok();

        let tidying = tidy_cache(root, &recorded(&[&repository]), SystemTime::now());

        fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
        if opens_anyway {
            testkit::skip!("running as root: permissions don't stop opening a folder");
        }
        assert!(closed.join("CACHEDIR.TAG").exists(), "slot 2 is kept");
        for number in 0..=1 {
            assert!(
                slot(root, &repository, number)
                    .join("CACHEDIR.TAG")
                    .exists(),
                "slot {number} is kept"
            );
        }
        assert!(!slot(root, &repository, 3).exists(), "slot 3 is spare");
        assert_eq!(
            tidying
                .failed
                .iter()
                .map(|failed| (&failed.path, failed.error.kind()))
                .collect::<Vec<_>>(),
            [(&closed, io::ErrorKind::PermissionDenied)]
        );
    }

    #[test]
    fn unfinished_files_over_a_day_old_are_removed_from_the_slots_kept() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let now = SystemTime::now();
        let repository = id("69");
        let dir = slot(root, &repository, 0);
        cache(&dir, 10);
        let old = unfinished(&dir, "a1", 300, now, DAY + Duration::from_secs(1));
        let older = unfinished(&dir, "a2", 200, now, 30 * DAY);
        let recent = unfinished(&dir, "a3", 100, now, DAY - Duration::from_secs(60));
        let ahead = unfinished(&dir, "a4", 100, now, Duration::ZERO);
        File::options()
            .write(true)
            .open(&ahead)
            .unwrap()
            .set_modified(now + DAY)
            .unwrap();

        let tidying = tidy_cache(root, &recorded(&[&repository]), now);

        assert!(!old.exists() && !older.exists(), "the old ones are gone");
        assert!(recent.exists(), "one written within the day is kept");
        assert!(ahead.exists(), "one dated ahead of now is kept");
        assert!(dir.join("CACHEDIR.TAG").exists());
        assert_eq!(
            tidying.removed,
            [Removed {
                repository,
                what: RemovedKind::UnfinishedFiles,
                count: 2,
                bytes: 500,
            }]
        );
    }

    /// The folder a daemon is given may be wider than its own, as when
    /// `LADDERBACK_CACHE_DIR` names a folder of the user's: only folders
    /// named exactly as a repository ID, and slots named exactly by their
    /// number, are Ladderback's.
    #[test]
    fn what_is_not_named_as_ladderbacks_is_left_alone() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let upper = root.join("0F".repeat(32));
        cache(&upper.join("0"), 10);
        let short = root.join("0f".repeat(31));
        cache(&short.join("0"), 10);
        let notes = root.join("notes");
        cache(&notes.join("0"), 10);
        file(&root.join("1e".repeat(32)), 10);
        let gone = id("2d");
        let odd: Vec<PathBuf> = ["00", "01", "64", "+1", "x", "4294967296"]
            .iter()
            .map(|name| root.join(gone.as_str()).join(name))
            .collect();
        for dir in &odd {
            cache(dir, 10);
        }
        let stray = root.join(gone.as_str()).join("readme");
        file(&stray, 10);
        let repository = id("3c");
        cache(&slot(root, &repository, 0), 10);
        // Not a name rustic gives an unfinished file: `<64 hex digits>-tmp-`.
        let not_rustics = slot(root, &repository, 0).join("x-tmp-");
        file(&not_rustics, 10);
        File::options()
            .write(true)
            .open(&not_rustics)
            .unwrap()
            .set_modified(SystemTime::now() - 2 * DAY)
            .unwrap();

        let tidying = tidy_cache(root, &recorded(&[&repository]), SystemTime::now());

        for dir in [&upper, &short, &notes].into_iter().chain(&odd) {
            assert!(dir.exists(), "{} is left alone", dir.display());
        }
        assert!(root.join("1e".repeat(32)).is_file());
        assert!(stray.is_file());
        assert!(not_rustics.exists());
        assert!(tidying.removed.is_empty(), "{:?}", tidying.removed);
        assert!(tidying.failed.is_empty(), "{:?}", tidying.failed);
    }

    /// Deleting is the one thing here that can do harm: a link in the
    /// cache folder, wherever it is and whatever it is named, is never
    /// followed, and what it points to outside the folder is kept.
    #[cfg(unix)]
    #[test]
    fn a_link_is_never_followed_out_of_the_cache_folder() {
        use std::os::unix::fs::symlink;

        use nix::fcntl::AT_FDCWD;
        use nix::sys::stat::{UtimensatFlags, utimensat};
        use nix::sys::time::TimeSpec;

        let scratch = tempfile::tempdir().unwrap();
        let old = SystemTime::now() - 2 * DAY;
        let outside = scratch.path().join("outside");
        cache(&outside.join("0"), 100);
        // Named and dated as an unfinished file tidying would remove.
        let outside_unfinished = outside.join(format!("{}-tmp-", "ab".repeat(32)));
        file(&outside_unfinished, 10);
        File::options()
            .write(true)
            .open(&outside_unfinished)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let before = files(&outside);
        let root = scratch.path().join("cache");
        fs::create_dir(&root).unwrap();
        // A repository no entry records, as a link to the outside folder.
        symlink(&outside, root.join("0f".repeat(32))).unwrap();
        // A recorded repository whose folder is a link.
        let linked = id("1e");
        symlink(&outside, root.join(linked.as_str())).unwrap();
        // A recorded repository with a link named as a slot above its spare
        // slot (taken for a slot, its lock would be refused, a link not
        // being the folder, and the spare slot would stay), a spare slot
        // that holds a link, and a kept slot with links in it: one to a
        // folder, and one named and dated, itself, as an unfinished file.
        let repository = id("2d");
        cache(&slot(&root, &repository, 0), 10);
        symlink(&outside, slot(&root, &repository, 0).join("away")).unwrap();
        let linked_unfinished =
            slot(&root, &repository, 0).join(format!("{}-tmp-", "cd".repeat(32)));
        symlink(&outside_unfinished, &linked_unfinished).unwrap();
        let old_since_epoch = TimeSpec::from(old.duration_since(SystemTime::UNIX_EPOCH).unwrap());
        utimensat(
            AT_FDCWD,
            &linked_unfinished,
            &old_since_epoch,
            &old_since_epoch,
            UtimensatFlags::NoFollowSymlink,
        )
        .unwrap();
        symlink(&outside, slot(&root, &repository, 7)).unwrap();
        let spare = slot(&root, &repository, 6);
        let spare_bytes = cache(&spare, 10);
        symlink(&outside, spare.join("away")).unwrap();
        // An unrecorded repository whose slot holds a link.
        let gone = id("3c");
        let gone_bytes = cache(&slot(&root, &gone, 0), 10);
        symlink(&outside, slot(&root, &gone, 0).join("away")).unwrap();

        let tidying = tidy_cache(&root, &recorded(&[&linked, &repository]), SystemTime::now());

        assert_eq!(files(&outside), before, "nothing outside was touched");
        assert!(!spare.exists() && !root.join(gone.as_str()).exists());
        assert_eq!(
            tidying.removed,
            [
                Removed {
                    repository: repository.clone(),
                    what: RemovedKind::SpareSlots,
                    count: 1,
                    bytes: spare_bytes,
                },
                Removed {
                    repository: gone,
                    what: RemovedKind::Unrecorded,
                    count: 1,
                    bytes: gone_bytes,
                },
            ]
        );
        for link in [
            root.join("0f".repeat(32)),
            root.join(linked.as_str()),
            slot(&root, &repository, 7),
            slot(&root, &repository, 0).join("away"),
            linked_unfinished,
        ] {
            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the link {} is left alone",
                link.display()
            );
        }
    }

    /// Every file under `dir`, with its size and last write.
    #[cfg(unix)]
    fn files(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
        let mut found = Vec::new();
        let mut folders = vec![dir.to_owned()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(&folder).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if metadata.is_dir() {
                    folders.push(entry.path());
                } else {
                    found.push((entry.path(), metadata.len(), metadata.modified().unwrap()));
                }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn a_cache_folder_that_cannot_be_read_is_a_failure_and_nothing_else() {
        let scratch = tempfile::tempdir().unwrap();
        let missing = scratch.path().join("missing");

        let tidying = tidy_cache(&missing, &recorded(&[]), SystemTime::now());

        assert!(tidying.removed.is_empty());
        assert_eq!(
            tidying
                .failed
                .iter()
                .map(|failed| (&failed.path, failed.error.kind()))
                .collect::<Vec<_>>(),
            [(&missing, io::ErrorKind::NotFound)]
        );
    }
}
