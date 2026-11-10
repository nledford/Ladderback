//! A repository's listing through the real worker, when it is longer than
//! one message the worker protocol carries (technical design §4.1), and when
//! a snapshot holds more than a list shows of it.

mod support;

use app::ports::{EngineResult, Job, SnapshotSummary};
use jiff::{Timestamp, tz::TimeZone};
use rustic_core::StringList;
use rustic_core::repofile::SnapshotFile;
use support::*;

/// A snapshot made `age` seconds after the epoch, as an old backup would
/// have written it.
fn snapshot(age: i64, label: &str, tags: &[String], paths: &[String]) -> SnapshotFile {
    SnapshotFile {
        time: Timestamp::from_second(age).unwrap().to_zoned(TimeZone::UTC),
        hostname: "demo-host".to_owned(),
        label: label.to_owned(),
        tags: list(tags),
        paths: list(paths),
        ..SnapshotFile::default()
    }
}

/// rustic's list of texts: empty for none, since `""` parses as one empty
/// text.
fn list(texts: &[String]) -> StringList {
    if texts.is_empty() {
        StringList::default()
    } else {
        texts.join(",").parse().unwrap()
    }
}

async fn listed(location: &Location) -> Vec<SnapshotSummary> {
    let events = run_to_end(&engine(), Job::Ls(location.access(PASSWORD))).await;
    let EngineResult::Snapshots(listed) = succeeded(&events) else {
        panic!("not a snapshots result: {events:?}");
    };
    listed.clone()
}

#[tokio::test]
async fn a_listing_longer_than_one_message_is_listed_whole_and_newest_first() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let repository = location.init();
    // 800 snapshots of a 2 KiB label each: over a megabyte of listing, which
    // no one message of the protocol holds.
    let snapshots: Vec<SnapshotFile> = (0..800)
        .map(|n| snapshot(1_000 + n, &format!("{n:04}{}", "x".repeat(2_048)), &[], &[]))
        .collect();
    repository.save_snapshots(snapshots).unwrap();
    let listed = listed(&location).await;
    assert_eq!(listed.len(), 800);
    let labels: Vec<&str> = listed.iter().map(|s| &s.label[..4]).collect();
    let newest_first: Vec<String> = (0..800).rev().map(|n| format!("{n:04}")).collect();
    assert_eq!(labels, newest_first);
}

#[tokio::test]
async fn a_snapshot_with_more_paths_than_a_listing_shows_says_so() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let repository = location.init();
    let many: Vec<String> = (0..100).map(|n| format!("/data/p{n:03}")).collect();
    let few: Vec<String> = (0..3).map(|n| format!("/data/p{n:03}")).collect();
    repository
        .save_snapshots(vec![
            snapshot(2_000, "many", &[], &many),
            snapshot(1_000, "few", &[], &few),
        ])
        .unwrap();
    let listed = listed(&location).await;
    let [many_listed, few_listed] = listed.as_slice() else {
        panic!("two snapshots: {listed:?}");
    };
    assert_eq!(many_listed.label, "many");
    assert_eq!(many_listed.paths, many[..64]);
    assert!(many_listed.paths_truncated, "{many_listed:?}");
    assert!(!many_listed.tags_truncated);
    assert_eq!(few_listed.paths, few);
    assert!(!few_listed.paths_truncated, "{few_listed:?}");
}

#[tokio::test]
async fn a_snapshot_with_more_tags_than_a_listing_shows_says_so() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let repository = location.init();
    let many: Vec<String> = (0..100).map(|n| format!("tag{n:03}")).collect();
    repository
        .save_snapshots(vec![snapshot(2_000, "many", &many, &[])])
        .unwrap();
    let listed = listed(&location).await;
    let [one] = listed.as_slice() else {
        panic!("one snapshot: {listed:?}");
    };
    assert_eq!(one.tags, many[..64]);
    assert!(one.tags_truncated, "{one:?}");
    assert!(!one.paths_truncated);
}
