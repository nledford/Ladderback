//! Initialising a repository through the real worker (§4.1): a `Job::Init`
//! creates one where nothing is yet, with default key options, both halves of
//! a hot/cold location, and never overwrites one that is already there. What
//! the job left behind is checked with `rustic_core` itself, never through
//! Ladderback.

mod support;

use std::assert_matches;
use std::fs;

use app::ports::{EngineEvent, EngineResult, Job};
use support::*;

/// Whether the job ended in failure, and why.
fn failed(events: &[EngineEvent]) -> &domain::execution::FailureReason {
    match events.last() {
        Some(EngineEvent::Failed(reason)) => reason,
        other => panic!("the job did not fail: {other:?}\nall events: {events:?}"),
    }
}

/// Every file's name and bytes under `dir`, sorted, as the repository stood.
fn contents(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push((
                    path.strip_prefix(dir.parent().unwrap())
                        .unwrap()
                        .display()
                        .to_string(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, &mut out);
    }
    out
}

#[tokio::test]
async fn initialising_an_empty_location_gives_the_repository_an_id() {
    let scratch = Scratch::new();
    let location = Location::local(&scratch.repo_dir());
    let events = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
    let EngineResult::Repository { id } = succeeded(&events) else {
        panic!("not a repository result: {events:?}");
    };
    assert_eq!(id.as_str().len(), 64, "the ID is 64 hex characters");
    off_runtime(|| location.open().config().id.to_hex());
}

#[tokio::test]
async fn initialising_where_a_repository_already_exists_is_refused_and_changes_nothing() {
    let scratch = Scratch::new();
    let location = Location::local(&scratch.repo_dir());
    off_runtime(|| location.init());
    let before = contents(&scratch.repo_dir());
    let events = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
    assert_matches!(
        failed(&events),
        domain::execution::FailureReason::RepositoryExists
    );
    assert_eq!(contents(&scratch.repo_dir()), before, "nothing was written");
}

#[tokio::test]
async fn initialising_where_the_hot_half_holds_a_repository_is_refused_and_changes_nothing() {
    let scratch = Scratch::new();
    let location = Location::hot_cold(&scratch.repo_dir(), &scratch.hot_dir());
    // A repository whose config lives only in the hot half: a local init
    // into the hot directory, the cold half left empty.
    off_runtime(|| Location::local(&scratch.hot_dir()).init());
    let before = contents(&scratch.hot_dir());
    let events = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
    assert_matches!(
        failed(&events),
        domain::execution::FailureReason::RepositoryExists
    );
    assert_eq!(contents(&scratch.hot_dir()), before, "nothing was written");
}

#[tokio::test]
async fn a_hot_cold_location_is_initialised_in_both_halves() {
    let scratch = Scratch::new();
    let location = Location::hot_cold(&scratch.repo_dir(), &scratch.hot_dir());
    let events = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
    let EngineResult::Repository { .. } = succeeded(&events) else {
        panic!("not a repository result: {events:?}");
    };
    off_runtime(|| location.open().config().id.to_hex());
    for half in [scratch.repo_dir(), scratch.hot_dir()] {
        assert!(
            half.join("config").is_file(),
            "the {} half holds no config",
            half.display()
        );
    }
}

#[tokio::test]
async fn identifying_the_initialised_repository_reports_the_same_id() {
    let scratch = Scratch::new();
    let location = Location::local(&scratch.repo_dir());
    let init = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
    let EngineResult::Repository { id } = succeeded(&init) else {
        panic!("not a repository result: {init:?}");
    };
    let identify = run_to_end(&engine(), Job::Identify(location.access(PASSWORD))).await;
    assert_eq!(
        succeeded(&identify),
        &EngineResult::Repository { id: id.clone() }
    );
}

/// An empty scratch folder holds no repository: a job that opens one fails
/// as `RepositoryNotFound`, the code a saved repository's row reads as one
/// to create (decision 53), and the folder stays empty.
#[tokio::test]
async fn identifying_an_empty_folder_fails_as_no_repository_there() {
    let scratch = Scratch::new();
    let empty = scratch.repo_dir();
    fs::create_dir_all(&empty).expect("the empty folder");
    let location = Location::local(&empty);
    let events = run_to_end(&engine(), Job::Identify(location.access(PASSWORD))).await;
    assert_eq!(
        failed(&events),
        &domain::execution::FailureReason::RepositoryNotFound
    );
    assert_eq!(contents(&empty), Vec::new(), "nothing was written");
}

/// A repository folder that isn't there is what an unplugged drive looks
/// like (decision 53). A backup there fails as it always did, never as
/// `RepositoryNotFound`, and leaves no folder behind: had it made an empty
/// one at the mount point, the next job would find a folder with no
/// repository in it, and the row would offer Create on the system disk.
/// OpenDAL's `fs` makes its root folder when it is set up, so the job fails
/// before that; rustic's local backend makes nothing when it opens. Create
/// still makes the folder.
#[tokio::test]
async fn a_backup_where_the_repository_folder_is_not_there_makes_none_and_create_still_does() {
    const HOUR: domain::SignedDuration = domain::SignedDuration::from_hours(1);
    let scratch = Scratch::new();
    let opendal = scratch.dir.path().join("usb-opendal").join("restic");
    let local = scratch.dir.path().join("usb-local").join("restic");
    for (missing, location) in [
        (&opendal, Location::opendal_fs(&opendal, None)),
        (&local, Location::local(&local)),
    ] {
        for attempt in 1..=2 {
            let job = backup_job(&location, &scratch.source(), "home", HOUR);
            let events = run_to_end(&engine(), job).await;
            assert_matches!(
                failed(&events),
                domain::execution::FailureReason::EngineFailed { .. },
                "backup {attempt} at {missing:?}: {events:?}"
            );
            assert!(
                !missing.parent().unwrap().exists(),
                "backup {attempt} made a folder at {missing:?}"
            );
        }
        let events = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
        succeeded(&events);
        assert!(missing.is_dir(), "Create made no folder at {missing:?}");
    }
}
