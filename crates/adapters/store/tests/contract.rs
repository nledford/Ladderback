//! The SQLite store against the operation store's and the schedule store's
//! contract suites, the ones the in-memory fakes pass too, so the fakes
//! can't drift from the real thing.

mod support;

use std::sync::{Arc, Mutex};

use app::contract;
use app::ports::ScheduleStore;
use async_trait::async_trait;
use support::*;

#[tokio::test]
async fn sqlite_store_meets_the_operation_store_contract() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    within(
        120,
        "the contract suite",
        contract::operation_store::run(&store),
    )
    .await;
    store.close().await;
}

#[tokio::test]
async fn sqlite_store_recovers_its_unfinished_operations() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    within(
        120,
        "the recovery contract suite",
        contract::operation_store::run_recovery(&store),
    )
    .await;
    store.close().await;
}

/// A new scratch database for each `empty`, and the last one opened again
/// for `reopened`, as the daemon opens its file on a restart.
#[derive(Default)]
struct SqliteSchedules(Mutex<Vec<Scratch>>);

#[async_trait]
impl contract::schedule_store::ScheduleHarness for SqliteSchedules {
    async fn empty(&self) -> Arc<dyn ScheduleStore> {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        self.0.lock().unwrap().push(scratch);
        Arc::new(store)
    }

    async fn reopened(&self) -> Arc<dyn ScheduleStore> {
        let path = self
            .0
            .lock()
            .unwrap()
            .last()
            .expect("a store was opened")
            .path();
        Arc::new(
            store::SqliteStore::open(&path)
                .await
                .expect("the scratch database opens again"),
        )
    }
}

#[tokio::test]
async fn sqlite_store_meets_the_schedule_store_contract() {
    within(
        120,
        "the schedule store contract suite",
        contract::schedule_store::run(&SqliteSchedules::default()),
    )
    .await;
}
