//! Execution use cases.

use std::pin::pin;
use std::sync::Arc;

use domain::execution::deadline::Deadline;
use domain::execution::{FailureReason, Operation, OperationStatus, Trigger, Warning, WarningKind};
use domain::planning::{HookEntry, OnFailure};
use domain::{OperationId, SnapshotId};
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{FutureExt, StreamExt, select_biased};

use crate::ports::{
    Clock, Engine, EngineError, EngineEvent, EngineResult, HookContext, HookOutcome, HookPhase,
    HookRunner, HookSetup, HookStatus, Job, OperationStore, RunningJob, StoreError,
};

/// Runs one operation on the engine and records its lifecycle in the
/// operation log: queued now, running once the engine starts, then finished
/// as the engine reports. Each write is the whole operation as it stands, so
/// a write the log refused is made good by the next one.
///
/// - Cancelling stops the engine and records the operation as cancelled.
/// - A job with a deadline budget gets a deadline once the engine reports
///   when it read the index; still running then, the engine is stopped and
///   the operation fails with `DeadlinePassed` (I3). The engine enforces the
///   same deadline itself, so either side may notice first.
/// - An engine that breaks the [`Engine`] contract fails the operation rather
///   than leaving it running: a stream that ends without a result counts as an
///   engine exit, and an event out of order as a protocol failure.
/// - The log never decides how an operation ends (backups never depend on
///   the database, §9): a write it refuses is reported in
///   [`Finished::recording_errors`] and nothing else changes.
pub struct RunOperation {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn OperationStore>,
    hooks: Option<HookPlan>,
}

/// The runner and the three levels of hooks one run wraps (§6.1).
struct HookPlan {
    runner: Arc<dyn HookRunner>,
    setup: HookSetup,
}

/// How a run ended: the operation as it finished, what the engine reported
/// when it succeeded, and what went wrong writing the operation log.
#[derive(Debug)]
pub struct Finished {
    pub operation: Operation,
    /// Set only when the operation accepted the engine's result: one the
    /// operation refused (a protocol violation) isn't kept.
    pub result: Option<EngineResult>,
    /// One for each write the log refused: a run writes at most three times.
    pub recording_errors: Vec<StoreError>,
}

/// A run in progress.
struct Run {
    operation: Operation,
    result: Option<EngineResult>,
    recording_errors: Vec<StoreError>,
    /// A before-hook with `on-failure = error` stopped the run before the
    /// engine: only the finally-hooks still run.
    stopped_by_hook: bool,
}

impl RunOperation {
    pub fn new(
        engine: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        store: Arc<dyn OperationStore>,
    ) -> Self {
        Self {
            engine,
            clock,
            store,
            hooks: None,
        }
    }

    /// Runs the agent, repository and plan hooks of `setup` around the
    /// operation (§6.1): before-hooks before the engine, after-hooks on
    /// success, failed-hooks on failure, finally-hooks either way.
    pub fn with_hooks(mut self, runner: Arc<dyn HookRunner>, setup: HookSetup) -> Self {
        self.hooks = Some(HookPlan { runner, setup });
        self
    }

    /// Runs `job`, started by `trigger`. When `cancel` completes, the
    /// operation is cancelled.
    pub async fn run(
        &self,
        id: OperationId,
        trigger: Trigger,
        job: Job,
        cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        let mut run = Run {
            operation: Operation::queue(id, job.kind(), trigger, job.target(), self.clock.now()),
            result: None,
            recording_errors: Vec::new(),
            stopped_by_hook: false,
        };
        // Queued before the engine is asked for anything, so an operation
        // that never gets going is in the history too.
        self.record(&mut run).await;
        self.run_before_hooks(&mut run).await;
        if !run.stopped_by_hook {
            self.drive(&mut run, job, cancel).await;
        }
        self.run_ending_hooks(&mut run).await;
        self.record(&mut run).await;
        Finished {
            operation: run.operation,
            result: run.result,
            recording_errors: run.recording_errors,
        }
    }

    /// Plays the engine's events into the operation until it finishes.
    async fn drive(&self, run: &mut Run, job: Job, cancel: impl Future<Output = ()> + Send) {
        let budget = job.deadline_budget();
        let RunningJob {
            events,
            mut control,
        } = match self.engine.run(run.operation.id(), job).await {
            Ok(running) => running,
            Err(error) => {
                let reason = match error {
                    EngineError::Unavailable { detail } => {
                        FailureReason::EngineUnavailable { detail }
                    }
                    EngineError::StartTimedOut => FailureReason::EngineStartTimedOut,
                };
                self.fail(run, reason);
                return;
            }
        };
        let mut cancel = pin!(cancel.fuse());
        // Ended streams stay ended; `select_biased!` needs that guarantee.
        let mut events = events.fuse();
        let mut deadline: Option<Deadline> = None;
        let mut timer: Fuse<BoxFuture<'_, ()>> = Fuse::terminated();
        loop {
            select_biased! {
                event = events.next() => {
                    let Some(event) = event else {
                        let reason = match deadline {
                            Some(d) if d.has_passed(self.clock.now()) => {
                                FailureReason::DeadlinePassed
                            }
                            _ => FailureReason::EngineExited { exit_code: None },
                        };
                        self.fail(run, reason);
                        return;
                    };
                    let is_start = matches!(event, EngineEvent::Started { .. });
                    if let EngineEvent::Started { index_read_at } = &event
                        && let Some(budget) = budget
                        && timer.is_terminated()
                    {
                        let d = Deadline::after_index_read(*index_read_at, budget);
                        deadline = Some(d);
                        timer = self.clock.sleep_until(d.at()).fuse();
                    }
                    self.apply(run, event);
                    if run.operation.status().is_finished() {
                        // A result stands even past the deadline: the snapshot
                        // it reports was written already. Dropping the job
                        // stops whatever the engine still runs.
                        return;
                    }
                    if is_start {
                        // The operation is running now. Nothing else it does
                        // changes what the log shows until it finishes, so
                        // progress and warnings aren't written one by one.
                        self.record(run).await;
                    }
                    // Checked on every event, so a stream that never pauses
                    // can't keep the timer from its turn.
                    if deadline.is_some_and(|d| d.has_passed(self.clock.now())) {
                        self.fail(run, FailureReason::DeadlinePassed);
                        return;
                    }
                }
                () = cancel => {
                    control.stop().await;
                    self.cancel(run);
                    return;
                }
                () = timer => {
                    // No grace past the deadline: dropping the job on return
                    // kills it at once (§4.1).
                    self.fail(run, FailureReason::DeadlinePassed);
                    return;
                }
            }
        }
    }

    fn apply(&self, run: &mut Run, event: EngineEvent) {
        let at = self.clock.now();
        let applied = match event {
            EngineEvent::Started { .. } => run.operation.start(at),
            EngineEvent::Progress(_) => run.operation.report_progress(),
            EngineEvent::Warning(warning) => run.operation.record_warning(warning),
            EngineEvent::Succeeded(result) => {
                let succeeded = run.operation.succeed(at);
                if succeeded.is_ok() {
                    run.result = Some(result);
                }
                succeeded
            }
            EngineEvent::Failed(reason) => run.operation.fail(reason, at),
        };
        if let Err(error) = applied {
            let transition = error.transition;
            self.fail(run, FailureReason::ProtocolViolated { transition });
        }
    }

    /// Writes the operation as it stands. A refused write is noted for the
    /// caller and nothing more.
    async fn record(&self, run: &mut Run) {
        if let Err(error) = self.store.save(&run.operation, run.result.as_ref()).await {
            run.recording_errors.push(error);
        }
    }

    // `run` returns as soon as the operation finishes, so it only ever
    // cancels or fails an unfinished one; a finished one keeps the outcome
    // it already has.
    fn cancel(&self, run: &mut Run) {
        let _ = run.operation.cancel(self.clock.now());
    }

    /// Runs the before-hooks of the three levels, in §6.1's order: agent,
    /// repository, plan. A failing one ends the run before the engine
    /// starts, as its `on-failure` says: `error` fails the operation with
    /// the hook's own exit, `warn` attaches a warning and the run goes on,
    /// `ignore` changes nothing.
    async fn run_before_hooks(&self, run: &mut Run) {
        let Some(hooks) = &self.hooks else {
            return;
        };
        let setup = hooks.setup.clone();
        for entry in before_list(&setup) {
            let context = self.context(run, &setup, HookPhase::Before);
            let outcome = hooks.runner.run(entry, &context).await;
            if outcome.succeeded() {
                continue;
            }
            if entry.on_failure == OnFailure::Error {
                self.fail(
                    run,
                    FailureReason::HookFailed {
                        exit_code: exit_code_of(&outcome),
                    },
                );
                run.stopped_by_hook = true;
                return;
            }
            if entry.on_failure == OnFailure::Warn {
                self.warn(run, HookPhase::Before, &outcome);
            }
        }
    }

    /// Runs the after-hooks on success or the failed-hooks on failure, then
    /// the finally-hooks of the three levels, in §6.1's order: plan,
    /// repository, agent. None of them changes how a finished operation
    /// ended: a hook that failed with `error` or `warn` attaches a warning,
    /// and `ignore` changes nothing.
    async fn run_ending_hooks(&self, run: &mut Run) {
        let Some(hooks) = &self.hooks else {
            return;
        };
        let setup = hooks.setup.clone();
        let status = run.operation.status().clone();
        let phase = ending_phase(&status, run.stopped_by_hook);
        if let Some(phase) = phase {
            for entry in ending_list(&setup, phase) {
                let context = self.context(run, &setup, phase);
                let outcome = hooks.runner.run(entry, &context).await;
                if !outcome.succeeded() && entry.on_failure != OnFailure::Ignore {
                    self.warn(run, phase, &outcome);
                }
            }
        }
        for entry in ending_list(&setup, HookPhase::Finally) {
            let context = self.context(run, &setup, HookPhase::Finally);
            let outcome = hooks.runner.run(entry, &context).await;
            if !outcome.succeeded() && entry.on_failure != OnFailure::Ignore {
                self.warn(run, HookPhase::Finally, &outcome);
            }
        }
    }

    /// The context every hook of `phase` runs in. After success the
    /// snapshot is known; after a failure its code is.
    fn context(&self, run: &Run, setup: &HookSetup, phase: HookPhase) -> HookContext {
        HookContext {
            phase,
            plan: setup.plan_id.clone(),
            repository: setup.repository_alias.clone(),
            operation: run.operation.id(),
            snapshot: run.result.as_ref().and_then(snapshot_of),
            error: match run.operation.status() {
                OperationStatus::Failed { reason }
                    if phase == HookPhase::Failed || phase == HookPhase::Finally =>
                {
                    Some(reason.code().to_owned())
                }
                _ => None,
            },
        }
    }

    /// Attaches a hook's failure as a warning, with how it ended and no
    /// English: a code's own words are the renderer's business (§8.4).
    fn warn(&self, run: &mut Run, phase: HookPhase, outcome: &HookOutcome) {
        let warning = Warning {
            kind: WarningKind::Hook,
            item: None,
            during: phase.as_env().to_owned(),
            detail: outcome_detail(outcome),
        };
        let _ = run.operation.attach_warning(warning);
    }

    fn fail(&self, run: &mut Run, reason: FailureReason) {
        let _ = run.operation.fail(reason, self.clock.now());
    }
}

/// The before-hooks of the three levels in §6.1's order: agent,
/// repository, plan.
fn before_list(setup: &HookSetup) -> impl Iterator<Item = &'_ HookEntry> {
    setup
        .agent
        .run_before
        .iter()
        .chain(&setup.repository.run_before)
        .chain(&setup.plan.run_before)
}

/// The `phase` hooks of the three levels in §6.1's order: plan,
/// repository, agent.
fn ending_list(setup: &HookSetup, phase: HookPhase) -> impl Iterator<Item = &'_ HookEntry> {
    let plan = match phase {
        HookPhase::After => &setup.plan.run_after,
        HookPhase::Failed => &setup.plan.run_failed,
        _ => &setup.plan.run_finally,
    };
    let repository = match phase {
        HookPhase::After => &setup.repository.run_after,
        HookPhase::Failed => &setup.repository.run_failed,
        _ => &setup.repository.run_finally,
    };
    let agent = match phase {
        HookPhase::After => &setup.agent.run_after,
        HookPhase::Failed => &setup.agent.run_failed,
        _ => &setup.agent.run_finally,
    };
    plan.iter().chain(repository).chain(agent)
}

/// Which ending list runs for `status`: the after-hooks on a success, the
/// failed-hooks on a failure, and neither when a before-hook stopped the run
/// or the operation was cancelled. The finally-hooks always run.
fn ending_phase(status: &OperationStatus, stopped_by_hook: bool) -> Option<HookPhase> {
    match status {
        OperationStatus::Succeeded | OperationStatus::SucceededWithWarnings => {
            Some(HookPhase::After)
        }
        OperationStatus::Failed { .. } if !stopped_by_hook => Some(HookPhase::Failed),
        _ => None,
    }
}

/// The exit a hook's outcome reports, when it ran and gave one.
fn exit_code_of(outcome: &HookOutcome) -> Option<i32> {
    match outcome.status {
        HookStatus::Failed { exit_code } => exit_code,
        _ => None,
    }
}

/// How a failing hook ended, as a code the renderer can describe: its exit
/// status, `timeout`, or `not-started`.
fn outcome_detail(outcome: &HookOutcome) -> String {
    match outcome.status {
        HookStatus::Failed {
            exit_code: Some(code),
        } => code.to_string(),
        HookStatus::Failed { exit_code: None } => "no-exit".to_owned(),
        HookStatus::TimedOut => "timeout".to_owned(),
        HookStatus::NotStarted { .. } => "not-started".to_owned(),
        HookStatus::Succeeded => String::new(),
    }
}

/// The snapshot a successful backup produced, when it was one.
fn snapshot_of(result: &EngineResult) -> Option<SnapshotId> {
    match result {
        EngineResult::Backup(summary) => Some(summary.snapshot.clone()),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domain::execution::{
        FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger,
    };
    use domain::planning::{HookEntry, Hooks, OnFailure};
    use domain::{OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp};

    use super::{HookPhase, HookSetup, Run, RunOperation, ending_list, ending_phase, snapshot_of};
    use crate::fakes::{FakeClock, InMemoryOperationStore, SimulatedEngine};
    use crate::ports::{BackupSummary, EngineResult};

    fn t(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).expect("the demo time is valid")
    }

    fn entry(name: &str) -> HookEntry {
        HookEntry::new(name, ["demo"], OnFailure::Error).expect("the demo hook is valid")
    }

    /// A setup with one named hook in every list of every level, so a list
    /// that runs the wrong phase is told apart by the program name.
    fn setup() -> HookSetup {
        let lists = |word: &str| Hooks {
            run_before: vec![entry(&format!("{word}-before"))],
            run_after: vec![entry(&format!("{word}-after"))],
            run_failed: vec![entry(&format!("{word}-failed"))],
            run_finally: vec![entry(&format!("{word}-finally"))],
        };
        HookSetup {
            agent: lists("agent"),
            repository: lists("repository"),
            plan: lists("plan"),
            plan_id: PlanId::parse("demo-plan").expect("the demo plan parses"),
            repository_alias: RepositoryAlias::parse("nas").expect("the demo alias parses"),
        }
    }

    fn use_case() -> RunOperation {
        let clock = Arc::new(FakeClock::new(t(0)));
        RunOperation::new(
            Arc::new(SimulatedEngine::new(clock.clone())),
            clock,
            Arc::new(InMemoryOperationStore::new()),
        )
    }

    fn run_of(operation: Operation) -> Run {
        Run {
            operation,
            result: None,
            recording_errors: Vec::new(),
            stopped_by_hook: false,
        }
    }

    fn operation(kind: OperationKind) -> Operation {
        Operation::queue(
            OperationId::from_uuid(uuid::Uuid::from_u128(7)),
            kind,
            Trigger::Manual,
            Target::default(),
            t(0),
        )
    }

    fn failed_run() -> Run {
        let mut op = operation(OperationKind::Backup);
        op.start(t(1)).expect("the demo operation starts");
        op.fail(FailureReason::EngineExited { exit_code: Some(1) }, t(2))
            .expect("the demo operation fails");
        run_of(op)
    }

    fn succeeded_run() -> Run {
        let mut op = operation(OperationKind::Backup);
        op.start(t(1)).expect("the demo operation starts");
        op.succeed(t(2)).expect("the demo operation succeeds");
        run_of(op)
    }

    #[test]
    fn only_the_failed_and_finally_phases_carry_the_error_code() {
        let run = failed_run();
        let setup = setup();
        let use_case = use_case();
        for phase in [
            HookPhase::Before,
            HookPhase::After,
            HookPhase::Failed,
            HookPhase::Finally,
        ] {
            let want = matches!(phase, HookPhase::Failed | HookPhase::Finally)
                .then(|| "engine-exited".to_owned());
            assert_eq!(
                use_case.context(&run, &setup, phase).error,
                want,
                "LADDERBACK_ERROR of {phase:?}"
            );
        }
    }

    #[test]
    fn no_phase_of_a_success_carries_an_error_code() {
        let run = succeeded_run();
        let setup = setup();
        let use_case = use_case();
        for phase in [
            HookPhase::Before,
            HookPhase::After,
            HookPhase::Failed,
            HookPhase::Finally,
        ] {
            assert_eq!(
                use_case.context(&run, &setup, phase).error,
                None,
                "LADDERBACK_ERROR of {phase:?}"
            );
        }
    }

    #[test]
    fn every_ending_list_holds_only_its_own_phase() {
        let setup = setup();
        for (phase, word) in [
            (HookPhase::After, "after"),
            (HookPhase::Failed, "failed"),
            (HookPhase::Finally, "finally"),
        ] {
            let ran: Vec<String> = ending_list(&setup, phase)
                .map(|entry| entry.command.program().to_owned())
                .collect();
            assert_eq!(
                ran,
                [
                    format!("plan-{word}"),
                    format!("repository-{word}"),
                    format!("agent-{word}")
                ],
                "the {word}-hooks of the three levels, in order"
            );
        }
    }

    #[test]
    fn a_before_hook_stop_runs_neither_after_nor_failed_hooks() {
        let failed = OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: Some(1) },
        };
        assert_eq!(ending_phase(&failed, true), None);
        assert_eq!(ending_phase(&failed, false), Some(HookPhase::Failed));
    }

    #[test]
    fn a_success_runs_the_after_hooks_even_with_warnings() {
        assert_eq!(
            ending_phase(&OperationStatus::Succeeded, false),
            Some(HookPhase::After)
        );
        assert_eq!(
            ending_phase(&OperationStatus::SucceededWithWarnings, false),
            Some(HookPhase::After)
        );
    }

    #[test]
    fn only_a_backup_result_carries_a_snapshot() {
        let snapshot = SnapshotId::parse(&"1".repeat(64)).expect("the demo snapshot parses");
        let backup = EngineResult::Backup(BackupSummary {
            repository: RepositoryId::parse(&"1".repeat(64)).expect("the demo repository parses"),
            snapshot: snapshot.clone(),
            parent: None,
            files_new: 1,
            files_changed: 0,
            files_unmodified: 0,
            data_added: 10,
            data_added_packed: 5,
            error_count: 0,
        });
        assert_eq!(snapshot_of(&backup), Some(snapshot));
        assert_eq!(
            snapshot_of(&EngineResult::Fake { steps_completed: 1 }),
            None
        );
    }

    #[test]
    fn a_hook_warning_names_how_the_hook_ended() {
        use crate::ports::{HookOutcome, HookOutput, HookStatus};
        let outcome = |status: HookStatus| HookOutcome {
            status,
            stdout: HookOutput::default(),
            stderr: HookOutput::default(),
        };
        assert_eq!(
            super::outcome_detail(&outcome(HookStatus::Failed { exit_code: Some(3) })),
            "3"
        );
        assert_eq!(
            super::outcome_detail(&outcome(HookStatus::Failed { exit_code: None })),
            "no-exit"
        );
        assert_eq!(
            super::outcome_detail(&outcome(HookStatus::TimedOut)),
            "timeout"
        );
        assert_eq!(
            super::outcome_detail(&outcome(HookStatus::NotStarted {
                detail: "gone".to_owned()
            })),
            "not-started"
        );
        assert_eq!(super::outcome_detail(&outcome(HookStatus::Succeeded)), "");
    }
}
