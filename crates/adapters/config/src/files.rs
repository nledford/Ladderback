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
/// the operating systems have, rounded up. A longer chain, or a loop, is an
/// error.
const MAX_LINKS: usize = 40;

/// The file at the end of the symlinks at `path`, which needn't exist: a link
/// that points at nothing leads to where the file would be. A name that isn't
/// a link is itself. A relative link is relative to the folder of the link.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    // One look at each of the links and one at the file the last of them leads to.
    for _ in 0..=MAX_LINKS {
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

/// `20261108T132021.123Z`: the moment in UTC, for a person reading the names.
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
/// `<number>-<stamp>.toml`, and forgets the versions beyond the newest
/// [`KEPT_VERSIONS`]. The number counts up from the highest one in the folder,
/// and it, not the time, is the order of the versions: a clock set back would
/// put the newest versions first by their time, and forget them first.
///
/// The version is on disk, and so is its place in the folder, when this
/// returns `Ok`.
pub(crate) fn keep(path: &Path, old: &[u8], at: Timestamp) -> io::Result<()> {
    keep_with(path, old, at, |folder| File::open(folder)?.sync_all())
}

/// [`keep`] with the way a folder is flushed to disk given, so a test sees
/// which folders are, and when: a power loss can't be made to happen.
fn keep_with(
    path: &Path,
    old: &[u8],
    at: Timestamp,
    flush_folder: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let folder = history_folder(path);
    private_folder(&folder)?;
    let number = versions(&folder)?
        .last()
        .map_or(Some(1), |(last, _)| last.checked_add(1))
        .ok_or_else(|| io::Error::other("the history has run out of version numbers"))?;
    let version = folder.join(format!("{number:06}-{}.toml", stamp(at)));
    let mut kept = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&version)?;
    kept.write_all(old)?;
    kept.sync_all()?;
    // The file is whole, but its entry in the folder isn't safe until the
    // folder is flushed, and neither is the folder's own, if it is new: a
    // version reported as kept must survive a power loss, because the file is
    // about to be replaced.
    if let Err(error) = flush_folder(&folder).and_then(|()| flush_folder(folder_of(path))) {
        // Not kept: the file isn't replaced, and the retry mustn't find a copy
        // of the same bytes it has to number after and count among the ten.
        let _ = fs::remove_file(&version);
        return Err(error);
    }
    // Forgetting is housekeeping: a version that can't be removed now is
    // removed by a later save, and must not fail this one.
    let _ = forget_old(&folder);
    Ok(())
}

/// Whether `text` is what [`stamp`] writes: `20261108T132021.123Z`.
fn is_stamp(text: &str) -> bool {
    const SHAPE: &[u8] = b"ddddddddTdddddd.dddZ";
    text.len() == SHAPE.len()
        && text.bytes().zip(SHAPE).all(|(byte, shape)| match shape {
            b'd' => byte.is_ascii_digit(),
            literal => byte == *literal,
        })
}

/// The number a kept version's name starts with: `000042` in
/// `000042-20261108T132021.123Z.toml`. A name of another shape, such as a
/// note someone left in the folder, isn't a version: it is not counted, and
/// never forgotten.
fn number_of(name: &str) -> Option<u64> {
    let (number, stamp) = name.strip_suffix(".toml")?.split_once('-')?;
    if !is_stamp(stamp) || number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// The kept versions as (number, name), oldest first.
fn versions(folder: &Path) -> io::Result<Vec<(u64, String)>> {
    let mut versions = Vec::new();
    for entry in fs::read_dir(folder)? {
        if let Ok(name) = entry?.file_name().into_string()
            && let Some(number) = number_of(&name)
        {
            versions.push((number, name));
        }
    }
    versions.sort_unstable();
    Ok(versions)
}

fn forget_old(folder: &Path) -> io::Result<()> {
    let versions = versions(folder)?;
    let surplus = versions.len().saturating_sub(KEPT_VERSIONS);
    for (_, name) in versions.iter().take(surplus) {
        fs::remove_file(folder.join(name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn the_history_folder_and_its_parent_are_flushed_after_a_version_is_in_it() {
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
            [
                (history_folder(&path), 1),
                (scratch.path().to_path_buf(), 1)
            ],
            "the history folder, with the version already in it, then the folder it is new in"
        );
    }

    #[test]
    fn a_link_is_followed_to_its_end_and_a_missing_file_is_where_it_would_be() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let folder = scratch.path();
        let plain = folder.join("plain.toml");
        assert_eq!(resolve(&plain).expect("resolved"), plain, "nothing there");
        fs::write(&plain, "").expect("a file");
        assert_eq!(resolve(&plain).expect("resolved"), plain, "not a link");

        std::os::unix::fs::symlink("plain.toml", folder.join("one.toml")).expect("a link");
        std::os::unix::fs::symlink(folder.join("one.toml"), folder.join("two.toml"))
            .expect("a link");
        assert_eq!(
            resolve(&folder.join("two.toml")).expect("resolved"),
            plain,
            "a relative link, then an absolute one"
        );
    }

    #[test]
    fn a_path_that_cannot_be_looked_at_is_an_error_not_a_missing_file() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let file = scratch.path().join("file");
        fs::write(&file, "").expect("a file");

        let resolved = resolve(&file.join("config.toml"));

        assert!(resolved.is_err(), "{resolved:?}");
    }

    #[test]
    fn forty_links_in_a_row_are_followed_and_forty_one_are_not() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let folder = scratch.path();
        let plain = folder.join("plain.toml");
        let chain = |links: usize| {
            for n in 0..links {
                let next = if n + 1 == links {
                    "plain.toml".to_owned()
                } else {
                    format!("link-{}.toml", n + 1)
                };
                let _ = fs::remove_file(folder.join(format!("link-{n}.toml")));
                std::os::unix::fs::symlink(next, folder.join(format!("link-{n}.toml")))
                    .expect("a link");
            }
            resolve(&folder.join("link-0.toml"))
        };

        assert_eq!(chain(MAX_LINKS).expect("resolved"), plain);
        let too_many = chain(MAX_LINKS + 1);
        assert!(too_many.is_err(), "{too_many:?}");
    }

    #[test]
    fn a_version_that_cannot_be_flushed_is_not_left_in_the_history() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let at: Timestamp = "2026-11-08T13:20:21.123Z".parse().expect("a time");

        let kept = keep_with(&path, b"old", at, |_| Err(io::Error::other("no")));

        assert!(kept.is_err(), "{kept:?}");
        assert!(
            names_in(&history_folder(&path)).is_empty(),
            "a failed keep isn't retried into duplicates"
        );
    }

    #[test]
    fn a_config_folder_that_cannot_be_flushed_fails_the_keep_too() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let at: Timestamp = "2026-11-08T13:20:21.123Z".parse().expect("a time");

        let kept = keep_with(&path, b"old", at, |folder| {
            if folder == history_folder(&path) {
                Ok(())
            } else {
                Err(io::Error::other("no"))
            }
        });

        assert!(kept.is_err(), "{kept:?}");
        assert!(names_in(&history_folder(&path)).is_empty());
    }

    #[test]
    fn a_name_is_a_version_when_it_starts_with_a_number_and_ends_in_toml() {
        let number = |name| number_of(name);
        assert_eq!(number("000042-20261108T132021.123Z.toml"), Some(42));
        assert_eq!(number("7-20261108T132021.123Z.toml"), Some(7));
        assert_eq!(number("000042-20261108T132021.123Z.txt"), None);
        assert_eq!(
            number("20261108T132021.123Z-00.toml"),
            None,
            "no number first"
        );
        assert_eq!(number("42.toml"), None, "no stamp");
        assert_eq!(
            number("-20261108T132021.123Z.toml"),
            None,
            "an empty number"
        );
        assert_eq!(
            number("+7-20261108T132021.123Z.toml"),
            None,
            "a sign isn't a digit"
        );
        assert_eq!(
            number("99999999999999999999999-20261108T132021.123Z.toml"),
            None,
            "too big"
        );
        assert_eq!(
            number("20261108-notes.toml"),
            None,
            "a date, not a number and a stamp"
        );
        assert_eq!(number("000042-x.toml"), None, "no stamp after the number");
        assert_eq!(
            number("000042-20261108T132021.123.toml"),
            None,
            "a stamp without its Z"
        );
        assert_eq!(
            number("000042-2026-11-08T13:20:21Z.toml"),
            None,
            "another kind of stamp"
        );
    }

    #[test]
    fn the_next_version_is_numbered_after_the_highest_whatever_the_clock_says() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let folder = history_folder(&path);
        fs::create_dir(&folder).expect("the history");
        for name in [
            "000041-20990101T000000.000Z.toml",
            "notes.toml",
            "x-1.toml",
            "20261108-notes.toml",
        ] {
            fs::write(folder.join(name), "").expect("a file");
        }
        let at: Timestamp = "2026-11-08T13:20:21.123Z".parse().expect("a time");

        keep_with(&path, b"old", at, |_| Ok(())).expect("kept");

        assert!(
            folder.join("000042-20261108T132021.123Z.toml").exists(),
            "{:?}",
            names_in(&folder)
        );
        for other in ["notes.toml", "x-1.toml", "20261108-notes.toml"] {
            assert!(
                folder.join(other).exists(),
                "{other} isn't a version and is left alone: {:?}",
                names_in(&folder)
            );
        }
    }

    #[test]
    fn a_history_with_no_number_left_refuses_the_version_and_changes_nothing() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let folder = history_folder(&path);
        fs::create_dir(&folder).expect("the history");
        let last = format!("{}-20261108T132021.123Z.toml", u64::MAX);
        fs::write(folder.join(&last), "").expect("a file");
        let at: Timestamp = "2026-11-08T13:20:21.123Z".parse().expect("a time");

        let kept = keep_with(&path, b"old", at, |_| Ok(()));

        assert!(kept.is_err(), "{kept:?}");
        assert_eq!(names_in(&folder), [last]);
    }

    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(folder)
            .expect("the folder")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
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
