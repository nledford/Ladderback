//! Runs the `@e2e` scenarios in `features/` against the real `steading`
//! binary: operations run in real `steading worker` processes (§13.2).

use std::sync::Arc;
use std::time::Duration;

use app::execution::RunOperation;
use app::ports::{Engine, FakeEnd, FakeJob, Job};
use cucumber::World;
use domain::execution::Operation;
use domain::{OperationId, Uuid};
use engine::WorkerEngine;
use platform::SystemClock;

#[path = "../crates/app/tests/steps/operation.rs"]
mod operation_steps;

const STEADING: &str = env!("CARGO_BIN_EXE_steading");

#[derive(World)]
#[world(init = Self::new)]
struct Agent {
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
        Self {
            engine: Arc::new(WorkerEngine::new(STEADING, ["worker"])),
            job: FakeJob {
                steps: 1,
                step_delay: Duration::from_millis(10),
                warnings: Vec::new(),
                end: FakeEnd::Succeed,
            },
            operation: None,
        }
    }

    fn make_engine_unavailable(&mut self) {
        let missing = std::env::temp_dir().join("steading-e2e-no-such-binary");
        self.engine = Arc::new(WorkerEngine::new(missing, ["worker"]));
    }

    async fn run_operation(&mut self) {
        let use_case = RunOperation::new(self.engine.clone(), Arc::new(SystemClock));
        let id = OperationId::from_uuid(Uuid::from_u128(1));
        self.operation = Some(use_case.run(id, Job::Fake(self.job.clone())).await);
    }

    fn operation(&self) -> &Operation {
        self.operation.as_ref().expect("the operation has run")
    }
}

#[tokio::main]
async fn main() {
    // See crates/app/tests/acceptance.rs: nothing to list for libtest.
    if std::env::args().any(|a| a == "--list") {
        return;
    }
    let features = concat!(env!("CARGO_MANIFEST_DIR"), "/features");
    Agent::cucumber()
        .fail_on_skipped()
        .filter_run_and_exit(features, |feature, rule, scenario| {
            operation_steps::tagged(feature, rule, scenario, "e2e")
        })
        .await;
}
