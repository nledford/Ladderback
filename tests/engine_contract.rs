//! The real engine, `steading worker` processes, meets the same contract as
//! `app`'s simulated engine.

use std::time::Duration;

use app::contract;
use engine::WorkerEngine;

#[tokio::test]
async fn worker_engine_meets_the_engine_contract() {
    let engine = WorkerEngine::new(env!("CARGO_BIN_EXE_steading"), ["worker"]);
    contract::engine::run(&engine, Duration::from_millis(5)).await;
}
