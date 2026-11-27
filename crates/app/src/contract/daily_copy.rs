//! The `DailyCopy` contract (technical design §9; §16, decision 174): a
//! copy taken is dated, and outlives the daemon.

use std::sync::Arc;

use async_trait::async_trait;

use crate::ports::DailyCopy;

/// Opens the daily copy of a database: a new database's, with no copy
/// yet, or the one before opened again on the same place.
#[async_trait]
pub trait DailyCopyHarness: Send + Sync {
    /// The daily copy of a database that has none yet.
    async fn empty(&self) -> Arc<dyn DailyCopy>;
    /// The one [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn DailyCopy>;
}

pub async fn run(harness: &dyn DailyCopyHarness) {
    a_new_database_has_no_copy(harness).await;
    a_copy_taken_is_dated(harness).await;
    the_next_copy_is_dated_no_earlier(harness).await;
    the_copy_outlives_the_daemon(harness).await;
}

async fn a_new_database_has_no_copy(harness: &dyn DailyCopyHarness) {
    let copy = harness.empty().await;
    assert_eq!(copy.taken_at().await, Ok(None));
}

async fn a_copy_taken_is_dated(harness: &dyn DailyCopyHarness) {
    let copy = harness.empty().await;
    assert_eq!(copy.take().await, Ok(()));
    let taken = copy.taken_at().await;
    assert!(matches!(taken, Ok(Some(_))), "{taken:?}");
}

async fn the_next_copy_is_dated_no_earlier(harness: &dyn DailyCopyHarness) {
    let copy = harness.empty().await;
    assert_eq!(copy.take().await, Ok(()));
    let first = copy.taken_at().await;
    assert_eq!(copy.take().await, Ok(()));
    let second = copy.taken_at().await;
    match (&first, &second) {
        (Ok(Some(first)), Ok(Some(second))) => assert!(second >= first, "{first} then {second}"),
        _ => panic!("both copies are dated: {first:?}, {second:?}"),
    }
}

async fn the_copy_outlives_the_daemon(harness: &dyn DailyCopyHarness) {
    let copy = harness.empty().await;
    assert_eq!(copy.take().await, Ok(()));
    let taken = copy.taken_at().await;
    assert!(matches!(taken, Ok(Some(_))), "{taken:?}");
    assert_eq!(harness.reopened().await.taken_at().await, taken);
}
