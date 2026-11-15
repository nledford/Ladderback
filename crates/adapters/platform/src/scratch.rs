//! The scratch folder of `ladderback mcp --scratch` (technical design §4.6,
//! §10.5): a new folder in the temporary folder, readable by its owner only,
//! that holds a session's configuration, data and repository and is deleted
//! when the session ends.
//!
//! A session holds its folder's lock (the data-dir lock's file, at the
//! folder's root) for as long as it lives, so a sweep at the next start
//! tells a folder a crashed session left from one in use: it deletes only
//! the folders whose lock it can take. A folder is made under a name no
//! sweep looks at and renamed once it is locked, so no sweep sees it
//! unlocked.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::data_dir_lock::{DataDirLock, LockError};

/// The start of every scratch folder's name.
pub const SCRATCH_PREFIX: &str = "ladderback-scratch-";

/// A scratch folder, held for as long as this value lives.
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
    _lock: DataDirLock,
}

impl Scratch {
    /// Makes `<parent>/ladderback-scratch-<name>`, readable by its owner
    /// only, and holds it.
    ///
    /// # Errors
    ///
    /// The folder could not be made, locked or named.
    pub fn create(parent: &Path, name: &str) -> io::Result<Self> {
        let path = parent.join(format!("{SCRATCH_PREFIX}{name}"));
        if fs::symlink_metadata(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is already there", path.display()),
            ));
        }
        // Staged under a name no sweep looks at, until it is locked.
        let staged = parent.join(format!(".{SCRATCH_PREFIX}{name}"));
        make_private_dir(&staged)?;
        let held = DataDirLock::acquire(&staged)
            .map_err(lock_error)
            .and_then(|lock| fs::rename(&staged, &path).map(|()| lock));
        match held {
            Ok(lock) => Ok(Self { path, _lock: lock }),
            Err(error) => {
                // Nothing of a folder that never came to be is left.
                let _ = fs::remove_dir_all(&staged);
                Err(error)
            }
        }
    }

    /// Where the folder is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `name` in the folder, readable by its owner
    /// only, and returns its path.
    ///
    /// # Errors
    ///
    /// The file could not be written.
    pub fn write_private(&self, name: &str, contents: &[u8]) -> io::Result<PathBuf> {
        let path = self.path.join(name);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(contents)?;
        Ok(path)
    }

    /// Deletes the folder and all it holds, then lets go of it.
    ///
    /// # Errors
    ///
    /// Something in it could not be deleted.
    pub fn remove(self) -> io::Result<()> {
        // The lock is let go only once the folder is gone, so no sweep
        // deletes it under this session.
        fs::remove_dir_all(&self.path)
    }
}

/// What a sweep did: the folders it deleted, and those it found nobody
/// holds but could not delete, with why.
#[derive(Debug, Default)]
pub struct Swept {
    pub removed: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, io::Error)>,
}

/// Deletes each scratch folder directly in `parent` that no session holds:
/// one a crashed session left. Anything else is left alone, a link named
/// like a scratch folder included.
pub fn sweep(parent: &Path) -> Swept {
    let mut swept = Swept::default();
    let Ok(entries) = fs::read_dir(parent) else {
        return swept;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(SCRATCH_PREFIX));
        let folder = fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
        if !named || !folder {
            continue;
        }
        // Held by a live session, or not ours to open (another user's
        // folder in a shared temporary folder): left alone.
        let Ok(_lock) = DataDirLock::acquire(&path) else {
            continue;
        };
        match fs::remove_dir_all(&path) {
            Ok(()) => swept.removed.push(path),
            Err(error) => swept.failed.push((path, error)),
        }
    }
    swept
}

/// Makes `path`, which must not be there, readable by its owner only.
fn make_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        builder.mode(0o700).create(path)?;
        // The umask may take more away than the group's and others' bits.
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    builder.create(path)
}

/// A lock that couldn't be taken, as the error of making the folder.
fn lock_error(error: LockError) -> io::Error {
    match error {
        LockError::InUse { .. } => io::Error::new(io::ErrorKind::WouldBlock, "the folder is held"),
        LockError::Unavailable { detail } => io::Error::other(detail),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_scratch_folder_is_new_and_readable_by_its_owner_only() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::create(parent.path(), "one").unwrap();
        assert_eq!(scratch.path(), parent.path().join("ladderback-scratch-one"));
        assert_eq!(mode(scratch.path()), 0o700);
        let entries: Vec<_> = fs::read_dir(parent.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "only the folder is left: {entries:?}");
    }

    #[test]
    fn a_scratch_folder_is_never_one_that_was_there() {
        let parent = tempfile::tempdir().unwrap();
        let _first = Scratch::create(parent.path(), "one").unwrap();
        Scratch::create(parent.path(), "one").expect_err("the name is taken");
    }

    #[test]
    fn a_private_file_is_readable_by_its_owner_only() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::create(parent.path(), "one").unwrap();
        let path = scratch.write_private("password", b"canary").unwrap();
        assert_eq!(path, scratch.path().join("password"));
        assert_eq!(fs::read(&path).unwrap(), b"canary");
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn removing_a_scratch_folder_deletes_all_it_holds() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::create(parent.path(), "one").unwrap();
        let path = scratch.path().to_owned();
        fs::create_dir(path.join("data")).unwrap();
        fs::write(path.join("data").join("file"), "demo").unwrap();
        scratch.remove().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_sweep_deletes_a_scratch_folder_nobody_holds() {
        let parent = tempfile::tempdir().unwrap();
        let left = parent.path().join("ladderback-scratch-crashed");
        fs::create_dir_all(left.join("data")).unwrap();
        fs::write(left.join("data").join("file"), "demo").unwrap();
        let swept = sweep(parent.path());
        assert!(!left.exists());
        assert_eq!(swept.removed, vec![left]);
        assert!(swept.failed.is_empty(), "{:?}", swept.failed);
    }

    #[test]
    fn a_sweep_keeps_a_scratch_folder_in_use() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::create(parent.path(), "live").unwrap();
        let swept = sweep(parent.path());
        assert!(scratch.path().exists());
        assert!(swept.removed.is_empty(), "{:?}", swept.removed);
    }

    #[test]
    fn a_sweep_leaves_everything_else_alone() {
        let parent = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("file"), "demo").unwrap();
        let other = parent.path().join("notes");
        fs::create_dir(&other).unwrap();
        let file = parent.path().join("ladderback-scratch-file");
        fs::write(&file, "demo").unwrap();
        let link = parent.path().join("ladderback-scratch-link");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let swept = sweep(parent.path());
        assert!(other.exists());
        assert!(file.exists());
        assert!(fs::symlink_metadata(&link).is_ok());
        assert!(outside.path().join("file").exists());
        assert!(swept.removed.is_empty(), "{:?}", swept.removed);
    }
}
