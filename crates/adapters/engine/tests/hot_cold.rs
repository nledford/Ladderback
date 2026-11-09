//! A hot/cold repository through the real worker (technical design §4.1): a
//! job opens both halves, so what rustic keeps in the hot half gets there and
//! what is wrong with the hot half is found. The repositories are made with
//! `rustic_core` itself, in scratch folders.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use app::ports::{EngineEvent, EngineResult, Job};
use domain::SignedDuration;
use rustic_core::{BackupOptions, PathList, SnapshotOptions};
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// The size of every file under `dir` together.
fn bytes_under(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                bytes_under(&path)
            } else {
                fs::metadata(&path).unwrap().len()
            }
        })
        .sum()
}

fn assert_has_snapshot(half: &str, folder: &Path, snapshot: &str) {
    assert!(
        folder.join("snapshots").join(snapshot).is_file(),
        "the {half} half has no file for the snapshot {snapshot}"
    );
}

/// What a hot half is for: the small files that are read all the time are in
/// both halves, the file data only in the cold one.
#[tokio::test]
async fn a_backup_writes_its_snapshot_to_both_halves_and_its_file_data_to_the_cold_half_only() {
    let scratch = Scratch::new();
    let location = scratch.hot_cold();
    location.init();
    // Incompressible, so the data is as big in the repository as in the source.
    write_file(scratch.source().join("big.bin"), &Rng(5).bytes(2 << 20));
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let snapshot = backed_up(&events).snapshot.as_str().to_owned();
    assert_has_snapshot("hot", &scratch.hot_dir(), &snapshot);
    assert_has_snapshot("cold", &scratch.repo_dir(), &snapshot);
    assert!(
        bytes_under(&scratch.repo_dir()) >= 2 << 20,
        "the cold half does not hold the file data"
    );
    assert!(
        bytes_under(&scratch.hot_dir()) < 1 << 20,
        "the hot half holds the file data"
    );
}

#[tokio::test]
async fn a_repository_is_identified_through_both_halves_and_is_gone_without_the_hot_one() {
    let scratch = Scratch::new();
    let location = scratch.hot_cold();
    let expected = location.init().config().id.to_hex().to_string();
    let engine = engine();
    let events = run_to_end(&engine, Job::Identify(location.access(PASSWORD))).await;
    let EngineResult::Repository { id } = succeeded(&events) else {
        panic!("not an identify result: {events:?}");
    };
    assert_eq!(id.as_str(), expected);

    // The hot half is part of the repository: rustic reads its config first.
    fs::remove_dir_all(scratch.hot_dir()).unwrap();
    let events = run_to_end(&engine, Job::Identify(location.access(PASSWORD))).await;
    assert!(
        matches!(events.last(), Some(EngineEvent::Failed(_))),
        "{events:?}"
    );
}

/// Whether a check ran to its end and found nothing wrong.
fn is_clean(events: &[EngineEvent]) -> bool {
    matches!(
        events.last(),
        Some(EngineEvent::Succeeded(EngineResult::Check(summary))) if summary.errors == 0
    )
}

/// rustic reads the files it keeps in the hot half from there, so a snapshot
/// that half has lost can't be read at all and the check fails: what must not
/// happen is a clean check.
#[tokio::test]
async fn a_check_does_not_pass_a_repository_whose_hot_half_has_lost_a_snapshot() {
    let scratch = Scratch::new();
    let location = scratch.hot_cold();
    let repository = location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    // A snapshot that rustic itself made, into both halves.
    let paths = PathList::from_iter([scratch.source()]).sanitize().unwrap();
    let snapshot = repository
        .to_indexed_ids()
        .unwrap()
        .backup(
            &BackupOptions::default(),
            &paths,
            SnapshotOptions::default().to_snapshot().unwrap(),
        )
        .unwrap();
    let engine = engine();
    let intact = run_to_end(&engine, check_job(&location, false)).await;
    assert!(is_clean(&intact), "{intact:?}");

    fs::remove_file(
        scratch
            .hot_dir()
            .join("snapshots")
            .join(snapshot.id.to_hex().as_str()),
    )
    .unwrap();
    let lost = run_to_end(&engine, check_job(&location, false)).await;
    assert!(
        !is_clean(&lost),
        "a snapshot missing from the hot half went unnoticed: {lost:?}"
    );
}

/// Each half is a backend of its own: here both are OpenDAL's `fs` service,
/// and only `options_hot` and `options_cold` tell them apart.
#[tokio::test]
async fn each_half_is_opened_with_its_own_options() {
    let scratch = Scratch::new();
    let root = |dir: std::path::PathBuf| {
        BTreeMap::from([("root".to_owned(), dir.to_string_lossy().into_owned())])
    };
    let location = Location {
        repository: "opendal:fs".to_owned(),
        repo_hot: Some("opendal:fs".to_owned()),
        options: BTreeMap::new(),
        options_hot: root(scratch.hot_dir()),
        options_cold: root(scratch.repo_dir()),
    };
    off_runtime(|| location.init());
    write_file(scratch.source().join("a.txt"), b"hello");
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let snapshot = backed_up(&events).snapshot.as_str().to_owned();
    assert_has_snapshot("hot", &scratch.hot_dir(), &snapshot);
    assert_has_snapshot("cold", &scratch.repo_dir(), &snapshot);
}

/// A killed worker leaves temporary files in a local hot half as it does in a
/// local cold one, and they are cleaned up on the same terms.
#[tokio::test]
async fn only_old_temporary_files_are_removed_from_the_hot_half_too() {
    let scratch = Scratch::new();
    let location = scratch.hot_cold();
    location.init();
    let hot = scratch.hot_dir();
    let hex = "ab".repeat(32);
    let old_tmp = hot.join("snapshots").join(format!("{hex}-tmp-1"));
    let old_config_tmp = hot.join("config-tmp-1");
    let fresh_tmp = hot.join("snapshots").join(format!("{hex}-tmp-2"));
    let old_other = hot.join("snapshots").join("notes.txt");
    for path in [&old_tmp, &old_config_tmp, &fresh_tmp, &old_other] {
        write_file(path, b"x");
    }
    for path in [&old_tmp, &old_config_tmp, &old_other] {
        two_days_old(path);
    }
    let events = run_to_end(&engine(), Job::Identify(location.access(PASSWORD))).await;
    succeeded(&events);
    assert!(
        !old_tmp.exists() && !old_config_tmp.exists(),
        "a leftover in the hot half was not removed"
    );
    assert!(fresh_tmp.exists() && old_other.exists());
}
