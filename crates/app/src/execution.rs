//! Execution use cases.

use std::pin::pin;
use std::sync::Arc;

use domain::OperationId;
use domain::execution::deadline::Deadline;
use domain::execution::{FailureReason, Operation, Trigger};
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{FutureExt, StreamExt, select_biased};

use crate::ports::{
    Clock, Engine, EngineError, EngineEvent, EngineResult, Job, OperationStore, RunningJob,
    StoreError,
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
        }
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
        };
        // Queued before the engine is asked for anything, so an operation
        // that never gets going is in the history too.
        self.record(&mut run).await;
        self.drive(&mut run, job, cancel).await;
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

    fn fail(&self, run: &mut Run, reason: FailureReason) {
        let _ = run.operation.fail(reason, self.clock.now());
    }
}
