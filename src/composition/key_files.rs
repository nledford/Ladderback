//! What keeps the secret key files in the data folder out of every backup
//! and every dry run that walks the folder (§16, decisions 120, "A copied
//! data folder", and 122, item 3): the globs that name them, or, when no
//! glob can name the folder, the refusal of a backup that would take them.

use std::path::{Path, PathBuf};

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
        let _ = (given, canonical);
        todo!()
    }

    /// The globs every walk of the data folder leaves the key files out
    /// with; none when no glob can name them.
    pub(super) fn globs(&self) -> &[String] {
        match self {
            Self::Globs(globs) => globs,
            Self::Unnamed(_) => &[],
        }
    }

    /// The globs a backup of `sources` appends to its filter, or its
    /// refusal when no glob can name the key files it would take.
    pub(super) fn for_backup(&self, sources: &[Source]) -> Result<&[String], StartError> {
        let _ = sources;
        todo!()
    }
}
