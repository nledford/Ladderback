//! A whole copy of the database at a path of the caller's (§9; §16,
//! decision 174), and the file steps the store's copies share. The daily
//! copy takes one; a copy before a migration (#1474) is meant to be the
//! same routine.
//!
//! The copy is written to `<destination>-new` first, made empty with the
//! database's permissions before any row reaches it, and renamed over
//! `destination` only once it is whole and on the disk: a copy that stops
//! before the rename, by a failure, a dropped future or a crash, leaves
//! whatever was at `destination` as it was. A work file a stopped copy
//! left is removed by the next.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use sqlx::SqliteConnection;

use crate::report::cap;

/// The suffix of the file a copy is written to before it takes its place.
const WORK: &str = "-new";

/// Why a copy failed, in SQLite's or the operating system's words, capped.
#[derive(Debug)]
pub(crate) enum CopyFailed {
    /// The copy didn't take the destination's place, which is as it was.
    NotInPlace { detail: String },
    /// The copy took the destination's place, but its folder couldn't be
    /// synced, so the rename may not survive a power loss.
    Unsynced { detail: String },
}

impl CopyFailed {
    fn of(error: impl std::fmt::Display) -> Self {
        Self::NotInPlace {
            detail: cap(error.to_string()),
        }
    }
}

/// Copies the database at `database`, through `conn`, one of its own
/// connections, to `destination`, as the module says. The copy is a
/// consistent snapshot: writes beside it go on, and those it doesn't
/// hold are in the next. On an error `destination` is as it was, unless
/// only the folder's sync after the rename failed, which the error says.
pub(crate) async fn copy_to(
    conn: &mut SqliteConnection,
    database: &Path,
    destination: &Path,
) -> Result<(), CopyFailed> {
    let work = beside(destination, WORK);
    let target = work
        .to_str()
        .ok_or_else(|| CopyFailed::of(format!("{} isn't UTF-8", work.display())))?;
    clear(&work).map_err(CopyFailed::of)?;
    make_empty_like(&work, database).map_err(CopyFailed::of)?;
    let copied = match sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(conn)
        .await
    {
        Ok(_) => put_in_place(&work, destination),
        Err(error) => Err(CopyFailed::of(error)),
    };
    if copied.is_err() {
        // Best effort: the next copy removes it all the same.
        let _ = clear(&work);
    }
    copied
}

/// The work file on the disk, then in `destination`'s place, the rename on
/// the disk too.
fn put_in_place(work: &Path, destination: &Path) -> Result<(), CopyFailed> {
    File::open(work)
        .and_then(|file| file.sync_all())
        .map_err(CopyFailed::of)?;
    fs::rename(work, destination).map_err(CopyFailed::of)?;
    sync_dir(folder(destination)).map_err(|error| CopyFailed::Unsynced {
        detail: cap(format!("its folder couldn't be synced: {error}")),
    })
}

/// Removes `file` and the journals SQLite may have left beside it.
pub(crate) fn clear(file: &Path) -> io::Result<()> {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        remove_if_there(&beside(file, suffix))?;
    }
    Ok(())
}

/// `path` with `suffix` after its name.
pub(crate) fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Makes `file`, which must not be there, empty and with `like`'s
/// permissions, so a copy of `like`'s rows written into it is never more
/// open than `like`. SQLite writes a snapshot into an empty file and takes
/// it as a new database.
pub(crate) fn make_empty_like(file: &Path, like: &Path) -> io::Result<()> {
    let permissions = fs::metadata(like)?.permissions();
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(permissions.mode() & 0o777);
    }
    options.open(file)?;
    // The umask may have taken more away than `like`'s mode did.
    fs::set_permissions(file, permissions)
}

pub(crate) fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The folder `path` is in.
pub(crate) fn folder(path: &Path) -> &Path {
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    }
}

/// Makes the renames in `dir` durable.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}
