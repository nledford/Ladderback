//! The file system side of the store: a bounded read, an atomic write, and the
//! history of earlier versions. Everything here blocks, so the store runs it
//! on a thread meant for that, one caller at a time.

use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use domain::Timestamp;
use jiff::tz::TimeZone;

use crate::schema::{KEPT_VERSIONS, MAX_CONFIG_BYTES, over_cap};

/// What reading the config file found.
pub(crate) enum Contents {
    /// There is no file.
    Missing,
    Bytes(Vec<u8>),
    /// The file is longer than [`MAX_CONFIG_BYTES`]; only that much was read.
    TooLarge,
}

/// Reads the file, and no more than one byte over the cap of it: a file that
/// never ends (a symlink to `/dev/zero`) can't fill the memory.
pub(crate) fn read(path: &Path) -> io::Result<Contents> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Contents::Missing),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    if over_cap(bytes.len()) {
        Ok(Contents::TooLarge)
    } else {
        Ok(Contents::Bytes(bytes))
    }
}

/// A folder only its owner can use, and its parents if they are missing too:
/// the files in it may hold a password.
fn private_folder(path: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

/// The folder a file is in: `.` for a name on its own.
fn folder_of(path: &Path) -> &Path {
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    }
}

/// How many links in a row [`resolve`] follows before it gives up: the limit
/// the operating systems have, rounded up.
const MAX_LINKS: usize = 40;

/// The file at the end of the symlinks at `path`, which needn't exist: a link
/// that points at nothing leads to where the file would be. A name that isn't
/// a link is itself. A relative link is relative to the folder of the link.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = fs::read_link(&current)?;
                current = folder_of(&current).join(target);
            }
            Ok(_) => return Ok(current),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "{} leads through more than {MAX_LINKS} symbolic links",
        path.display()
    )))
}

/// Writes `bytes` as the whole of the file at `path`, or leaves the file as it
/// was: they go to a temporary file in the same folder, which is flushed to
/// disk and then renamed over the file. The folder is made if it is missing.
///
/// If `path` is a symlink, the file it leads to is written, in its folder, and
/// the link stays: renaming over the link would cut the file a dotfile manager
/// keeps from the config.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let path = resolve(path)?;
    let path = path.as_path();
    let folder = folder_of(path);
    private_folder(folder)?;
    let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
    temporary
        .as_file()
        .set_permissions(Permissions::from_mode(0o600))?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    // Without this the rename may not survive a crash; if the folder can't be
    // flushed the file is still whole, so that isn't a failure of the write.
    let _ = File::open(folder).and_then(|folder| folder.sync_all());
    Ok(())
}

/// The folder of the history, next to the file.
pub(crate) fn history_folder(path: &Path) -> PathBuf {
    folder_of(path).join("history")
}

/// `20261108T132021.123Z`: the moment in UTC, so that names sort by time.
fn stamp(at: Timestamp) -> String {
    let at = at.to_zoned(TimeZone::UTC).datetime();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}.{:03}Z",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.millisecond()
    )
}

/// Keeps `old`, the bytes of the file about to be replaced, in the history as
/// `<stamp>-<nn>.toml`, `nn` counting up for saves in the same millisecond, and
/// forgets the versions beyond the newest [`KEPT_VERSIONS`].
pub(crate) fn keep(path: &Path, old: &[u8], at: Timestamp) -> io::Result<()> {
    let folder = history_folder(path);
    private_folder(&folder)?;
    let prefix = format!("{}-", stamp(at));
    let next = names(&folder)?
        .iter()
        .filter_map(|name| {
            name.strip_prefix(&prefix)?
                .strip_suffix(".toml")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .map_or(0, |last| last.saturating_add(1));
    let mut kept = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(folder.join(format!("{prefix}{next:02}.toml")))?;
    kept.write_all(old)?;
    kept.sync_all()?;
    // Forgetting is housekeeping: a version that can't be removed now is
    // removed by a later save, and must not fail this one.
    let _ = forget_old(&folder);
    Ok(())
}

/// The names of the kept versions, oldest first.
fn names(folder: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(folder)? {
        if let Ok(name) = entry?.file_name().into_string()
            && name.ends_with(".toml")
        {
            names.push(name);
        }
    }
    names.sort_unstable();
    Ok(names)
}

fn forget_old(folder: &Path) -> io::Result<()> {
    let names = names(folder)?;
    let surplus = names.len().saturating_sub(KEPT_VERSIONS);
    for name in names.iter().take(surplus) {
        fs::remove_file(folder.join(name))?;
    }
    Ok(())
}

/// Not yet written: the red test below needs it to exist.
#[allow(dead_code, reason = "the fix makes `keep` use it")]
fn keep_with(
    _path: &Path,
    _old: &[u8],
    _at: Timestamp,
    _flush_folder: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    todo!("keep_with")
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn the_history_folder_is_flushed_after_a_version_is_in_it() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let flushed = RefCell::new(Vec::new());
        let at: Timestamp = "2026-11-08T13:20:21.123Z".parse().expect("a time");

        keep_with(&path, b"old", at, |folder| {
            let versions = fs::read_dir(folder)?.count();
            flushed.borrow_mut().push((folder.to_path_buf(), versions));
            Ok(())
        })
        .expect("kept");

        assert_eq!(
            flushed.into_inner(),
            [(history_folder(&path), 1)],
            "flushed once, the history folder, with the version already in it"
        );
    }

    #[test]
    fn a_version_whose_folder_cannot_be_flushed_is_not_reported_kept() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let at: Timestamp = "2026-11-08T13:20:21.123Z".parse().expect("a time");

        let kept = keep_with(&path, b"old", at, |_| {
            Err(io::Error::other("the disk is gone"))
        });

        assert!(kept.is_err(), "{kept:?}");
    }

    #[test]
    fn a_name_on_its_own_is_in_the_current_folder() {
        assert_eq!(folder_of(Path::new("config.toml")), Path::new("."));
        assert_eq!(folder_of(Path::new("/")), Path::new("."));
        assert_eq!(
            folder_of(Path::new("/home/demo/config.toml")),
            Path::new("/home/demo")
        );
        assert_eq!(folder_of(Path::new("demo/config.toml")), Path::new("demo"));
    }
}
