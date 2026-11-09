//! Initialising a repository through the real worker (§4.1): a `Job::Init`
//! creates one where nothing is yet, with default key options, both halves of
//! a hot/cold location, and never overwrites one that is already there. What
//! the job left behind is checked with `rustic_core` itself, never through
//! Ladderback.

mod support;

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
    assert!(matches!(
        failed(&events),
        domain::execution::FailureReason::EngineFailed { .. }
    ));
    assert_eq!(contents(&scratch.repo_dir()), before, "nothing was written");
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
