//! The in-memory fakes pass the same contract suites as the real adapters.

use std::sync::Arc;
use std::time::Duration;

use app::contract;
use app::contract::clock::ClockHarness;
use app::fakes::{FakeClock, InMemoryOperationStore, SimulatedEngine};
use app::ports::{Clock, Engine};
use async_trait::async_trait;
use domain::{SignedDuration, Timestamp};

fn epoch() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

struct ManualClock(Arc<FakeClock>);

#[async_trait]
impl ClockHarness for ManualClock {
    fn clock(&self) -> Arc<dyn Clock> {
        self.0.clone()
    }

    async fn let_time_pass(&self, by: SignedDuration) {
        self.0.advance(by);
    }
}

#[tokio::test]
async fn fake_clock_meets_the_clock_contract() {
    let harness = ManualClock(Arc::new(FakeClock::new(epoch())));
    contract::clock::run(&harness, SignedDuration::from_secs(60)).await;
}

#[tokio::test]
async fn simulated_engine_meets_the_engine_contract() {
    let clock = Arc::new(FakeClock::auto_advancing(epoch()));
    let engine = SimulatedEngine::new(clock);
    contract::engine::run(&engine, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn in_memory_operation_store_meets_the_operation_store_contract() {
    contract::operation_store::run(&InMemoryOperationStore::new()).await;
}

#[tokio::test]
async fn simulated_engine_steps_take_time_on_its_clock() {
    let clock = Arc::new(FakeClock::auto_advancing(epoch()));
    let engine = SimulatedEngine::new(clock.clone());
    let job = app::ports::FakeJob {
        steps: 3,
        step_delay: Duration::from_secs(60),
        warnings: Vec::new(),
        end: app::ports::FakeEnd::Succeed,
        deadline_budget: None,
    };
    let op = domain::OperationId::from_uuid(domain::Uuid::from_u128(1));
    let events = engine
        .run(op, app::ports::Job::Fake(job))
        .await
        .unwrap()
        .events;
    futures::StreamExt::collect::<Vec<_>>(events).await;
    assert_eq!(clock.now(), epoch() + SignedDuration::from_mins(3));
}
