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
        todo!()
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
