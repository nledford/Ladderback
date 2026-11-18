//! What keeps the secret key files in the data folder out of every backup
//! and every dry run that walks the folder (§16, decisions 120, "A copied
//! data folder", and 122, item 3): the globs that name them, or, when no
//! glob can name the folder, the refusal of a backup that would take them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use domain::planning::Source;
use http_adapter::StartError;

/// How the secret key files in the data folder stay out of a backup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum KeyFilesLeftOut {
    /// The globs that name them ([`domain::fleet::left_out_globs`]), for
    /// the data folder as given and as rustic walks it: rustic
    /// canonicalizes a source, so on macOS a folder under `/var` is walked
    /// as `/private/var`.
    Globs(Vec<String>),
    /// No glob can name them, as a form of the data folder's path isn't
    /// UTF-8: the folder in each form. A backup whose sources hold one of
    /// them while it holds a key file is refused, rather than take the
    /// keys (fail closed); any other backup runs as before.
    Unnamed(Vec<PathBuf>),
}

impl Default for KeyFilesLeftOut {
    /// No globs: a test's runner, whose data folder holds no key file.
    fn default() -> Self {
        Self::Globs(Vec::new())
    }
}

impl KeyFilesLeftOut {
    /// For the data folder at `data_dir`, as given and as it resolves now.
    pub(super) fn of(data_dir: &Path) -> Self {
        Self::over(data_dir, std::fs::canonicalize(data_dir).ok().as_deref())
    }

    /// For the data folder at `given`, which resolves to `canonical` when
    /// it could be resolved.
    pub(super) fn over(given: &Path, canonical: Option<&Path>) -> Self {
        let mut folders = vec![given];
        folders.extend(canonical.filter(|path| *path != given));
        let named: Option<Vec<&str>> = folders.iter().map(|folder| folder.to_str()).collect();
        match named {
            Some(named) => Self::Globs(
                named
                    .into_iter()
                    .flat_map(domain::fleet::left_out_globs)
                    .collect(),
            ),
            None => Self::Unnamed(folders.into_iter().map(Path::to_path_buf).collect()),
        }
    }

    /// The globs each dry run's walks leave the key files out with; none
    /// when no glob can name them.
    pub(super) fn for_dry_runs(&self) -> app::dry_run::LeftOut {
        let globs = match self {
            Self::Globs(globs) => globs.clone(),
            Self::Unnamed(_) => Vec::new(),
        };
        Arc::new(move |_: &[Source]| Some(globs.clone()))
    }

    /// The globs a backup of `sources` appends to its filter, or its
    /// refusal when no glob can name the key files it would take: a
    /// source holds a form of the data folder while the folder holds one.
    pub(super) fn for_backup(&self, sources: &[Source]) -> Result<&[String], StartError> {
        match self {
            Self::Globs(globs) => Ok(globs),
            Self::Unnamed(folders) => {
                let taken = folders.iter().any(|folder| {
                    sources
                        .iter()
                        .any(|source| holds(Path::new(source.as_str()), folder))
                        && holds_key_file(folder)
                });
                if taken {
                    Err(StartError::KeyFilesNotLeftOut)
                } else {
                    Ok(&[])
                }
            }
        }
    }
}

/// Whether a backup of `source` walks `folder`: `folder` is the source or
/// below it, as written or as the source resolves, which is how rustic
/// walks it.
fn holds(source: &Path, folder: &Path) -> bool {
    folder.starts_with(source)
        || std::fs::canonicalize(source).is_ok_and(|resolved| folder.starts_with(resolved))
}

/// Whether `folder` holds a file the globs would leave out. A folder that
/// can't be read is taken to hold one, so the backup is refused rather
/// than risk the keys; one that isn't there holds none.
fn holds_key_file(folder: &Path) -> bool {
    match std::fs::read_dir(folder) {
        Ok(mut entries) => entries.any(|entry| {
            entry.map_or(true, |entry| {
                domain::fleet::is_left_out(entry.file_name().as_encoded_bytes())
            })
        }),
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}
