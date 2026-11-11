//! The real `NormalizeSources` adapter: a plan's sources come back the way
//! rustic records them — absolute, dots removed, symlinks resolved, sorted
//! and deduplicated — which is what adoption's matching compares (§7.3).

use app::ports::NormalizeSources;
use domain::planning::Source;
use engine::RusticSources;

#[tokio::test]
async fn a_symlink_is_resolved_as_rustic_records_it() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let real = scratch.path().join("real");
    std::fs::create_dir(&real).expect("a folder");
    let link = scratch.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("a symlink");
    let source = Source::parse(link.to_str().expect("UTF-8")).expect("a source");

    let normalized = RusticSources
        .normalized(&[source])
        .await
        .expect("the source normalizes");
    let expected = std::fs::canonicalize(&real).expect("the real path");
    assert_eq!(
        normalized,
        vec![Source::parse(expected.to_str().expect("UTF-8")).expect("a source")],
        "the symlink is resolved, as a snapshot's paths are"
    );
}

#[tokio::test]
async fn the_same_source_twice_is_one_source() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let real = scratch.path().join("real");
    std::fs::create_dir(&real).expect("a folder");
    let source = Source::parse(real.to_str().expect("UTF-8")).expect("a source");

    let normalized = RusticSources
        .normalized(&[source.clone(), source])
        .await
        .expect("the sources normalize");
    assert_eq!(normalized.len(), 1, "{normalized:?}");
}

#[tokio::test]
async fn a_dotted_path_comes_back_without_its_dots() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let inner = scratch.path().join("inner");
    std::fs::create_dir(&inner).expect("a folder");
    let dotted = format!("{}/./inner", scratch.path().to_str().expect("UTF-8"));

    let normalized = RusticSources
        .normalized(&[Source::parse(&dotted).expect("a source")])
        .await
        .expect("the source normalizes");
    let expected = std::fs::canonicalize(&inner).expect("the inner folder");
    assert_eq!(
        normalized,
        vec![Source::parse(expected.to_str().expect("UTF-8")).expect("a source")],
        "the dots are gone, as a snapshot's paths have them"
    );
}

#[tokio::test]
async fn a_source_that_cannot_be_read_is_an_error_not_a_panic() {
    let missing = format!("{}/nowhere", std::env::temp_dir().to_string_lossy());
    let error = RusticSources
        .normalized(&[Source::parse(&missing).expect("a source")])
        .await
        .expect_err("nothing is at the path");
    assert!(
        matches!(error, app::ports::SourcesError::Unreadable { .. }),
        "{error:?}"
    );
}
