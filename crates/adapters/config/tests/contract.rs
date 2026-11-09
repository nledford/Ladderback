//! The file store against the config store's contract suite, the one the
//! in-memory fake passes too, so the fake can't drift from the real thing.

mod support;

use std::time::Duration;

use app::contract;
use support::Scratch;

#[tokio::test]
async fn the_file_store_meets_the_config_store_contract() {
    let scratch = Scratch::new();
    let store = scratch.store();
    tokio::time::timeout(Duration::from_secs(60), contract::config_store::run(&store))
        .await
        .expect("the contract suite finishes");
}
