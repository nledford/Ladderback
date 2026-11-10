//! The SQLite store against the operation store's contract suite, the one the
//! in-memory fake passes too, so the fake can't drift from the real thing.

mod support;

use app::contract;
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
