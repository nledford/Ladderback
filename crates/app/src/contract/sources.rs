//! The `NormalizeSources` contract (technical design §3.2, §7.3): a plan's
//! sources as the engine records them in a snapshot's paths. Adoption
//! matches an unmanaged group's snapshots against them, so the form matters:
//! dots and repeated or trailing `/` gone, each folder once, in the order
//! rustic sorts paths (by component, not by text).
//!
//! Folders that exist are all the suite reads. What a link resolves to, and
//! the error for a source that isn't there, need a disk the fake doesn't
//! model (every path is a folder to it), so the real adapter's own tests
//! check them.

use std::sync::Arc;

use domain::planning::Source;

use crate::ports::NormalizeSources;

/// Builds the disk for one case.
pub trait SourcesHarness {
    /// A normalizer over a disk that holds `folders`, each a path relative to
    /// the disk's root with its parents made as needed, and the root,
    /// resolved.
    fn disk(&self, folders: &[&str]) -> (Arc<dyn NormalizeSources>, Source);
}

/// Folders `a` and `b`, `a`'s own `b` and one whose name continues `a`'s with
/// a dash: `a/b` sorts before `a-b` by component, though not by text.
const FOLDERS: &[&str] = &["a", "b", "a/b", "a-b"];

pub async fn run(harness: &dyn SourcesHarness) {
    a_source_already_in_form_comes_back_as_written(harness).await;
    dots_and_repeated_or_trailing_slashes_are_removed(harness).await;
    a_folder_written_twice_is_one_source(harness).await;
    the_sources_are_sorted(harness).await;
    the_sources_are_sorted_by_component_not_by_text(harness).await;
    no_sources_are_no_sources(harness).await;
    normalizing_what_is_normalized_changes_nothing(harness).await;
    each_source_is_walked_in_the_order_given(harness).await;
}

/// `path` under `root`.
fn under(root: &Source, path: &str) -> Source {
    let text = format!("{}/{path}", root.as_str().trim_end_matches('/'));
    let source = Source::parse(&text);
    assert!(source.is_ok(), "{text:?} is a source: {source:?}");
    source.unwrap_or_else(|_| root.clone())
}

async fn normalized(
    harness: &dyn SourcesHarness,
    written: impl FnOnce(&Source) -> Vec<Source>,
) -> (Vec<Source>, Source) {
    let (disk, root) = harness.disk(FOLDERS);
    let sources = disk.normalized(&written(&root)).await;
    assert!(sources.is_ok(), "the folders are there: {sources:?}");
    (sources.unwrap_or_default(), root)
}

async fn a_source_already_in_form_comes_back_as_written(harness: &dyn SourcesHarness) {
    let (sources, root) = normalized(harness, |root| vec![under(root, "a")]).await;
    assert_eq!(sources, [under(&root, "a")]);
}

async fn dots_and_repeated_or_trailing_slashes_are_removed(harness: &dyn SourcesHarness) {
    for written in ["./a", "b/../a", "/a", "a/", "a/.", "./a/./"] {
        let (sources, root) = normalized(harness, |root| vec![under(root, written)]).await;
        assert_eq!(sources, [under(&root, "a")], "{written:?}");
    }
}

async fn a_folder_written_twice_is_one_source(harness: &dyn SourcesHarness) {
    let (sources, root) = normalized(harness, |root| {
        vec![under(root, "a"), under(root, "./a"), under(root, "a/")]
    })
    .await;
    assert_eq!(sources, [under(&root, "a")]);
}

async fn the_sources_are_sorted(harness: &dyn SourcesHarness) {
    let (sources, root) =
        normalized(harness, |root| vec![under(root, "b"), under(root, "a")]).await;
    assert_eq!(sources, [under(&root, "a"), under(&root, "b")]);
}

async fn the_sources_are_sorted_by_component_not_by_text(harness: &dyn SourcesHarness) {
    let (sources, root) =
        normalized(harness, |root| vec![under(root, "a-b"), under(root, "a/b")]).await;
    assert_eq!(sources, [under(&root, "a/b"), under(&root, "a-b")]);
}

async fn no_sources_are_no_sources(harness: &dyn SourcesHarness) {
    let (sources, _) = normalized(harness, |_| Vec::new()).await;
    assert_eq!(sources, []);
}

async fn normalizing_what_is_normalized_changes_nothing(harness: &dyn SourcesHarness) {
    let (disk, root) = harness.disk(FOLDERS);
    let written = [
        under(&root, "b/../b"),
        under(&root, "./a-b"),
        under(&root, "a/b"),
    ];
    let once = disk.normalized(&written).await;
    assert!(once.is_ok(), "the folders are there: {once:?}");
    let once = once.unwrap_or_default();
    assert_eq!(disk.normalized(&once).await, Ok(once));
}

/// A walk resolves each source alone: it keeps the order given and a source
/// given twice.
async fn each_source_is_walked_in_the_order_given(harness: &dyn SourcesHarness) {
    let (disk, root) = harness.disk(FOLDERS);
    let written = [under(&root, "b"), under(&root, "./a"), under(&root, "b/")];
    assert_eq!(
        disk.walked(&written).await,
        [under(&root, "b"), under(&root, "a"), under(&root, "b")]
    );
}
