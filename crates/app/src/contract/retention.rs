//! The `Retention` contract (technical design §3.2): which of one managed
//! group's snapshots a `keep-last` policy lets go. The forget use case
//! hands it a group exactly as the partition built it (newest first) and
//! relies on three things: the newest `keep-last` of the group stay and the
//! rest are named, a snapshot outside the group is never named, and a policy
//! that sets nothing is refused (it keeps everything, so the use case never
//! asks with one).
//!
//! Only `keep-last` is checked, since it is the one rule the port's fake
//! evaluates: the calendar and `keep-within` rules are rustic's own, which
//! the real adapter hands the policy to, and the fake refuses them. Nor is a
//! failed open checked, since the fake opens nothing.

use std::sync::Arc;

use async_trait::async_trait;
use domain::SnapshotId;
use domain::maintenance::Snapshot;
use domain::planning::{KeepCount, RetentionPolicy};

use crate::ports::{RepositoryAccess, Retention, RetentionError};

/// A retention and the repository it reads.
pub struct Repository {
    pub retention: Arc<dyn Retention>,
    /// How the retention reaches the repository.
    pub access: RepositoryAccess,
    /// Every snapshot the repository holds, newest first, each taken at a
    /// moment of its own, as the listing reports them.
    pub snapshots: Vec<Snapshot>,
}

/// Makes a repository for each check.
#[async_trait]
pub trait RetentionHarness: Send + Sync {
    /// A retention over a repository that holds `count` snapshots.
    async fn repository(&self, count: usize) -> Repository;
}

pub async fn run(harness: &dyn RetentionHarness) {
    the_newest_snapshots_stay_and_the_rest_are_named(harness).await;
    keeping_one_names_all_but_the_newest(harness).await;
    a_count_the_group_does_not_pass_names_nothing(harness).await;
    keeping_all_names_nothing(harness).await;
    only_the_snapshots_it_is_given_are_counted_and_named(harness).await;
    a_group_with_no_snapshot_names_nothing(harness).await;
    a_policy_that_sets_nothing_is_refused(harness).await;
}

fn keep_last(count: i64) -> RetentionPolicy {
    let keep = KeepCount::new(count);
    assert!(keep.is_ok(), "{count} is a count: {keep:?}");
    RetentionPolicy {
        keep_last: keep.ok(),
        ..RetentionPolicy::default()
    }
}

/// The snapshots at `places` of `all`, in the order `all` has them.
fn group(all: &[Snapshot], places: &[usize]) -> Vec<Snapshot> {
    places
        .iter()
        .filter_map(|place| all.get(*place).cloned())
        .collect()
}

/// The IDs of the snapshots at `places` of `all`, ordered: the port doesn't
/// promise the order it names them in.
fn ids(all: &[Snapshot], places: &[usize]) -> Vec<SnapshotId> {
    ordered(group(all, places).into_iter().map(|s| s.id).collect())
}

fn ordered(mut ids: Vec<SnapshotId>) -> Vec<SnapshotId> {
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    ids
}

/// A repository of `count` snapshots, after a failed assertion if the harness
/// made another number: a case that selects snapshots by place would
/// otherwise pass on fewer.
async fn made(harness: &dyn RetentionHarness, count: usize) -> Repository {
    let repository = harness.repository(count).await;
    assert_eq!(
        repository.snapshots.len(),
        count,
        "{:?}",
        repository.snapshots
    );
    repository
}

async fn removes(
    repository: &Repository,
    policy: &RetentionPolicy,
    group: &[Snapshot],
) -> Result<Vec<SnapshotId>, RetentionError> {
    repository
        .retention
        .removes(&repository.access, policy, group)
        .await
        .map(ordered)
}

async fn the_newest_snapshots_stay_and_the_rest_are_named(harness: &dyn RetentionHarness) {
    let repository = made(harness, 5).await;
    let all = &repository.snapshots;
    let removed = removes(&repository, &keep_last(2), all).await;
    assert_eq!(removed, Ok(ids(all, &[2, 3, 4])));
}

async fn keeping_one_names_all_but_the_newest(harness: &dyn RetentionHarness) {
    let repository = made(harness, 3).await;
    let all = &repository.snapshots;
    let removed = removes(&repository, &keep_last(1), all).await;
    assert_eq!(removed, Ok(ids(all, &[1, 2])));
}

async fn a_count_the_group_does_not_pass_names_nothing(harness: &dyn RetentionHarness) {
    let repository = made(harness, 3).await;
    let all = &repository.snapshots;
    for count in [3, 4] {
        let removed = removes(&repository, &keep_last(count), all).await;
        assert_eq!(removed, Ok(Vec::new()), "keep-last {count} of three");
    }
}

async fn keeping_all_names_nothing(harness: &dyn RetentionHarness) {
    let repository = made(harness, 3).await;
    let removed = removes(&repository, &keep_last(-1), &repository.snapshots).await;
    assert_eq!(removed, Ok(Vec::new()));
}

/// The repository holds a snapshot between each two of the group's: they
/// belong to another plan, so they neither count toward `keep-last` nor are
/// named, however old.
async fn only_the_snapshots_it_is_given_are_counted_and_named(harness: &dyn RetentionHarness) {
    let repository = made(harness, 5).await;
    let all = &repository.snapshots;
    let ours = group(all, &[0, 2, 4]);
    let removed = removes(&repository, &keep_last(1), &ours).await;
    assert_eq!(removed, Ok(ids(all, &[2, 4])));
    let removed = removes(&repository, &keep_last(3), &ours).await;
    assert_eq!(removed, Ok(Vec::new()));
}

async fn a_group_with_no_snapshot_names_nothing(harness: &dyn RetentionHarness) {
    let repository = made(harness, 2).await;
    let removed = removes(&repository, &keep_last(1), &[]).await;
    assert_eq!(removed, Ok(Vec::new()));
}

async fn a_policy_that_sets_nothing_is_refused(harness: &dyn RetentionHarness) {
    let repository = made(harness, 2).await;
    let removed = removes(
        &repository,
        &RetentionPolicy::default(),
        &repository.snapshots,
    )
    .await;
    assert_eq!(removed, Err(RetentionError::InvalidPolicy));
}
