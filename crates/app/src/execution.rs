//! Execution use cases.

use std::pin::pin;
use std::sync::Arc;

use domain::execution::deadline::Deadline;
use domain::execution::full_disk_access::FullDiskAccess;
use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning, WarningKind,
};
use domain::planning::{HookEntry, OnFailure};
use domain::{OperationId, RepositoryId, SignedDuration, SnapshotId};
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{FutureExt, StreamExt, select_biased};

use crate::announce::{announce_end, announce_start};
use crate::full_disk_access::FullDiskAccessCheck;
use crate::log_write::save_within_bound;
use crate::ports::{
    Clock, Engine, EngineEvent, EngineResult, EventBus, HookContext, HookOutcome, HookPhase,
    HookRunner, HookSetup, HookStatus, Job, JobControl, OperationStore, RunningJob, StoreError,
};
use crate::queue::Place;

/// Runs one operation on the engine and records its lifecycle in the
/// operation log: queued now, running once the engine starts, then finished
/// as the engine reports. Each write is the whole operation as it stands, so
/// a write the log refused is made good by the next one.
///
/// - Cancelling stops the engine and records the operation as cancelled.
/// - A job with a deadline budget gets a deadline once the engine reports
///   when it read the index; still running then, the engine is stopped, with
///   its grace period, and the operation fails with `DeadlinePassed` (I3). The
///   engine enforces the same deadline itself, so either side may notice
///   first; an engine that reports it is given the grace period too. A backup
///   that archives winds down in it, saving the index of what it wrote, and
///   starts no snapshot write; one already writing its snapshot finishes it
///   (§4.1).
/// - An engine that breaks the [`Engine`] contract fails the operation rather
///   than leaving it running: a stream that ends without a result counts as an
///   engine exit, and an event out of order as a protocol failure.
/// - The log never decides how an operation ends (backups never depend on
///   the database, §9): a write it refuses is reported in
///   [`Finished::recording_errors`] and nothing else changes.
/// - With a bus ([`Self::with_events`]) the run announces how the operation
///   ended, once, after the ending hooks and the last write to the log, so a
///   page that reads the history on the news finds the end there; a cancelled
///   operation is not announced (§4.4).
pub struct RunOperation {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn OperationStore>,
    hooks: Option<HookPlan>,
    events: Option<Arc<dyn EventBus>>,
    full_disk_access: Option<Arc<FullDiskAccessCheck>>,
}

/// The runner and the three levels of hooks one run wraps (§6.1).
struct HookPlan {
    runner: Arc<dyn HookRunner>,
    setup: HookSetup,
}

/// An operation that is recorded as queued and waits to be run, with the job
/// it runs: what [`RunOperation::queue`] hands to [`RunOperation::run_queued`].
/// Dropping it unrun leaves the operation queued in the log for good.
#[must_use = "a queued operation that is never run stays queued in the log"]
pub struct Queued {
    run: Run,
    job: Job,
    /// Attached when the run starts ([`Self::with_warnings`]).
    warnings: Vec<Warning>,
}

impl Queued {
    /// `operation`, which the log already holds as queued, with the job it
    /// runs: a backup queued before the daemon stopped, taken up again by the
    /// persistent queue (§5, decision 73). Nothing is recorded: the log has
    /// it as it was queued, so it keeps its ID, trigger and queue time.
    pub fn resumed(operation: Operation, job: Job) -> Self {
        Self {
            run: Run {
                operation,
                result: None,
                recording_errors: Vec::new(),
                stopped_by_hook: false,
            },
            job,
            warnings: Vec::new(),
        }
    }

    /// The run with `warnings` it carries from its start: what reading the
    /// job's secrets told (#386). They are attached when the run starts, not
    /// here, so the queued row holds none and a run the queue takes up again,
    /// which reads its secrets anew, carries them once.
    pub fn with_warnings(self, warnings: Vec<Warning>) -> Self {
        Self { warnings, ..self }
    }
}

/// How the write that says an operation is running ended, or what cut it
/// short.
enum Written {
    Saved(Result<(), StoreError>),
    Cancelled,
    DeadlinePassed,
}

/// How a run ended: the operation as it finished, what the engine reported
/// when it succeeded, and what went wrong writing the operation log.
#[derive(Debug)]
pub struct Finished {
    pub operation: Operation,
    /// Set only when the operation accepted the engine's result: one the
    /// operation refused (a protocol violation) isn't kept.
    pub result: Option<EngineResult>,
    /// One for each write the log refused or never answered: a run writes at
    /// most four times. The last of them has no write after it to make it
    /// good.
    pub recording_errors: Vec<StoreError>,
    /// Why the stats a check reads before its end is recorded couldn't be
    /// read or kept (§16, decision 104); `None` for every other run. The
    /// daemon notes it in its log, and the check's own outcome stands.
    pub stats_failed: Option<crate::stats::StatsError>,
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
            events: None,
            full_disk_access: None,
        }
    }

    /// Runs the agent, repository and plan hooks of `setup` around the
    /// operation (§6.1): before-hooks before the engine, after-hooks on
    /// success, failed-hooks on failure, finally-hooks either way.
    pub fn with_hooks(mut self, runner: Arc<dyn HookRunner>, setup: HookSetup) -> Self {
        self.hooks = Some(HookPlan { runner, setup });
        self
    }

    /// Announces how each operation ends on `events` (§4.4): once, after the
    /// ending hooks and the last write to the log, whether the log took that
    /// write or not.
    pub fn with_events(mut self, events: Arc<dyn EventBus>) -> Self {
        self.events = Some(events);
        self
    }

    /// Runs `check`'s Full Disk Access probe before each backup, and records
    /// each warning as what it found explains it (§16, decision 115): a
    /// source item the system refused while the grant was missing gets the
    /// grant's own warning.
    pub fn with_full_disk_access(mut self, check: Arc<FullDiskAccessCheck>) -> Self {
        self.full_disk_access = Some(check);
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
        let queued = self.queue(id, trigger, job).await;
        self.run_queued(queued, cancel).await
    }

    /// Queues the operation for `job` and records it, before the engine is
    /// asked for anything: an operation that never gets going is in the
    /// history too, and a caller that answers its client first can answer
    /// with an operation the log already knows. [`Self::run_queued`] runs it.
    pub async fn queue(&self, id: OperationId, trigger: Trigger, job: Job) -> Queued {
        let mut run = Run {
            operation: Operation::queue(id, job.kind(), trigger, job.target(), self.clock.now()),
            result: None,
            recording_errors: Vec::new(),
            stopped_by_hook: false,
        };
        self.record(&mut run).await;
        Queued {
            run,
            job,
            warnings: Vec::new(),
        }
    }

    /// Records a backup the schedule started whose start was refused before
    /// anything ran (§5): one write, of the operation queued and failed with
    /// `reason` in the same instant, then the news that it failed, as any
    /// failed operation's. No hook runs and the engine is never asked.
    pub async fn refused(
        &self,
        id: OperationId,
        trigger: Trigger,
        target: Target,
        reason: FailureReason,
    ) -> Finished {
        let operation = Operation::refused(
            id,
            OperationKind::Backup,
            trigger,
            target,
            reason,
            self.clock.now(),
        );
        let mut run = Run {
            operation,
            result: None,
            recording_errors: Vec::new(),
            stopped_by_hook: false,
        };
        self.record(&mut run).await;
        announce_end(self.events.as_deref(), &run.operation).await;
        Finished {
            operation: run.operation,
            result: None,
            recording_errors: run.recording_errors,
            stats_failed: None,
        }
    }

    /// Ends `operation`, a backup the log held as queued when the daemon
    /// stopped, failed with `reason`: the next start took it up again (decision
    /// 73) and its start was refused before anything ran (§5). One write, of
    /// the end, then the news that it failed, as [`Self::refused`]. No hook
    /// runs and the engine is never asked.
    pub async fn refuse_resumed(&self, operation: Operation, reason: FailureReason) -> Finished {
        let mut run = Run {
            operation,
            result: None,
            recording_errors: Vec::new(),
            stopped_by_hook: false,
        };
        self.fail(&mut run, reason);
        self.record(&mut run).await;
        announce_end(self.events.as_deref(), &run.operation).await;
        Finished {
            operation: run.operation,
            result: None,
            recording_errors: run.recording_errors,
            stats_failed: None,
        }
    }

    /// Runs the job of an operation [`Self::queue`] recorded, as
    /// [`Self::run`] does.
    pub async fn run_queued(
        &self,
        queued: Queued,
        cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        let Queued {
            mut run,
            job,
            warnings,
        } = queued;
        for warning in warnings {
            let _ = run.operation.attach_warning(warning);
        }
        self.run_before_hooks(&mut run).await;
        if !run.stopped_by_hook {
            self.drive(&mut run, job, cancel).await;
        }
        self.run_ending_hooks(&mut run).await;
        self.record(&mut run).await;
        announce_end(self.events.as_deref(), &run.operation).await;
        Finished {
            operation: run.operation,
            result: run.result,
            recording_errors: run.recording_errors,
            stats_failed: None,
        }
    }

    /// Runs the job of an operation [`Self::queue`] recorded once `place`'s
    /// turn comes, as [`Self::run_queued`] does, and gives the place up when
    /// the run has ended (§5, the concurrency limit). A cancel that comes
    /// while it waits ends the operation cancelled at once: nothing ran, so
    /// no hook runs and the engine isn't asked.
    pub async fn run_in_turn(
        &self,
        queued: Queued,
        mut place: Place,
        cancel: impl Future<Output = ()> + Send,
    ) -> Finished {
        let mut cancel = pin!(cancel.fuse());
        let turn = select_biased! {
            () = cancel => false,
            () = place.turn().fuse() => true,
        };
        let finished = if turn {
            self.run_queued(queued, cancel).await
        } else {
            self.cancel_waiting(queued).await
        };
        drop(place);
        finished
    }

    /// Ends `queued` cancelled before its turn came: it records the end and
    /// announces it, as a run does, and nothing else.
    async fn cancel_waiting(&self, queued: Queued) -> Finished {
        let Queued { mut run, .. } = queued;
        self.cancel(&mut run);
        self.record(&mut run).await;
        announce_end(self.events.as_deref(), &run.operation).await;
        Finished {
            operation: run.operation,
            result: run.result,
            recording_errors: run.recording_errors,
            stats_failed: None,
        }
    }

    /// Plays the engine's events into the operation until it finishes.
    async fn drive(&self, run: &mut Run, job: Job, cancel: impl Future<Output = ()> + Send) {
        let budget = job.deadline_budget();
        let ruled = crate::locations::ruled_positions(&job);
        // Run just before the backup, so a grant given since the last probe
        // counts, and one taken away does too.
        let found = match (&self.full_disk_access, &job) {
            (Some(check), Job::Backup(_)) => Some(check.check().await),
            _ => None,
        };
        let RunningJob {
            events,
            mut control,
        } = match self.engine.run(run.operation.id(), job).await {
            Ok(running) => running,
            Err(error) => {
                self.fail(run, FailureReason::from(error));
                return;
            }
        };
        let mut cancel = pin!(cancel.fuse());
        // A cancel made before the engine was up is honoured at once, not
        // raced against the events the engine already has ready: the client
        // was told it was accepted.
        if (&mut cancel).now_or_never().is_some() {
            control.stop().await;
            self.cancel(run);
            return;
        }
        // Ended streams stay ended; `select_biased!` needs that guarantee.
        let mut events = events.fuse();
        let mut deadline: Option<Deadline> = None;
        let mut timer: Fuse<BoxFuture<'_, ()>> = Fuse::terminated();
        loop {
            select_biased! {
                event = events.next() => {
                    let Some(event) = event else {
                        self.fail(run, self.reason_events_ended(deadline));
                        return;
                    };
                    let is_start = matches!(event, EngineEvent::Started { .. });
                    // The repository the engine says it opened, for the news
                    // of the start: a first job knew none (decision 57).
                    let opened = match &event {
                        EngineEvent::Started { repository, .. } => repository.clone(),
                        _ => None,
                    };
                    self.arm_deadline(&event, budget, &mut deadline, &mut timer);
                    self.keep_result(run, &event).await;
                    self.apply(run, event, &ruled, found);
                    if run.operation.status().is_finished() {
                        stop_past_deadline(run, &mut *control).await;
                        return;
                    }
                    if is_start {
                        let announced = self
                            .announce_running(
                                run,
                                opened.as_ref(),
                                &mut *control,
                                &mut cancel,
                                &mut timer,
                            )
                            .await;
                        if !announced {
                            return;
                        }
                    }
                    // Checked on every event, so a stream that never pauses
                    // can't keep the timer from its turn.
                    if deadline.is_some_and(|d| d.has_passed(self.clock.now())) {
                        control.stop().await;
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
                    // The engine's grace period lets a backup that archives
                    // wind down: it saves the index of what it wrote and
                    // starts no snapshot write, but finishes one it began
                    // (§4.1).
                    control.stop().await;
                    self.fail(run, FailureReason::DeadlinePassed);
                    return;
                }
            }
        }
    }

    /// Why a run whose engine's events ended before it finished failed: its
    /// deadline, if that has passed, or else the engine's exit.
    fn reason_events_ended(&self, deadline: Option<Deadline>) -> FailureReason {
        match deadline {
            Some(d) if d.has_passed(self.clock.now()) => FailureReason::DeadlinePassed,
            _ => FailureReason::EngineExited { exit_code: None },
        }
    }

    /// Sets the run's deadline, once, when the engine says it read the
    /// index of a job that has a deadline budget, and the timer that fires
    /// at it.
    fn arm_deadline<'a>(
        &'a self,
        event: &EngineEvent,
        budget: Option<SignedDuration>,
        deadline: &mut Option<Deadline>,
        timer: &mut Fuse<BoxFuture<'a, ()>>,
    ) {
        if let EngineEvent::Started { index_read_at, .. } = event
            && let Some(budget) = budget
            && timer.is_terminated()
        {
            let d = Deadline::after_index_read(*index_read_at, budget);
            *deadline = Some(d);
            *timer = self.clock.sleep_until(d.at()).fuse();
        }
    }

    /// A success's result is in the log on the running operation before any
    /// ending hook runs: a backup's snapshot is written by then, so an
    /// orderly stop that ends the run in its after hooks keeps it on the
    /// record (decision 92). A result the operation can't take, as one
    /// before its start, is never written.
    async fn keep_result(&self, run: &mut Run, event: &EngineEvent) {
        if let EngineEvent::Succeeded(result) = event
            && run.operation.status() == &OperationStatus::Running
        {
            let saved =
                save_within_bound(&*self.clock, &*self.store, &run.operation, Some(result)).await;
            if let Err(error) = saved {
                run.recording_errors.push(error);
            }
        }
    }

    /// Writes the operation that has just started running, then announces
    /// it. The operation is running now. Nothing else it does changes what
    /// the log shows until it finishes, so progress and warnings aren't
    /// written one by one. A cancel or the deadline doesn't wait for the
    /// write: a log that stalls must not hold them. Either one ends the run
    /// here, and then this returns `false`.
    async fn announce_running(
        &self,
        run: &mut Run,
        opened: Option<&RepositoryId>,
        control: &mut dyn JobControl,
        mut cancel: &mut (impl FusedFuture<Output = ()> + Unpin + Send),
        mut timer: &mut Fuse<BoxFuture<'_, ()>>,
    ) -> bool {
        let written = {
            let write = save_within_bound(
                &*self.clock,
                &*self.store,
                &run.operation,
                run.result.as_ref(),
            );
            let mut write = pin!(write.fuse());
            select_biased! {
                saved = write => Written::Saved(saved),
                () = cancel => Written::Cancelled,
                () = timer => Written::DeadlinePassed,
            }
        };
        match written {
            // Told only once the log has it, or has refused it: a page that
            // reads the history on the news finds the operation running
            // there (#305).
            Written::Saved(saved) => {
                if let Err(error) = saved {
                    run.recording_errors.push(error);
                }
                announce_start(self.events.as_deref(), &run.operation, opened).await;
                true
            }
            Written::Cancelled => {
                control.stop().await;
                self.cancel(run);
                false
            }
            Written::DeadlinePassed => {
                control.stop().await;
                self.fail(run, FailureReason::DeadlinePassed);
                false
            }
        }
    }

    /// Applies `event` to the run's operation; `ruled` are the positions of
    /// the repository's locations that have a rule, and `found` what the Full
    /// Disk Access probe found before a backup, which explains its warnings.
    fn apply(
        &self,
        run: &mut Run,
        event: EngineEvent,
        ruled: &[u8],
        found: Option<FullDiskAccess>,
    ) {
        let at = self.clock.now();
        let applied = match event {
            // A backup queued before its repository's ID was known names
            // the repository from its start on (§9).
            EngineEvent::Started { repository, .. } => {
                run.operation.start_on(at, repository.as_ref())
            }
            EngineEvent::Progress(_) => run.operation.report_progress(),
            // A dump is a download's read, never run as an operation.
            EngineEvent::Dumping(_) | EngineEvent::Piece(_) => Ok(()),
            EngineEvent::Warning(warning) => run.operation.record_warning(match found {
                Some(found) => found.explain(warning),
                None => warning,
            }),
            EngineEvent::Succeeded(result) => {
                let succeeded = run.operation.succeed(at);
                if succeeded.is_ok() {
                    run.result = Some(result);
                }
                succeeded
            }
            EngineEvent::Failed(reason) => run.operation.fail(reason, at),
            // A location the choice probed had no rule or one that held.
            EngineEvent::Located(record) => run
                .operation
                .record_locations(record, |position| ruled.contains(&position)),
        };
        if let Err(error) = applied {
            let transition = error.transition;
            self.fail(run, FailureReason::ProtocolViolated { transition });
        }
    }

    /// Writes the operation as it stands. A refused write, or one that
    /// stalls past [`crate::log_write::SAVE_BOUND`], is noted for the caller and nothing more.
    async fn record(&self, run: &mut Run) {
        let saved = save_within_bound(
            &*self.clock,
            &*self.store,
            &run.operation,
            run.result.as_ref(),
        )
        .await;
        if let Err(error) = saved {
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
            let outcome = hooks.runner.run(entry, &context, &setup.redactor).await;
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
                let outcome = hooks.runner.run(entry, &context, &setup.redactor).await;
                if !outcome.succeeded() && entry.on_failure != OnFailure::Ignore {
                    self.warn(run, phase, &outcome);
                }
            }
        }
        for entry in ending_list(&setup, HookPhase::Finally) {
            let context = self.context(run, &setup, HookPhase::Finally);
            let outcome = hooks.runner.run(entry, &context, &setup.redactor).await;
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

/// Stops the engine of a run that has just finished, if it finished by
/// passing its deadline. A result stands even past the deadline: the
/// snapshot it reports was written already. Dropping the job stops whatever
/// the engine still runs, but for a backup the engine stopped at its
/// deadline, which may still be saving the index of its packs or finishing a
/// snapshot write it began before the deadline.
async fn stop_past_deadline(run: &Run, control: &mut dyn JobControl) {
    if matches!(
        run.operation.status(),
        OperationStatus::Failed {
            reason: FailureReason::DeadlinePassed
        }
    ) {
        control.stop().await;
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
        FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning,
        WarningKind,
    };
    use domain::planning::{HookEntry, Hooks, OnFailure};
    use domain::{OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId, Timestamp};

    use super::{HookPhase, HookSetup, Run, RunOperation, ending_list, ending_phase, snapshot_of};
    use crate::fakes::{FakeClock, InMemoryOperationStore, SimulatedEngine};
    use crate::ports::{BackupSummary, EngineEvent, EngineResult};

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
            redactor: Default::default(),
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

    /// A backup queued from an entry that recorded no repository ID names
    /// none until its engine starts; from its start the log names the
    /// repository the engine opened (§9).
    #[test]
    fn a_run_queued_before_its_repository_was_known_names_the_one_its_engine_opened() {
        let opened = RepositoryId::parse(&"07".repeat(32)).expect("a repository ID");
        let use_case = use_case();
        let mut run = run_of(operation(OperationKind::Backup));
        use_case.apply(
            &mut run,
            EngineEvent::Started {
                index_read_at: t(1),
                repository: Some(opened.clone()),
            },
            &[],
            None,
        );
        assert_eq!(run.operation.status(), &OperationStatus::Running);
        assert_eq!(run.operation.target().repository, Some(opened));
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
            bytes_processed: Some(100),
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

    /// What the job's secrets told is the run's from its start, not the
    /// queued row's (#386).
    #[tokio::test]
    async fn warnings_handed_with_a_queued_run_are_attached_when_it_starts() {
        use crate::ports::{FakeEnd, FakeJob, Job, OperationStore};

        let readable = Warning {
            kind: WarningKind::SecretFileReadable,
            item: Some("/home/demo/nas-password".to_owned()),
            during: "secret".to_owned(),
            detail: "0644".to_owned(),
        };
        let clock = Arc::new(FakeClock::new(t(0)));
        let store = Arc::new(InMemoryOperationStore::new());
        let use_case = RunOperation::new(
            Arc::new(SimulatedEngine::new(clock.clone())),
            clock,
            store.clone(),
        );
        let id = OperationId::from_uuid(uuid::Uuid::from_u128(7));
        let job = Job::Fake(FakeJob {
            steps: 0,
            step_delay: std::time::Duration::ZERO,
            warnings: Vec::new(),
            end: FakeEnd::Succeed,
            deadline_budget: None,
        });
        let queued = use_case
            .queue(id, Trigger::Manual, job)
            .await
            .with_warnings(vec![readable.clone()]);
        let held = store.get(id).await.expect("the log answers");
        assert_eq!(
            held.map(|detail| detail.warnings),
            Some(Vec::new()),
            "the queued row holds none"
        );
        let finished = use_case
            .run_queued(queued, std::future::pending::<()>())
            .await;
        assert_eq!(finished.operation.warnings(), [readable]);
        assert_eq!(
            finished.operation.status(),
            &OperationStatus::SucceededWithWarnings
        );
    }

    /// A file the run's start read and its job met again, as a choice of
    /// location that reads the same password file does, is named once
    /// (#386).
    #[tokio::test]
    async fn a_readable_secret_file_met_at_the_start_and_in_the_job_is_named_once() {
        use crate::ports::{FakeEnd, FakeJob, Job};

        let readable = Warning {
            kind: WarningKind::SecretFileReadable,
            item: Some("/home/demo/nas-password".to_owned()),
            during: "secret".to_owned(),
            detail: "0644".to_owned(),
        };
        let clock = Arc::new(FakeClock::new(t(0)));
        let use_case = RunOperation::new(
            Arc::new(SimulatedEngine::new(clock.clone())),
            clock,
            Arc::new(InMemoryOperationStore::new()),
        );
        let job = Job::Fake(FakeJob {
            steps: 0,
            step_delay: std::time::Duration::ZERO,
            warnings: vec![readable.clone()],
            end: FakeEnd::Succeed,
            deadline_budget: None,
        });
        let queued = use_case
            .queue(
                OperationId::from_uuid(uuid::Uuid::from_u128(8)),
                Trigger::Manual,
                job,
            )
            .await
            .with_warnings(vec![readable.clone()]);
        let finished = use_case
            .run_queued(queued, std::future::pending::<()>())
            .await;
        assert_eq!(finished.operation.warnings(), [readable]);
        assert_eq!(finished.operation.warning_count(), 1);
    }

    #[tokio::test]
    async fn ending_hooks_attach_a_warning_only_when_not_ignored() {
        use crate::fakes::{FakeHookRunner, Scripted};

        let runner = Arc::new(FakeHookRunner::new());
        runner.script("after-error", Scripted::fails_with(4));
        runner.script("after-ignore", Scripted::fails_with(5));
        runner.script("finally-error", Scripted::fails_with(6));
        let mut plan = Hooks::default();
        plan.run_after.push(entry("after-error")); // on-failure error
        plan.run_after.push(
            HookEntry::new("after-ignore", ["demo"], OnFailure::Ignore)
                .expect("the demo hook is valid"),
        );
        plan.run_finally.push(entry("finally-error")); // on-failure error
        let use_case = use_case().with_hooks(
            runner,
            HookSetup {
                agent: Hooks::default(),
                repository: Hooks::default(),
                plan,
                plan_id: PlanId::parse("demo-plan").expect("the demo plan parses"),
                repository_alias: RepositoryAlias::parse("nas").expect("the demo alias parses"),
                redactor: Default::default(),
            },
        );
        let mut run = succeeded_run();
        use_case.run_ending_hooks(&mut run).await;
        let warnings = run.operation.warnings();
        assert_eq!(
            warnings
                .iter()
                .map(|warning| warning.detail.as_str())
                .collect::<Vec<_>>(),
            ["4", "6"],
            "the error hooks of both ending loops warn, the ignored one is silent"
        );
    }

    /// An engine whose job reports `events`, then ends.
    struct Reports(Vec<EngineEvent>);

    #[async_trait::async_trait]
    impl crate::ports::Engine for Reports {
        async fn run(
            &self,
            _: OperationId,
            _: crate::ports::Job,
        ) -> Result<crate::ports::RunningJob, crate::ports::EngineError> {
            use futures::StreamExt;
            Ok(crate::ports::RunningJob {
                events: futures::stream::iter(self.0.clone()).boxed(),
                control: Box::new(Ended),
            })
        }
    }

    struct Ended;

    #[async_trait::async_trait]
    impl crate::ports::JobControl for Ended {
        async fn stop(&mut self) {}
    }

    /// Every hook reads the log and succeeds: what it reads is what an
    /// orderly stop that came while the hook ran would find (decision 92).
    struct ReadsTheLog {
        store: Arc<InMemoryOperationStore>,
        seen: std::sync::Mutex<Vec<crate::ports::OperationRecord>>,
    }

    #[async_trait::async_trait]
    impl crate::ports::HookRunner for ReadsTheLog {
        async fn run(
            &self,
            _: &HookEntry,
            context: &crate::ports::HookContext,
            _: &domain::secrets::Redactor,
        ) -> crate::ports::HookOutcome {
            use crate::ports::OperationStore;
            let held = self.store.get(context.operation).await;
            if let Ok(Some(detail)) = held {
                self.seen.lock().unwrap().push(detail.record);
            }
            crate::ports::HookOutcome {
                status: crate::ports::HookStatus::Succeeded,
                stdout: crate::ports::HookOutput::default(),
                stderr: crate::ports::HookOutput::default(),
            }
        }

        fn environment_secrets(&self) -> Vec<domain::secrets::SecretValue> {
            Vec::new()
        }
    }

    /// The log as each ending hook of a run whose engine reports `events`
    /// finds it: one after, failed and finally hook at the plan's level.
    async fn the_log_as_the_ending_hooks_find_it(
        events: Vec<EngineEvent>,
    ) -> Vec<crate::ports::OperationRecord> {
        use crate::ports::{FakeEnd, FakeJob, Job};

        let clock = Arc::new(FakeClock::new(t(0)));
        let store = Arc::new(InMemoryOperationStore::new());
        let runner = Arc::new(ReadsTheLog {
            store: Arc::clone(&store),
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let plan = Hooks {
            run_before: Vec::new(),
            run_after: vec![entry("after")],
            run_failed: vec![entry("failed")],
            run_finally: vec![entry("finally")],
        };
        let use_case = RunOperation::new(Arc::new(Reports(events)), clock, store).with_hooks(
            runner.clone(),
            HookSetup {
                agent: Hooks::default(),
                repository: Hooks::default(),
                plan,
                plan_id: PlanId::parse("demo-plan").expect("the demo plan parses"),
                repository_alias: RepositoryAlias::parse("nas").expect("the demo alias parses"),
                redactor: Default::default(),
            },
        );
        let job = Job::Fake(FakeJob {
            steps: 0,
            step_delay: std::time::Duration::ZERO,
            warnings: Vec::new(),
            end: FakeEnd::Succeed,
            deadline_budget: None,
        });
        let _ = use_case
            .run(
                OperationId::from_uuid(uuid::Uuid::from_u128(9)),
                Trigger::Manual,
                job,
                std::future::pending::<()>(),
            )
            .await;
        runner.seen.lock().unwrap().clone()
    }

    fn backup_result() -> EngineResult {
        EngineResult::Backup(BackupSummary {
            repository: RepositoryId::parse(&"1".repeat(64)).expect("the demo repository parses"),
            snapshot: SnapshotId::parse(&"2".repeat(64)).expect("the demo snapshot parses"),
            parent: None,
            files_new: 1,
            files_changed: 0,
            files_unmodified: 0,
            bytes_processed: Some(100),
            data_added: 10,
            data_added_packed: 5,
            error_count: 0,
        })
    }

    /// A backup's snapshot is on its record, the operation still running,
    /// before its after hooks run: an orderly stop that ends the run in them
    /// keeps the snapshot on the record (decision 92).
    #[tokio::test]
    async fn a_backups_snapshot_is_on_its_running_record_before_its_after_hooks_run() {
        let seen = the_log_as_the_ending_hooks_find_it(vec![
            EngineEvent::Started {
                index_read_at: t(1),
                repository: None,
            },
            EngineEvent::Succeeded(backup_result()),
        ])
        .await;
        let first = seen.first().expect("the after hook read the log");
        assert_eq!(first.status, OperationStatus::Running);
        assert_eq!(first.result, Some(backup_result()));
    }

    /// A result the operation can't take, as one before its start, is never
    /// in the log: the run fails on it as a protocol violation.
    #[tokio::test]
    async fn a_result_before_the_start_is_never_in_the_log() {
        let seen =
            the_log_as_the_ending_hooks_find_it(vec![EngineEvent::Succeeded(backup_result())])
                .await;
        let first = seen.first().expect("an ending hook read the log");
        assert_eq!(first.result, None);
    }

    /// A secret only the run's redactor knows.
    const CANARY: &str = "canary-of-the-run";

    /// Every hook writes the canary and succeeds; what the redactor it was
    /// handed made of the canary is recorded with its program.
    struct MasksTheCanary {
        masked: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl crate::ports::HookRunner for MasksTheCanary {
        async fn run(
            &self,
            entry: &HookEntry,
            _: &crate::ports::HookContext,
            redactor: &domain::secrets::Redactor,
        ) -> crate::ports::HookOutcome {
            self.masked
                .lock()
                .unwrap()
                .push((entry.command.program().to_owned(), redactor.redact(CANARY)));
            crate::ports::HookOutcome {
                status: crate::ports::HookStatus::Succeeded,
                stdout: crate::ports::HookOutput::default(),
                stderr: crate::ports::HookOutput::default(),
            }
        }

        fn environment_secrets(&self) -> Vec<domain::secrets::SecretValue> {
            Vec::new()
        }
    }

    /// Every hook of a run is handed the run's redactor, the agent's, the
    /// repository's and the plan's, in every phase: before and after a run
    /// that succeeded, failed and finally after one that failed (security.md
    /// §10.2, #853).
    #[tokio::test]
    async fn every_hook_of_a_run_is_handed_the_runs_redactor() {
        use domain::secrets::{Redactor, SecretValue};

        let runner = Arc::new(MasksTheCanary {
            masked: std::sync::Mutex::new(Vec::new()),
        });
        let use_case = use_case().with_hooks(
            runner.clone(),
            HookSetup {
                redactor: Arc::new(Redactor::new([&SecretValue::new(CANARY)])),
                ..setup()
            },
        );
        use_case
            .run_before_hooks(&mut run_of(operation(OperationKind::Backup)))
            .await;
        use_case.run_ending_hooks(&mut succeeded_run()).await;
        use_case.run_ending_hooks(&mut failed_run()).await;

        let masked = runner.masked.lock().unwrap().clone();
        let programs: std::collections::BTreeSet<&str> =
            masked.iter().map(|(program, _)| program.as_str()).collect();
        let every_hook: std::collections::BTreeSet<String> = ["agent", "repository", "plan"]
            .iter()
            .flat_map(|level| {
                ["before", "after", "failed", "finally"].map(|phase| format!("{level}-{phase}"))
            })
            .collect();
        assert_eq!(
            programs,
            every_hook.iter().map(String::as_str).collect(),
            "every hook of every level and phase ran"
        );
        for (program, text) in &masked {
            assert_eq!(text, "***", "{program} was handed the run's redactor");
        }
    }
}
