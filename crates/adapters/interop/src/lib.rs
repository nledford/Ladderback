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

use std::collections::BTreeMap;
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

/// Writes a plan as a profile `rustic -P` accepts.
#[derive(Clone, Debug, Default)]
pub struct RusticProfileWriter;

impl RusticProfileWriter {
    pub fn new() -> Self {
        Self
    }
}

impl ProfileWriter for RusticProfileWriter {
    #[allow(clippy::todo, reason = "red test: interop export")]
    fn render(&self, export: &PlanExport) -> Result<RenderedProfile, ExportRefusal> {
        let _ = export;
        todo!("render one [[backup.snapshots]] entry, [forget] and [snapshot-filter]")
    }
}
