//! Runs every scenario in `features/` tagged neither `@e2e` nor `@ui`,
//! driving use cases with in-memory fakes and a fake clock (§13.2).

use std::sync::Arc;
use std::time::Duration;

use app::execution::RunOperation;
use app::fakes::{FakeClock, SimulatedEngine};
use app::ports::{Clock, Engine, EngineError, FakeEnd, FakeJob, Job, RunningJob};
use async_trait::async_trait;
use cucumber::{World, when};
use domain::execution::Operation;
use domain::{OperationId, SignedDuration, Timestamp, Uuid};

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
                deadline_budget: None,
            },
            operation: None,
        }
    }

    fn make_engine_unavailable(&mut self) {
        self.engine = Arc::new(Unavailable);
    }

    async fn run_operation(&mut self) {
        self.run_operation_until(futures::future::pending()).await;
    }

    async fn run_operation_until(&mut self, cancel: impl Future<Output = ()> + Send) {
        let use_case = RunOperation::new(self.engine.clone(), self.clock.clone());
        let id = OperationId::from_uuid(Uuid::from_u128(1));
        let job = Job::Fake(self.job.clone());
        self.operation = Some(use_case.run(id, job, cancel).await);
    }

    fn operation(&self) -> &Operation {
        self.operation.as_ref().expect("the operation has run")
    }
}

/// The shared clock jumps straight to each sleeper's deadline, so the engine
/// would never let a cancel in; this scenario moves time by hand instead.
#[when(expr = "the agent runs the operation and the user cancels it after {int} minute(s)")]
async fn run_and_cancel(agent: &mut Agent, minutes: u32) {
    let clock = Arc::new(FakeClock::new(agent.clock.now()));
    let use_case = RunOperation::new(Arc::new(SimulatedEngine::new(clock.clone())), clock.clone());
    let (cancel, cancelled) = futures::channel::oneshot::channel::<()>();
    let job = Job::Fake(agent.job.clone());
    let id = OperationId::from_uuid(Uuid::from_u128(1));
    let running = tokio::spawn(async move {
        use_case
            .run(id, job, async move {
                let _ = cancelled.await;
            })
            .await
    });
    for _ in 0..minutes {
        settle().await;
        clock.advance(SignedDuration::from_mins(1));
    }
    settle().await;
    cancel.send(()).expect("the operation is still running");
    agent.operation = Some(running.await.expect("the operation ran"));
}

/// Lets spawned work run until it waits on something.
async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

/// An engine whose worker can't be launched.
struct Unavailable;

#[async_trait]
impl Engine for Unavailable {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
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
