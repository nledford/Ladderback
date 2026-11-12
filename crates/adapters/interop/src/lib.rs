//! Interop adapter: rustic's profile format behind the `ProfileReader` and
//! `ProfileWriter` ports (technical design §6.2). It is the one place rustic
//! profile TOML is parsed or written; `domain` and `app` know only the types
//! the ports name.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod escape;
mod keys;
mod locate;
mod map;
mod parse;
mod part;
mod resolve;
mod tables;
mod write;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use app::ports::{
    ExportRefusal, ImportedProfile, PlanExport, ProfileError, ProfileReader, ProfileWriter,
    RenderedProfile,
};
use async_trait::async_trait;
use domain::RepositoryAlias;

/// The most a profile may hold. A profile over it is refused unread, so a
/// file that never ends can't fill the memory.
pub const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

/// How many profiles deep `use-profiles` is followed: a profile the profile
/// being read uses is at depth 1.
pub const MAX_USE_DEPTH: usize = 8;

/// How many profiles are read for one import, the first one included. A
/// profile used twice is read twice, as rustic merges it twice.
pub const MAX_PROFILES: usize = 32;

/// How many snapshot entries, hence plans, one import takes.
pub const MAX_ENTRIES: usize = 1000;

/// How many different keys of one profile the import says it left out. A
/// `[repository]` may hold any key, so a profile of a megabyte could name
/// tens of thousands.
pub const MAX_LEFT_OUT_KEYS: usize = 200;

/// How many items (a source, a glob, a tag, a hook, ...) the plans of one
/// import hold in all. The settings of `[backup]` are copied into every plan,
/// so a profile of a megabyte could otherwise ask for gigabytes.
pub const MAX_ITEMS: usize = 100_000;

/// Reads profiles from files, and the profiles they use from the folders it
/// is given, in order.
///
/// It never looks at the process: where rustic keeps its profiles, and the
/// variables rustic reads (`RUSTIC_PASSWORD` and the like), are handed to it.
pub struct FileProfileReader {
    search: Vec<PathBuf>,
    variables: BTreeMap<String, String>,
}

impl FileProfileReader {
    /// A reader that looks for a used profile in each of `search`, first to
    /// last, and that reads the rustic variables among `variables`.
    pub fn new(
        search: Vec<PathBuf>,
        variables: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            search,
            variables: variables.into_iter().collect(),
        }
    }
}

#[async_trait]
impl ProfileReader for FileProfileReader {
    async fn read(
        &self,
        file: &Path,
        repository: &RepositoryAlias,
    ) -> Result<ImportedProfile, ProfileError> {
        let shown = file.display().to_string();
        let path = file.to_path_buf();
        let search = self.search.clone();
        let variables = self.variables.clone();
        let alias = repository.clone();
        // The files are read with blocking calls, so off the async threads.
        let read = tokio::task::spawn_blocking(move || {
            let resolved = resolve::resolve(&path, &search)?;
            map::map(resolved, &alias, &variables, &path.display().to_string())
        })
        .await;
        read.unwrap_or(Err(ProfileError::Unreadable { file: shown }))
    }
}

/// The program a profile runs to read the secret of a keyring entry
/// (§6.2): the platform's own keyring command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyringCommand {
    /// `security find-generic-password`, macOS's.
    Security,
    /// `secret-tool lookup`, libsecret's, for the Secret Service on Linux.
    SecretTool,
}

impl KeyringCommand {
    /// The command of the platform this is built for: `security` on macOS,
    /// `secret-tool` elsewhere.
    pub const fn of_this_platform() -> Self {
        if cfg!(target_os = "macos") {
            Self::Security
        } else {
            Self::SecretTool
        }
    }
}

/// Writes a plan as a profile `rustic -P` accepts.
///
/// Like the reader, it never looks at the process: the keyring command and
/// the `PATH` are handed to it.
#[derive(Clone, Debug)]
pub struct RusticProfileWriter {
    keyring: KeyringCommand,
    path: Vec<PathBuf>,
}

impl RusticProfileWriter {
    /// A writer whose profiles read a keyring entry with `keyring`, which it
    /// looks for on `path`, the `PATH` the export runs under (`None` when it
    /// is unset). A command it can't find there is a notice.
    pub fn new(keyring: KeyringCommand, path: Option<&OsStr>) -> Self {
        Self {
            keyring,
            path: folders(path),
        }
    }
}

/// The folders a program is looked for in on `path`, first to last.
fn folders(path: Option<&OsStr>) -> Vec<PathBuf> {
    std::env::split_paths(path.unwrap_or_default()).collect()
}

impl ProfileWriter for RusticProfileWriter {
    fn render(&self, export: &PlanExport) -> Result<RenderedProfile, ExportRefusal> {
        write::render(export, self.keyring, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no `PATH` a program is looked for where `execvp` looks, not in
    /// the folder the export runs in.
    #[test]
    fn an_unset_path_is_searched_as_execvp_searches_it() {
        assert_eq!(
            folders(None),
            [PathBuf::from("/usr/bin"), PathBuf::from("/bin")]
        );
    }

    /// An empty or relative entry names a folder by the one rustic will run
    /// in, not the export's: it is left out, and an empty `PATH` has none.
    #[test]
    fn empty_and_relative_entries_of_the_path_are_left_out() {
        assert_eq!(
            folders(Some(OsStr::new(":bin:/opt/demo/bin::./tools:/usr/bin:"))),
            [PathBuf::from("/opt/demo/bin"), PathBuf::from("/usr/bin")]
        );
        assert_eq!(folders(Some(OsStr::new(""))), Vec::<PathBuf>::new());
    }
}
