//! The `DriveFlush` contract (technical design §7.3, I1; §16, decision
//! 178): an open database's drive flushes, again and again, and a closed
//! one's says it couldn't, so nothing that depends on its commits goes
//! out.

use std::sync::Arc;

use async_trait::async_trait;

use crate::ports::{DriveFlush, StoreError};

/// Sets up the drive of a database: one that is open, or one that has
/// closed.
#[async_trait]
pub trait DriveFlushHarness: Send + Sync {
    /// The drive of an open database.
    async fn open(&self) -> Arc<dyn DriveFlush>;
    /// The drive of a database that has closed.
    async fn closed(&self) -> Arc<dyn DriveFlush>;
}

pub async fn run(harness: &dyn DriveFlushHarness) {
    an_open_database_flushes(harness).await;
    an_open_database_flushes_again(harness).await;
    a_closed_database_does_not_flush(harness).await;
}

async fn an_open_database_flushes(harness: &dyn DriveFlushHarness) {
    assert_eq!(harness.open().await.flush().await, Ok(()));
}

async fn an_open_database_flushes_again(harness: &dyn DriveFlushHarness) {
    let drive = harness.open().await;
    assert_eq!(drive.flush().await, Ok(()));
    assert_eq!(drive.flush().await, Ok(()));
}

async fn a_closed_database_does_not_flush(harness: &dyn DriveFlushHarness) {
    let flushed = harness.closed().await.flush().await;
    assert!(
        matches!(flushed, Err(StoreError::Unavailable { .. })),
        "a closed database's flush is unavailable: {flushed:?}"
    );
}
