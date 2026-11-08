//! Runs every scenario in `features/` tagged neither `@e2e` nor `@ui`,
//! driving use cases with in-memory fakes and a fake clock (§13.2).

use std::sync::Arc;
use std::time::Duration;

use app::execution::RunOperation;
use app::fakes::{FakeClock, SimulatedEngine};
use app::ports::{Engine, EngineError, EngineEvent, FakeEnd, FakeJob, Job};
use async_trait::async_trait;
use cucumber::World;
use domain::execution::Operation;
use domain::{OperationId, Timestamp, Uuid};
use futures::stream::BoxStream;

#[path = "steps/operation.rs"]
mod operation_steps;

#[derive(World)]
#[world(init = Self::new)]
struct Agent {
    clock: Arc<FakeClock>,
    engine: Arc<dyn Engine>,
    job: FakeJob,
    operation: Option<Operation>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("job", &self.job)
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}

impl Agent {
    fn new() -> Self {
        let clock = Arc::new(FakeClock::auto_advancing(
            Timestamp::from_second(1_800_000_000).unwrap(),
        ));
        Self {
            engine: Arc::new(SimulatedEngine::new(clock.clone())),
            clock,
            job: FakeJob {
                steps: 1,
                step_delay: Duration::from_secs(1),
                warnings: Vec::new(),
                end: FakeEnd::Succeed,
            },
            operation: None,
        }
    }

    fn make_engine_unavailable(&mut self) {
        self.engine = Arc::new(Unavailable);
    }

    async fn run_operation(&mut self) {
        let use_case = RunOperation::new(self.engine.clone(), self.clock.clone());
        let id = OperationId::from_uuid(Uuid::from_u128(1));
        self.operation = Some(use_case.run(id, Job::Fake(self.job.clone())).await);
    }

    fn operation(&self) -> &Operation {
        self.operation.as_ref().expect("the operation has run")
    }
}

/// An engine whose worker can't be launched.
struct Unavailable;

#[async_trait]
impl Engine for Unavailable {
    async fn run(
        &self,
        _op: OperationId,
        _job: Job,
    ) -> Result<BoxStream<'static, EngineEvent>, EngineError> {
        Err(EngineError {
            detail: "worker not found".to_owned(),
        })
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test runner reads its own argv"
)]
#[tokio::main]
async fn main() {
    // `cargo test -- --list` passes `--list` to every test target; this
    // runner has no individually listable tests (scenarios run by name).
    if std::env::args().any(|a| a == "--list") {
        return;
    }
    let features = concat!(env!("CARGO_MANIFEST_DIR"), "/../../features");
    Agent::cucumber()
        .fail_on_skipped()
        .filter_run_and_exit(features, |feature, rule, scenario| {
            !operation_steps::tagged(feature, rule, scenario, "e2e")
                && !operation_steps::tagged(feature, rule, scenario, "ui")
        })
        .await;
}
