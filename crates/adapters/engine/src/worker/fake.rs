//! The fake job: no repository, only the worker boundary (process, event
//! protocol, crash and deadline handling).

use std::thread::sleep;
use std::time::Duration;

use super::convert::JobError;
use super::{Context, Done};
use crate::job::{FakeEndSpec, FakeJobSpec, OpResult, ProgressKind, WorkerEvent};

pub(crate) fn run(ctx: &Context, job: &FakeJobSpec) -> Result<Done, JobError> {
    todo!()
}
