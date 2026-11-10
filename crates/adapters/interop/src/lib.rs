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

use std::path::Path;

use app::ports::{
    ExportRefusal, ImportedProfile, PlanExport, ProfileError, ProfileReader, ProfileWriter,
    RenderedProfile,
};
use async_trait::async_trait;
use domain::RepositoryAlias;

/// Reads profiles from files, and the profiles they use from rustic's config
/// folder, `/etc/rustic` and the working folder.
#[derive(Clone, Debug, Default)]
pub struct FileProfileReader;

impl FileProfileReader {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProfileReader for FileProfileReader {
    #[allow(clippy::todo, reason = "red test: interop import")]
    async fn read(
        &self,
        file: &Path,
        repository: &RepositoryAlias,
    ) -> Result<ImportedProfile, ProfileError> {
        let _ = (file, repository);
        todo!("parse the profile and the profiles it uses, merge as rustic does, map to the domain")
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
