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
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use domain::RepositoryId;

use crate::cache_lock::CacheLock;
use crate::client::MAX_CACHE_SLOTS;
use crate::worker::tmp_files::{STALE_AFTER, is_id_temporary};

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
///   a slot another process's worker holds, or one whose lock can't be
///   looked at, and slot 0, which a lone job always uses;
/// - every slot a repository keeps loses the unfinished files rustic left
///   (`<id>-tmp-`) more than a day old.
pub fn tidy_cache(root: &Path, recorded: &Recorded, now: SystemTime) -> CacheTidying {
    let mut tidying = CacheTidying::default();
    let mut repositories: Vec<(RepositoryId, PathBuf)> = entries(root, &mut tidying)
        .into_iter()
        .filter(is_folder)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let id = RepositoryId::parse(name).ok()?;
            // Only rustic's own form, the one slots are made under.
            (id.as_str() == name).then(|| (id, entry.path()))
        })
        .collect();
    repositories.sort();
    for (id, dir) in repositories {
        // An entry with no ID may be any repository's: keep its folder.
        let is_recorded = match recorded {
            Recorded::Exactly(ids) => ids.contains(&id),
            Recorded::NotAllKnown => true,
        };
        tidy_repository(&mut tidying, id, &dir, is_recorded, now);
    }
    tidying
}

/// A slot whose lock this process holds while it decides about the slot.
struct FreeSlot {
    number: u32,
    dir: PathBuf,
    _lock: CacheLock,
}

/// Tidies the folder `dir` of the repository `id`.
fn tidy_repository(
    tidying: &mut CacheTidying,
    id: RepositoryId,
    dir: &Path,
    is_recorded: bool,
    now: SystemTime,
) {
    let mut free = Vec::new();
    // Slot 0 is the one a lone job always takes.
    let mut highest_in_use = 0;
    for (number, slot) in slots(dir, tidying) {
        match CacheLock::hold(&slot) {
            Ok(lock) => free.push(FreeSlot {
                number,
                dir: slot,
                _lock: lock,
            }),
            // A running worker's slot, or one that may be.
            Err(error) => {
                if error.kind() != io::ErrorKind::WouldBlock {
                    tidying.failed.push(NotRemoved { path: slot, error });
                }
                highest_in_use = highest_in_use.max(number);
            }
        }
    }
    if !is_recorded {
        let (count, bytes) = remove_slots(free, tidying);
        let folder_removed = match fs::remove_dir(dir) {
            Ok(()) => true,
            // A slot that is kept, or something that isn't Ladderback's.
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => false,
            Err(error) => {
                tidying.failed.push(NotRemoved {
                    path: dir.to_owned(),
                    error,
                });
                false
            }
        };
        if count > 0 || folder_removed {
            tidying.removed.push(Removed {
                repository: id,
                what: RemovedKind::Unrecorded,
                count,
                bytes,
            });
        }
        return;
    }
    let (spare, kept): (Vec<_>, Vec<_>) = free
        .into_iter()
        .partition(|slot| slot.number > highest_in_use);
    let (count, bytes) = remove_slots(spare, tidying);
    if count > 0 {
        tidying.removed.push(Removed {
            repository: id.clone(),
            what: RemovedKind::SpareSlots,
            count,
            bytes,
        });
    }
    let (mut count, mut bytes) = (0_u64, 0_u64);
    for slot in kept {
        let (removed, size) = remove_unfinished(&slot.dir, now, tidying);
        count = count.saturating_add(removed);
        bytes = bytes.saturating_add(size);
    }
    if count > 0 {
        tidying.removed.push(Removed {
            repository: id,
            what: RemovedKind::UnfinishedFiles,
            count,
            bytes,
        });
    }
}

/// The cache slots in a repository's folder `dir`: real folders, not links,
/// named by their number as slots are made (`0` to `63`, no leading zero).
fn slots(dir: &Path, tidying: &mut CacheTidying) -> Vec<(u32, PathBuf)> {
    let mut slots: Vec<(u32, PathBuf)> = entries(dir, tidying)
        .into_iter()
        .filter(is_folder)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let number = name.parse::<u32>().ok()?;
            (number < MAX_CACHE_SLOTS && number.to_string() == name).then(|| (number, entry.path()))
        })
        .collect();
    slots.sort();
    slots
}

/// Removes `slots`, each while its lock is held, and returns how many were
/// removed and the size of their files.
fn remove_slots(slots: Vec<FreeSlot>, tidying: &mut CacheTidying) -> (u64, u64) {
    let (mut count, mut bytes) = (0_u64, 0_u64);
    for slot in slots {
        let size = size_of(&slot.dir);
        // `remove_dir_all` removes a link it meets, never what it points to.
        match fs::remove_dir_all(&slot.dir) {
            Ok(()) => {
                count = count.saturating_add(1);
                bytes = bytes.saturating_add(size);
            }
            Err(error) => tidying.failed.push(NotRemoved {
                path: slot.dir.clone(),
                error,
            }),
        }
    }
    (count, bytes)
}

/// Removes the unfinished files rustic left in the slot `dir` more than
/// [`STALE_AFTER`] before `now`, and returns how many and their size.
fn remove_unfinished(dir: &Path, now: SystemTime, tidying: &mut CacheTidying) -> (u64, u64) {
    let (mut count, mut bytes) = (0_u64, 0_u64);
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in entries(&folder, tidying) {
            if is_folder(&entry) {
                folders.push(entry.path());
                continue;
            }
            // A link named as an unfinished file is not one.
            let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
            if !is_file || !entry.file_name().to_str().is_some_and(is_id_temporary) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let age = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok());
            if !age.is_some_and(|age| age > STALE_AFTER) {
                continue;
            }
            let path = entry.path();
            match fs::remove_file(&path) {
                Ok(()) => {
                    count = count.saturating_add(1);
                    bytes = bytes.saturating_add(metadata.len());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => tidying.failed.push(NotRemoved { path, error }),
            }
        }
    }
    (count, bytes)
}

/// The size of the regular files under `dir`, links not followed. What
/// can't be read counts as nothing: the size is only for the log.
fn size_of(dir: &Path) -> u64 {
    let mut bytes = 0_u64;
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(reader) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in reader.flatten() {
            if is_folder(&entry) {
                folders.push(entry.path());
            } else if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let size = entry.metadata().map_or(0, |metadata| metadata.len());
                bytes = bytes.saturating_add(size);
            }
        }
    }
    bytes
}

/// Whether `entry` is a folder. `DirEntry::file_type` doesn't follow a
/// link, so a link to a folder is not one.
fn is_folder(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|kind| kind.is_dir())
}

/// The entries of `dir`. A folder that can't be read, or an entry that
/// can't, is noted in `tidying`.
fn entries(dir: &Path, tidying: &mut CacheTidying) -> Vec<fs::DirEntry> {
    let reader = match fs::read_dir(dir) {
        Ok(reader) => reader,
        Err(error) => {
            tidying.failed.push(NotRemoved {
                path: dir.to_owned(),
                error,
            });
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    for entry in reader {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => tidying.failed.push(NotRemoved {
                path: dir.to_owned(),
                error,
            }),
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, FileTimes};
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
        fs::set_times(&path, FileTimes::new().set_modified(now - age)).unwrap();
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
        fs::set_times(&ahead, FileTimes::new().set_modified(now + DAY)).unwrap();

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
        fs::set_times(
            &not_rustics,
            FileTimes::new().set_modified(SystemTime::now() - 2 * DAY),
        )
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
        fs::set_times(&outside_unfinished, FileTimes::new().set_modified(old)).unwrap();
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
