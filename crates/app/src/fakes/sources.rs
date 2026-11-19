//! A `NormalizeSources` fake for the maintenance use cases' unit tests:
//! sources in the form the engine records them, without a disk. Dots and
//! repeated or trailing slashes are removed, and the sources come back sorted
//! by path component and without duplicates, as rustic's `PathList` does
//! (the shared contract suite checks it against the real adapter). It reads
//! no disk, so it resolves no link and refuses no source: a test's plan, with
//! sources that exist on no machine, is checked against exactly its sources.

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use domain::planning::Source;

use crate::ports::{NormalizeSources, SourcesError};

#[derive(Default)]
pub struct FakeSources;

#[async_trait]
impl NormalizeSources for FakeSources {
    async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
        let mut paths: Vec<PathBuf> = sources
            .iter()
            .map(|source| without_dots(source.as_str()))
            .collect();
        paths.sort();
        paths.dedup();
        paths
            .iter()
            .map(|path| {
                let unreadable = |detail: String| SourcesError::Unreadable { detail };
                let text = path
                    .to_str()
                    .ok_or_else(|| unreadable("a source is not UTF-8".to_owned()))?;
                Source::parse(text).map_err(|error| unreadable(error.to_string()))
            })
            .collect()
    }
}

/// `path` from the root with each `.` and repeated or trailing `/` gone and
/// each `..` taking the folder before it away (at the root, nothing).
fn without_dots(path: &str) -> PathBuf {
    let mut kept = PathBuf::from("/");
    for component in Path::new(path).components() {
        match component {
            Component::Normal(name) => kept.push(name),
            Component::ParentDir => {
                kept.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use domain::planning::Source;

    use super::FakeSources;
    use crate::ports::NormalizeSources;

    #[tokio::test]
    async fn a_source_on_no_disk_is_not_refused_and_nothing_is_resolved() {
        let written: Vec<Source> = ["/srv/demo/music", "/srv/demo/../demo/photos/.."]
            .map(|text| Source::parse(text).expect("a demo source"))
            .into();
        let expected: Vec<Source> = ["/srv/demo", "/srv/demo/music"]
            .map(|text| Source::parse(text).expect("a demo source"))
            .into();
        assert_eq!(FakeSources.normalized(&written).await, Ok(expected));
    }

    #[tokio::test]
    async fn a_parent_of_the_root_is_the_root() {
        let written = [Source::parse("/../../a/../..").expect("a source")];
        assert_eq!(
            FakeSources.normalized(&written).await,
            Ok(vec![Source::parse("/").expect("a source")])
        );
    }
}
