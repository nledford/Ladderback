//! The real `Retention` adapter against a scratch repository: it removes
//! exactly the snapshots rustic's own retention lets go, and it agrees with
//! the port's fake on the same group (§13.3: fakes don't drift).

use std::sync::Arc;

use app::fakes::FakeRetention;
use app::ports::{EngineEvent, EngineResult, Job, RepositoryAccess, Retention, RetentionError};
use domain::SnapshotId;
use domain::Timestamp;
use domain::maintenance::Snapshot;
use domain::planning::{KeepCount, RetentionPolicy};
use engine::{RusticRetention, WorkerEngine};
use rustic_core::{BackupOptions, PathList, SnapshotOptions};

mod support;

use support::{PASSWORD, Scratch, engine, run_to_end, write_file};

/// The repository's snapshots as the use case sees them: the listing's,
/// newest first.
async fn listed(engine: &WorkerEngine, access: &RepositoryAccess) -> Vec<Snapshot> {
    let events = run_to_end(engine, Job::Ls(access.clone())).await;
    let list = events.into_iter().find_map(|event| match event {
        EngineEvent::Succeeded(EngineResult::Snapshots(list)) => Some(list),
        _ => None,
    });
    let Some(list) = list else {
        panic!("the listing never answered");
    };
    let mut group: Vec<Snapshot> = list
        .iter()
        .map(|snapshot| Snapshot {
            id: snapshot.id.clone(),
            time: snapshot.time,
            hostname: snapshot.hostname.clone(),
            label: snapshot.label.clone(),
            tags: snapshot.tags.clone(),
            paths: snapshot.paths.clone(),
        })
        .collect();
    group.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.time));
    group
}

/// Four backups of changing contents: four distinct snapshots, each made
/// at least a second of wall-clock time after the last, so no two share a
/// timestamp and the oldest are unambiguous.
async fn four_snapshots(_scratch: &Scratch, access: &RepositoryAccess) -> Vec<Snapshot> {
    let group = listed(&engine(), access).await;
    assert_eq!(group.len(), 4, "{group:?}");
    group
}

#[tokio::test]
async fn keep_last_removes_the_oldest_as_the_fake_does() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let repo = location.open().to_indexed_ids().unwrap();
    let paths = PathList::from_iter([scratch.source()]).sanitize().unwrap();
    for number in 1..=4 {
        write_file(
            scratch.source().join("a.txt"),
            format!("content {number}").as_bytes(),
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
        repo.backup(
            &BackupOptions::default(),
            &paths,
            SnapshotOptions::default().to_snapshot().unwrap(),
        )
        .unwrap();
    }
    let access = location.access(PASSWORD);
    let group = four_snapshots(&scratch, &access).await;

    let policy = RetentionPolicy {
        keep_last: Some(KeepCount::new(2).unwrap()),
        ..RetentionPolicy::default()
    };
    let real = RusticRetention
        .removes(&access, &policy, &group)
        .await
        .expect("the real adapter answers");
    let expected = Arc::new(FakeRetention)
        .removes(&access, &policy, &group)
        .await
        .expect("the fake answers");

    assert_eq!(real, expected, "the fake and the real adapter agree");
    let oldest: Vec<_> = group[2..].iter().map(|s| s.id.clone()).collect();
    assert_eq!(real, oldest, "the oldest two go: the group is newest first");
}

#[tokio::test]
async fn an_error_never_carries_the_repository_credentials() {
    // A repository string with credentials in it, pointed at a port
    // nothing listens on: the open fails, and its detail must come back
    // redacted (I4), like the worker's own errors.
    let access = RepositoryAccess {
        repository: "sftp://demo:hunter2@127.0.0.1:1/repo".to_owned(),
        repo_hot: None,
        password: domain::secrets::SecretValue::new("e2e-scratch-password"),
        id: None,
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
        secrets: Vec::new(),
    };
    let group = vec![Snapshot {
        id: SnapshotId::parse(&"07".repeat(32)).unwrap(),
        time: Timestamp::from_second(1_800_000_000).unwrap(),
        hostname: "demo-host".to_owned(),
        label: String::new(),
        tags: Vec::new(),
        paths: vec!["/home/demo".to_owned()],
    }];
    let error = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &group,
        )
        .await
        .expect_err("the unreachable repository fails");
    let detail = match &error {
        RetentionError::Engine { detail } => detail.clone(),
        other => panic!("an engine failure, not {other:?}"),
    };
    assert!(
        !detail.contains("hunter2") && !detail.contains("e2e-scratch-password"),
        "the detail leaks a credential: {detail}"
    );
}
