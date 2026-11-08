//! Execution use cases.

use std::pin::pin;
use std::sync::Arc;

use domain::OperationId;
use domain::execution::deadline::Deadline;
use domain::execution::{FailureReason, Operation};
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{FutureExt, StreamExt, select_biased};

use crate::ports::{Clock, Engine, EngineEvent, Job, RunningJob};

/// Runs one operation on the engine and records its lifecycle: queued now,
/// running once the engine starts, then finished as the engine reports.
///
/// - Cancelling stops the engine and records the operation as cancelled.
/// - A job with a deadline budget gets a deadline once the engine reports
///   when it read the index; still running then, the engine is stopped and
///   the operation fails with `DeadlinePassed` (I3). The engine enforces the
///   same deadline itself, so either side may notice first.
/// - An engine that breaks the [`Engine`] contract fails the operation rather
///   than leaving it running: a stream that ends without a result counts as an
///   engine exit, and an event out of order as a protocol failure.
pub struct RunOperation {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
}

impl RunOperation {
    pub fn new(engine: Arc<dyn Engine>, clock: Arc<dyn Clock>) -> Self {
        Self { engine, clock }
    }

    /// Runs `job`. When `cancel` completes, the operation is cancelled.
    pub async fn run(
        &self,
        id: OperationId,
        job: Job,
        cancel: impl Future<Output = ()> + Send,
    ) -> Operation {
        let mut op = Operation::queue(id, self.clock.now());
        let budget = job.deadline_budget();
        let RunningJob {
            events,
            mut control,
        } = match self.engine.run(id, job).await {
            Ok(running) => running,
            Err(error) => {
                self.fail(
                    &mut op,
                    FailureReason::EngineUnavailable {
                        detail: error.detail,
                    },
                );
                return op;
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
                        self.fail(&mut op, reason);
                        return op;
                    };
                    if let EngineEvent::Started { index_read_at } = &event
                        && let Some(budget) = budget
                        && timer.is_terminated()
                    {
                        let d = Deadline::after_index_read(*index_read_at, budget);
                        deadline = Some(d);
                        timer = self.clock.sleep_until(d.at()).fuse();
                    }
                    self.apply(&mut op, event);
                    if op.status().is_finished() {
                        // A result stands even past the deadline: the snapshot
                        // it reports was written already. Dropping the job
                        // stops whatever the engine still runs.
                        return op;
                    }
                    // Checked on every event, so a stream that never pauses
                    // can't keep the timer from its turn.
                    if deadline.is_some_and(|d| d.has_passed(self.clock.now())) {
                        self.fail(&mut op, FailureReason::DeadlinePassed);
                        return op;
                    }
                }
                () = cancel => {
                    control.stop().await;
                    self.cancel(&mut op);
                    return op;
                }
                () = timer => {
                    // No grace past the deadline: dropping the job on return
                    // kills it at once (§4.1).
                    self.fail(&mut op, FailureReason::DeadlinePassed);
                    return op;
                }
            }
        }
    }

    fn apply(&self, op: &mut Operation, event: EngineEvent) {
        let at = self.clock.now();
        let applied = match event {
            EngineEvent::Started { .. } => op.start(at),
            EngineEvent::Progress(_) => op.report_progress(),
            EngineEvent::Warning(warning) => op.record_warning(warning),
            EngineEvent::Succeeded(_) => op.succeed(at),
            EngineEvent::Failed(reason) => op.fail(reason, at),
        };
        if let Err(error) = applied {
            let transition = error.transition;
            self.fail(op, FailureReason::ProtocolViolated { transition });
        }
    }

    // `run` returns as soon as the operation finishes, so it only ever
    // cancels or fails an unfinished one; a finished one keeps the outcome
    // it already has.
    fn cancel(&self, op: &mut Operation) {
        let _ = op.cancel(self.clock.now());
    }

    fn fail(&self, op: &mut Operation, reason: FailureReason) {
        let _ = op.fail(reason, self.clock.now());
    }
}
