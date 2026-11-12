//! Leftover temporary files of the local backend.
//!
//! The local backend writes `<id>-tmp-` next to the file it is saving, and
//! renames it when it is complete. A worker killed in between (a deadline, a
//! stop, a crash) leaves the temporary file behind, and nothing cleans it up.
//! Once a job has opened a local repository, the worker deletes such files
//! from the folders the backend writes to, as long as they are old enough
//! that no other writer can still be using them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How old a temporary file must be before it counts as abandoned. A pack
/// takes seconds to write, so a day leaves room for a slow remote mount and a
/// second machine backing up into the same repository.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// What a cleanup did.
#[derive(Debug, Default)]
pub(crate) struct Cleanup {
    pub removed: Vec<PathBuf>,
    /// Files that couldn't be deleted, and why. None of it fails a job.
    pub failed: Vec<(PathBuf, io::Error)>,
}

/// Deletes the temporary files of the repository at `root` that were last
/// written more than [`STALE_AFTER`] before `now`.
///
/// Only the places the local backend writes to are looked at, and nothing
/// deeper: the repository's root (the config file's `config-tmp-…`), `keys`,
/// `snapshots` and `index`, and the `data/<two hex digits>` folders (the
/// others' `<64 lowercase hex digits>-tmp-…`). Everything else in the tree is
/// somebody's own, whatever it is called. Only regular files are touched,
/// symlinks aren't followed, and a repository that doesn't exist has nothing
/// to clean.
///
/// The caller must have opened the repository first: a path that turned out
/// not to be one is not ours to clean.
pub(crate) fn remove_stale_files(root: &Path, now: SystemTime) -> Cleanup {
    let mut cleanup = Cleanup::default();
    sweep(root, is_config_temporary, now, &mut cleanup);
    for folder in ["keys", "snapshots", "index"] {
        sweep(&root.join(folder), is_id_temporary, now, &mut cleanup);
    }
    for folder in pack_folders(&root.join("data"), &mut cleanup) {
        sweep(&folder, is_id_temporary, now, &mut cleanup);
    }
    cleanup
}

/// The `<two hex digits>` folders directly inside `data`.
fn pack_folders(data: &Path, cleanup: &mut Cleanup) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for entry in entries(data, cleanup) {
        // `DirEntry::file_type` doesn't follow symlinks.
        let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let is_pack_folder = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.len() == 2 && name.bytes().all(is_lower_hex));
        if is_folder && is_pack_folder {
            folders.push(entry.path());
        }
    }
    folders
}

/// Deletes the stale files directly inside `dir` whose names `is_temporary`
/// accepts.
fn sweep(dir: &Path, is_temporary: fn(&str) -> bool, now: SystemTime, cleanup: &mut Cleanup) {
    for entry in entries(dir, cleanup) {
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        let name = entry.file_name();
        if !is_file || !name.to_str().is_some_and(is_temporary) {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok());
        if !age.is_some_and(|age| age > STALE_AFTER) {
            continue;
        }
        let path = entry.path();
        match fs::remove_file(&path) {
            Ok(()) => cleanup.removed.push(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => cleanup.failed.push((path, error)),
        }
    }
}

/// The entries of `dir`. A folder that isn't there has none; one that can't
/// be read, or an entry that can't, is noted in `cleanup`.
fn entries(dir: &Path, cleanup: &mut Cleanup) -> Vec<fs::DirEntry> {
    let reader = match fs::read_dir(dir) {
        Ok(reader) => reader,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            cleanup.failed.push((dir.to_path_buf(), error));
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    for entry in reader {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => cleanup.failed.push((dir.to_path_buf(), error)),
        }
    }
    entries
}

fn is_lower_hex(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'a'..=b'f')
}

/// The name the local backend gives the temporary file of the config file.
fn is_config_temporary(name: &str) -> bool {
    name.starts_with("config-tmp-")
}

/// The name the local backend gives the temporary file of a key, snapshot,
/// index or pack: `<64 lowercase hex digits>-tmp-…`.
pub(crate) fn is_id_temporary(name: &str) -> bool {
    let Some((id, _)) = name.split_once("-tmp-") else {
        return false;
    };
    id.len() == 64 && id.bytes().all(is_lower_hex)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn age(path: &Path, age: Duration, now: SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(now - age)
            .unwrap();
    }

    fn touch(path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref().to_owned();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"bytes").unwrap();
        path
    }

    fn two_days() -> Duration {
        Duration::from_secs(2 * 24 * 60 * 60)
    }

    #[test]
    fn old_temporary_files_are_deleted_wherever_the_backend_writes_them() {
        let repo = TempDir::new().unwrap();
        let now = SystemTime::now();
        let leftovers = [
            touch(repo.path().join("config-tmp-")),
            touch(repo.path().join(format!("data/01/{ID}-tmp-"))),
            touch(repo.path().join(format!("index/{ID}-tmp-"))),
            touch(repo.path().join(format!("snapshots/{ID}-tmp-"))),
            touch(repo.path().join(format!("keys/{ID}-tmp-"))),
            touch(repo.path().join(format!("data/ff/{ID}-tmp-with-a-suffix"))),
        ];
        for leftover in &leftovers {
            age(leftover, two_days(), now);
        }
        let cleanup = remove_stale_files(repo.path(), now);
        assert!(cleanup.failed.is_empty(), "{:?}", cleanup.failed);
        assert_eq!(cleanup.removed.len(), leftovers.len());
        for leftover in &leftovers {
            assert!(!leftover.exists(), "{leftover:?} is still there");
        }
    }

    #[test]
    fn a_temporary_file_that_may_still_be_in_use_stays() {
        let repo = TempDir::new().unwrap();
        let now = SystemTime::now();
        let recent = touch(repo.path().join(format!("data/01/{ID}-tmp-")));
        age(&recent, Duration::from_secs(23 * 60 * 60), now);
        let fresh = touch(repo.path().join("config-tmp-"));
        // A clock set back: the file looks like it is from the future.
        let future = touch(repo.path().join(format!("index/{ID}-tmp-")));
        File::options()
            .write(true)
            .open(&future)
            .unwrap()
            .set_modified(now + Duration::from_secs(3600))
            .unwrap();
        let cleanup = remove_stale_files(repo.path(), now);
        assert!(cleanup.removed.is_empty());
        assert!(recent.exists() && fresh.exists() && future.exists());
    }

    #[test]
    fn nothing_but_temporary_files_is_ever_touched() {
        let repo = TempDir::new().unwrap();
        let now = SystemTime::now();
        let bystanders = [
            touch(repo.path().join("config")),
            touch(repo.path().join(format!("data/01/{ID}"))),
            touch(repo.path().join(format!("index/{ID}-tmp"))),
            touch(repo.path().join(format!("index/{ID}tmp-"))),
            touch(repo.path().join("index/notes-tmp-1")),
            touch(repo.path().join("keys/config-tmp")),
            // Not an ID: too short, uppercase, not hex.
            touch(repo.path().join("data/01/0123abcd-tmp-")),
            touch(
                repo.path()
                    .join(format!("data/01/{}-tmp-", ID.to_uppercase())),
            ),
            touch(repo.path().join(format!("data/01/{}-tmp-", "g".repeat(64)))),
            touch(repo.path().join(format!("data/01/x{ID}-tmp-"))),
            touch(repo.path().join(format!("data/01/{ID}0-tmp-"))),
        ];
        for bystander in &bystanders {
            age(bystander, two_days(), now);
        }
        // A directory with a temporary name, holding a file with a plain one.
        let dir = repo.path().join(format!("snapshots/{ID}-tmp-"));
        let inside = touch(dir.join("inside"));
        age(&inside, two_days(), now);

        let cleanup = remove_stale_files(repo.path(), now);
        assert!(cleanup.removed.is_empty(), "{:?}", cleanup.removed);
        for bystander in &bystanders {
            assert!(bystander.exists(), "{bystander:?} was deleted");
        }
        assert!(dir.is_dir() && inside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_with_a_temporary_name_is_left_alone_and_never_followed() {
        let repo = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let now = SystemTime::now();
        let target = touch(elsewhere.path().join("precious"));
        age(&target, two_days(), now);
        fs::create_dir_all(repo.path().join("data/01")).unwrap();
        let link = repo.path().join(format!("data/01/{ID}-tmp-"));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let linked_dir = repo.path().join("data/02");
        std::os::unix::fs::symlink(elsewhere.path(), &linked_dir).unwrap();
        let outside_tmp = touch(elsewhere.path().join("config-tmp-"));
        age(&outside_tmp, two_days(), now);

        let cleanup = remove_stale_files(repo.path(), now);
        assert!(cleanup.removed.is_empty());
        assert!(target.exists() && outside_tmp.exists());
        assert!(link.symlink_metadata().is_ok());
    }

    #[test]
    fn a_folder_that_is_not_a_repository_keeps_files_that_only_look_like_leftovers() {
        // A mistyped repository path: somebody's own folder, say.
        let folder = TempDir::new().unwrap();
        let now = SystemTime::now();
        let lookalikes = [
            touch(folder.path().join(format!("projects/deep/er/{ID}-tmp-1"))),
            touch(folder.path().join("projects/config-tmp-1")),
            touch(folder.path().join(format!("{ID}-tmp-1"))),
        ];
        for lookalike in &lookalikes {
            age(lookalike, two_days(), now);
        }
        let cleanup = remove_stale_files(folder.path(), now);
        assert!(cleanup.removed.is_empty(), "{:?}", cleanup.removed);
        for lookalike in &lookalikes {
            assert!(lookalike.exists(), "{lookalike:?} was deleted");
        }
    }

    #[test]
    fn a_repository_loses_leftovers_only_from_the_folders_rustic_writes_to() {
        let repo = TempDir::new().unwrap();
        let now = SystemTime::now();
        let elsewhere = [
            touch(repo.path().join(format!("notes/{ID}-tmp-1"))),
            touch(repo.path().join(format!("locks/{ID}-tmp-1"))),
            // `data` holds one folder per two hex digits of a pack's ID.
            touch(repo.path().join(format!("data/{ID}-tmp-1"))),
            touch(repo.path().join(format!("data/zz/{ID}-tmp-1"))),
            touch(repo.path().join(format!("data/ABC/{ID}-tmp-1"))),
            touch(repo.path().join(format!("data/ab/deeper/{ID}-tmp-1"))),
            touch(repo.path().join(format!("index/deeper/{ID}-tmp-1"))),
            // Only the config file has this kind of temporary name, and it
            // sits at the root.
            touch(repo.path().join("index/config-tmp-1")),
            touch(repo.path().join(format!("{ID}-tmp-1"))),
        ];
        let theirs = [
            touch(repo.path().join("config-tmp-1")),
            touch(repo.path().join(format!("keys/{ID}-tmp-1"))),
            touch(repo.path().join(format!("snapshots/{ID}-tmp-1"))),
            touch(repo.path().join(format!("index/{ID}-tmp-1"))),
            touch(repo.path().join(format!("data/ab/{ID}-tmp-1"))),
        ];
        for path in elsewhere.iter().chain(&theirs) {
            age(path, two_days(), now);
        }
        let cleanup = remove_stale_files(repo.path(), now);
        assert_eq!(cleanup.removed.len(), theirs.len(), "{:?}", cleanup.removed);
        for path in &theirs {
            assert!(!path.exists(), "{path:?} is still there");
        }
        for path in &elsewhere {
            assert!(path.exists(), "{path:?} was deleted");
        }
    }

    #[test]
    fn a_repository_that_does_not_exist_has_nothing_to_clean() {
        let repo = TempDir::new().unwrap();
        let cleanup = remove_stale_files(&repo.path().join("missing"), SystemTime::now());
        assert!(cleanup.removed.is_empty() && cleanup.failed.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_deleted_is_reported_and_the_rest_still_goes() {
        let repo = TempDir::new().unwrap();
        let now = SystemTime::now();
        let stuck = touch(repo.path().join(format!("data/01/{ID}-tmp-")));
        let free = touch(repo.path().join(format!("index/{ID}-tmp-")));
        age(&stuck, two_days(), now);
        age(&free, two_days(), now);
        // Deleting needs write access to the directory.
        let locked = stuck.parent().unwrap();
        fs::set_permissions(locked, fs::Permissions::from_mode(0o555)).unwrap();
        let cleanup = remove_stale_files(repo.path(), now);
        // Running as root, permissions don't stop a write; there is nothing to see.
        let can_write_anyway = File::create(locked.join("probe")).is_ok();
        fs::set_permissions(locked, fs::Permissions::from_mode(0o755)).unwrap();
        if can_write_anyway {
            testkit::skip!("running as root: permissions don't stop deleting a file");
        }
        assert_eq!(cleanup.failed.len(), 1, "{:?}", cleanup.failed);
        assert_eq!(cleanup.failed[0].0, stuck);
        assert_eq!(cleanup.removed, std::slice::from_ref(&free));
        assert!(stuck.exists() && !free.exists());
    }
}
