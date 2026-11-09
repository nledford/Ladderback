//! The real `Retention` adapter against a scratch repository: it removes
//! exactly the snapshots rustic's own retention lets go, and it agrees with
//! the port's fake on the same group (§13.3: fakes don't drift).

use std::sync::Arc;

use app::fakes::FakeRetention;
use app::ports::{EngineEvent, EngineResult, Job, RepositoryAccess, Retention};
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

/// Four backups of changing contents: four distinct snapshots, a minute of
/// wall-clock time apart at the very least.
async fn four_snapshots(scratch: &Scratch, access: &RepositoryAccess) -> Vec<Snapshot> {
    let repository = scratch.local();
    let _ = repository;
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
