//! The real `NormalizeSources` adapter: a plan's sources come back the way
//! rustic records them — absolute, dots removed, symlinks resolved, sorted
//! and deduplicated — which is what adoption's matching compares (§7.3).

use std::assert_matches;

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
    assert_matches!(error, app::ports::SourcesError::Unreadable { .. });
}

/// The sources a glob is read against (#510): each resolved as rustic
/// resolves it, in the order given, and one that isn't there as written.
#[tokio::test]
async fn each_source_is_walked_resolved_in_order_or_as_written_when_missing() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let real = scratch.path().join("real");
    std::fs::create_dir(&real).expect("a folder");
    let link = scratch.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("a symlink");
    let missing = format!("{}/nowhere/./x", scratch.path().to_str().expect("UTF-8"));
    let source = |text: &str| Source::parse(text).expect("a source");

    let walked = RusticSources
        .walked(&[source(link.to_str().expect("UTF-8")), source(&missing)])
        .await;
    let expected = std::fs::canonicalize(&real).expect("the real path");
    assert_eq!(
        walked,
        [source(expected.to_str().expect("UTF-8")), source(&missing)]
    );
}

/// The whole path said for each glob of `plan` that can never match.
async fn absolutes(plan: &domain::planning::Plan) -> Vec<Option<String>> {
    app::globs::never_matching_globs(plan, &RusticSources)
        .await
        .into_iter()
        .map(|glob| glob.absolute)
        .collect()
}

/// Which of a plan's sources a glob that can never match is for (#510): of
/// several, the one whose folder on disk holds the glob's folders; with
/// none or more than one, no whole path is said.
#[tokio::test]
async fn of_two_sources_the_whole_path_goes_under_the_one_that_holds_the_glob_s_folders() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let home = scratch.path().join("home");
    let disk = scratch.path().join("disk");
    let text = |path: &std::path::Path| path.to_str().expect("UTF-8").to_owned();
    let mut plan = domain::planning::Plan::new(
        domain::PlanId::parse("home").expect("an ID"),
        domain::RepositoryAlias::parse("nas").expect("an alias"),
        vec![
            Source::parse(&text(&home)).expect("a source"),
            Source::parse(&text(&disk)).expect("a source"),
        ],
    );
    plan.filter.globs = vec!["!docker/app/cache".to_owned()];

    // Under neither: the sources are there, the folders aren't.
    std::fs::create_dir_all(&home).expect("a folder");
    std::fs::create_dir_all(&disk).expect("a folder");
    assert_eq!(absolutes(&plan).await, [None]);

    // Under the second only, as walked (its links followed).
    std::fs::create_dir_all(disk.join("docker/app/cache")).expect("a folder");
    let walked = std::fs::canonicalize(&disk).expect("the real path");
    assert_eq!(
        absolutes(&plan).await,
        [Some(format!("!{}/docker/app/cache", text(&walked)))]
    );

    // Under both.
    std::fs::create_dir_all(home.join("docker/app/cache")).expect("a folder");
    assert_eq!(absolutes(&plan).await, [None]);
}
