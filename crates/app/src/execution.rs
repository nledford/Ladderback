//! Execution use cases.

use std::sync::Arc;

use domain::OperationId;
use domain::execution::{FailureReason, Operation};
use futures::StreamExt;

use crate::ports::{Clock, Engine, EngineEvent, Job};

/// Runs one operation on the engine and records its lifecycle: queued now,
/// running once the engine starts, then finished as the engine reports.
///
/// An engine that breaks the [`Engine`] contract fails the operation rather
/// than leaving it running: a stream that ends without a result counts as an
/// engine exit, and an event out of order as a protocol failure.
pub struct RunOperation {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
}

impl RunOperation {
    pub fn new(engine: Arc<dyn Engine>, clock: Arc<dyn Clock>) -> Self {
        Self { engine, clock }
    }

    pub async fn run(&self, id: OperationId, job: Job) -> Operation {
        let mut op = Operation::queue(id, self.clock.now());
        let mut events = match self.engine.run(id, job).await {
            Ok(events) => events,
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
        while let Some(event) = events.next().await {
            let at = self.clock.now();
            let applied = match event {
                EngineEvent::Started => op.start(at),
                EngineEvent::Progress(_) => op.report_progress(),
                EngineEvent::Warning(warning) => op.record_warning(warning),
                EngineEvent::Succeeded(_) => op.succeed(at),
                EngineEvent::Failed(reason) => op.fail(reason, at),
            };
            if let Err(error) = applied {
                let transition = error.transition;
                self.fail(&mut op, FailureReason::ProtocolViolated { transition });
            }
            if op.status().is_finished() {
                // Dropping the stream stops whatever the engine still runs.
                return op;
            }
        }
        self.fail(&mut op, FailureReason::EngineExited { exit_code: None });
        op
    }

    fn fail(&self, op: &mut Operation, reason: FailureReason) {
        // Only an unfinished operation can be failed. Every caller fails one
        // that just broke the contract and so is unfinished; a finished one
        // keeps the outcome it already has.
        let _ = op.fail(reason, self.clock.now());
    }
}
