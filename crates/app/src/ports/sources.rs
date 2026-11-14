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

    /// Each of `sources` as a backup walks it, in the order given: resolved
    /// one at a time as [`Self::normalized`] resolves it, so one that can't
    /// be (it isn't there yet) comes back as written, and the domain reads
    /// its dots as rustic does (#510).
    async fn walked(&self, sources: &[Source]) -> Vec<Source> {
        let mut walked = Vec::with_capacity(sources.len());
        for source in sources {
            let resolved = self
                .normalized(std::slice::from_ref(source))
                .await
                .ok()
                .and_then(|mut one| one.pop());
            walked.push(resolved.unwrap_or_else(|| source.clone()));
        }
        walked
    }

    /// Whether `source`, as the backup walks it, holds `folders`, a path
    /// written from it, on disk: whether the path they make resolves as
    /// [`Self::normalized`] resolves a source. It tells which of a plan's
    /// sources a glob that can never match was written from (#510).
    async fn holds(&self, source: &Source, folders: &str) -> bool {
        let under = source.as_str().trim_end_matches('/');
        let Ok(path) = Source::parse(&format!("{under}/{folders}")) else {
            return false;
        };
        self.normalized(std::slice::from_ref(&path)).await.is_ok()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    fn source(text: &str) -> Source {
        Source::parse(text).expect("a source")
    }

    /// Resolves a source under `/var`, as a machine whose folders are links
    /// into it, and none that isn't there: `/missing`, and anything under
    /// it.
    struct UnderVar;

    #[async_trait]
    impl NormalizeSources for UnderVar {
        async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
            sources
                .iter()
                .map(|written| match written.as_str() {
                    text if text.starts_with("/missing") => Err(SourcesError::Unreadable {
                        detail: "not there".to_owned(),
                    }),
                    text => Ok(source(&format!("/var{text}"))),
                })
                .collect()
        }
    }

    #[tokio::test]
    async fn each_source_is_walked_resolved_in_the_order_given_or_as_written() {
        let written = [source("/srv/b"), source("/missing/./a"), source("/home/a")];
        assert_eq!(
            UnderVar.walked(&written).await,
            [
                source("/var/srv/b"),
                source("/missing/./a"),
                source("/var/home/a"),
            ]
        );
    }

    #[tokio::test]
    async fn a_source_holds_the_folders_whose_path_under_it_resolves() {
        assert!(UnderVar.holds(&source("/srv/b"), "x/y").await);
        assert!(UnderVar.holds(&source("/srv/b"), "").await);
        // The path is one `/` between the two, even under the root.
        assert!(!UnderVar.holds(&source("/"), "missing/x").await);
        assert!(!UnderVar.holds(&source("/missing"), "x").await);
    }
}
