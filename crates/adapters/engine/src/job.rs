//! The worker protocol. Only the daemon and the worker speak it, and both
//! link this crate, so it isn't part of the published `api`.

use api::{ErrorCode, MsgArg, WarningCode};
use domain::OperationId;
use serde::{Deserialize, Serialize};

/// The longest message either side accepts: the job spec on the worker's
/// stdin, and each event line on its stdout. Bytes from another process
/// never size an allocation unbounded (technical design §13.5), and a
/// message past the cap fails the job.
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;

/// The one job a worker runs, read from its stdin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    pub op_id: OperationId,
    pub job: JobKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobKind {
    Fake(FakeJobSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeJobSpec {
    pub steps: u32,
    pub step_delay_ms: u64,
    pub warnings: Vec<FakeWarning>,
    pub end: FakeEndSpec,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeWarning {
    pub code: WarningCode,
    pub item: Option<String>,
    pub during: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "end", rename_all = "snake_case")]
pub enum FakeEndSpec {
    Succeed,
    Fail { detail: String },
    Exit { after_steps: u32, exit_code: u8 },
}

/// One line of the worker's stdout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    Started {
        op_id: OperationId,
        pid: u32,
    },
    ProgressStart {
        key: u32,
        kind: ProgressKind,
        prefix: String,
    },
    ProgressLength {
        key: u32,
        len: u64,
    },
    ProgressInc {
        key: u32,
        inc: u64,
    },
    ProgressFinish {
        key: u32,
    },
    Warning {
        item: Option<String>,
        during: String,
        code: WarningCode,
        detail: String,
    },
    /// Never localized (§8.4).
    Log {
        level: Level,
        message: String,
    },
    Result(OpResult),
    Failed {
        code: ErrorCode,
        args: Vec<(String, MsgArg)>,
        detail: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressKind {
    Spinner,
    Counter,
    Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum OpResult {
    Fake { steps_completed: u32 },
}
