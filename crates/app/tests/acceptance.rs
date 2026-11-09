//! Runs every scenario in `features/` tagged neither `@e2e` nor `@ui`,
//! driving use cases with in-memory fakes and a fake clock (§13.2).

use std::sync::Arc;
use std::time::Duration;

use app::execution::{Finished, RunOperation};
use app::fakes::{FakeClock, InMemoryOperationStore, SimulatedEngine};
use app::ports::{Clock, Engine, EngineError, FakeEnd, FakeJob, Job, OperationStore, RunningJob};
use async_trait::async_trait;
use cucumber::{World, when};
use domain::execution::{Operation, Trigger};
use domain::{OperationId, SignedDuration, Timestamp, Uuid};

#[path = "steps/config.rs"]
mod config_steps;

#[path = "steps/maintenance.rs"]
mod maintenance_steps;

#[path = "steps/operation.rs"]
mod operation_steps;

#[path = "steps/forget.rs"]
mod forget_steps;
#[path = "steps/secrets.rs"]
mod secrets_steps;

#[path = "steps/hooks.rs"]
mod hooks_steps;
#[path = "steps/repositories.rs"]
mod repositories_steps;

#[derive(World)]
#[world(init = Self::new)]
struct Agent {
    clock: Arc<FakeClock>,
    engine: Arc<dyn Engine>,
    store: Arc<InMemoryOperationStore>,
    job: FakeJob,
    finished: Option<Finished>,
    hooks: hooks_steps::HooksWorld,
    secrets: secrets_steps::SecretsWorld,
    config: config_steps::ConfigWorld,
    maintenance: maintenance_steps::MaintenanceWorld,
    repositories: repositories_steps::RepositoriesWorld,
    forget: forget_steps::ForgetWorld,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("job", &self.job)
            .field("finished", &self.finished)
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
            store: Arc::new(InMemoryOperationStore::new()),
            job: FakeJob {
                steps: 1,
                step_delay: Duration::from_secs(1),
                warnings: Vec::new(),
                end: FakeEnd::Succeed,
                deadline_budget: None,
            },
            finished: None,
            secrets: Default::default(),
            config: Default::default(),
            maintenance: Default::default(),
            hooks: hooks_steps::HooksWorld::new(),
            repositories: Default::default(),
            forget: Default::default(),
        }
    }

    fn make_engine_unavailable(&mut self) {
        self.engine = Arc::new(Unavailable);
    }

    async fn run_operation(&mut self) {
        self.run_operation_until(futures::future::pending()).await;
    }

    async fn run_operation_until(&mut self, cancel: impl Future<Output = ()> + Send) {
        let mut use_case =
            RunOperation::new(self.engine.clone(), self.clock.clone(), self.store.clone());
        if !self.hooks.setup.is_empty() {
            use_case = use_case.with_hooks(self.hooks.runner.clone(), self.hooks.setup.clone());
        }
        let id = OperationId::from_uuid(Uuid::from_u128(1));
        let job = Job::Fake(self.job.clone());
        self.finished = Some(use_case.run(id, Trigger::Manual, job, cancel).await);
    }

    fn finished(&self) -> &Finished {
        self.finished.as_ref().expect("the operation has run")
    }

    fn operation(&self) -> &Operation {
        &self.finished().operation
    }

    /// The operation log the runs write to.
    async fn store(&mut self) -> Arc<dyn OperationStore> {
        self.store.clone()
    }
}

/// The shared clock jumps straight to each sleeper's deadline, so the engine
/// would never let a cancel in; this scenario moves time by hand instead.
#[when(expr = "the agent runs the operation and the user cancels it after {int} minute(s)")]
async fn run_and_cancel(agent: &mut Agent, minutes: u32) {
    let clock = Arc::new(FakeClock::new(agent.clock.now()));
    let use_case = RunOperation::new(
        Arc::new(SimulatedEngine::new(clock.clone())),
        clock.clone(),
        agent.store.clone(),
    );
    let (cancel, cancelled) = futures::channel::oneshot::channel::<()>();
    let job = Job::Fake(agent.job.clone());
    let id = OperationId::from_uuid(Uuid::from_u128(1));
    let running = tokio::spawn(async move {
        use_case
            .run(id, Trigger::Manual, job, async move {
                let _ = cancelled.await;
            })
            .await
    });
    for _ in 0..minutes {
        until_sleeping(&clock).await;
        clock.advance(SignedDuration::from_mins(1));
    }
    until_sleeping(&clock).await;
    cancel.send(()).expect("the operation is still running");
    agent.finished = Some(running.await.expect("the operation ran"));
}

/// Waits until the engine sleeps on `clock`, so the next `advance` wakes it.
/// The operation runs on another worker thread; a fixed number of yields
/// doesn't guarantee it got that far, and an early `advance` loses a minute.
async fn until_sleeping(clock: &FakeClock) {
    for _ in 0..100_000 {
        if clock.sleepers() > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the engine never slept on the clock");
}

/// An engine whose worker can't be launched.
struct Unavailable;

#[async_trait]
impl Engine for Unavailable {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Err(EngineError::Unavailable {
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
    // `--name` and `--tags` narrow this runner's scenarios rather than
    // replace its filter (see `operation_steps::picked_by_cli_tags`).
    let mut cli = cucumber::cli::Opts::<_, _, _>::parsed();
    let name = cli.re_filter.take();
    let tags = cli.tags_filter.take();
    Agent::cucumber()
        .with_cli(cli)
        .fail_on_skipped()
        .filter_run_and_exit(features, move |feature, rule, scenario| {
            !operation_steps::tagged(feature, rule, scenario, "e2e")
                && !operation_steps::tagged(feature, rule, scenario, "ui")
                && name.as_ref().is_none_or(|re| re.is_match(&scenario.name))
                && operation_steps::picked_by_cli_tags(tags.as_ref(), feature, rule, scenario)
        })
        .await;
}
