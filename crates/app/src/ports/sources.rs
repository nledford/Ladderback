//! The `Sources` port: a plan's sources as the engine records them (§7.3,
//! step 3). A snapshot's paths are the plan's sources after rustic's own
//! normalization — dots removed, symlinks resolved, sorted, deduplicated —
//! and that normalization reads the disk, so it happens behind this port,
//! in an adapter. Adoption's matching compares like with like.

use async_trait::async_trait;
use domain::planning::Source;

/// Why a plan's sources couldn't be normalized.
#[derive(Debug, PartialEq, Eq)]
pub enum SourcesError {
    /// A source couldn't be read from the disk, or isn't UTF-8. `detail` is
    /// the adapter's own message.
    Unreadable { detail: String },
}

impl std::fmt::Display for SourcesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for SourcesError {}

/// A plan's sources as the engine records them: absolute, dots removed,
/// symlinks resolved, sorted and deduplicated, in order.
#[async_trait]
pub trait NormalizeSources: Send + Sync {
    async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError>;
}
