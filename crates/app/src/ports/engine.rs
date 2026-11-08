use std::time::Duration;

use async_trait::async_trait;
use domain::OperationId;
use domain::execution::{FailureReason, Warning};
use futures::stream::BoxStream;

/// Runs operations against a repository and reports what happens as a stream
/// of [`EngineEvent`]s.
///
/// Contract (checked by `contract::engine`): an accepted job's stream starts
/// with exactly one [`EngineEvent::Started`] and ends with exactly one
/// [`EngineEvent::Succeeded`] or [`EngineEvent::Failed`], after which it
/// yields nothing. An engine that fails before it starts yields only the
/// `Failed` event. Dropping the stream stops the operation.
#[async_trait]
pub trait Engine: Send + Sync {
    async fn run(
        &self,
        op: OperationId,
        job: Job,
    ) -> Result<BoxStream<'static, EngineEvent>, EngineError>;
}

/// The engine couldn't start the operation at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub detail: String,
}

/// What an operation should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// Touches no repository. Exercises the engine boundary (worker process,
    /// event protocol, crash handling) in tests and diagnostics.
    Fake(FakeJob),
}

/// A scripted job: count `steps`, waiting `step_delay` before each, report
/// `warnings` right after starting, then end as `end` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeJob {
    pub steps: u32,
    pub step_delay: Duration,
    pub warnings: Vec<Warning>,
    pub end: FakeEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeEnd {
    /// Report a result after the last step.
    Succeed,
    /// Report a failure with this detail after the last step.
    Fail { detail: String },
    /// Stop without reporting anything once `after_steps` steps are done,
    /// as a crashed engine would. The worker exits with `exit_code`; a
    /// process exit status is one byte, so the type keeps fakes honest.
    Exit { after_steps: u32, exit_code: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    Started,
    Progress(ProgressEvent),
    Warning(Warning),
    Succeeded(EngineResult),
    Failed(FailureReason),
}

/// One progress bar's change; `key` tells concurrent bars apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressEvent {
    Begin {
        key: u32,
        kind: ProgressKind,
        prefix: String,
    },
    Length {
        key: u32,
        len: u64,
    },
    Advance {
        key: u32,
        by: u64,
    },
    Finish {
        key: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    Spinner,
    Counter,
    Bytes,
}

/// What a successful operation produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineResult {
    Fake { steps_completed: u32 },
}
