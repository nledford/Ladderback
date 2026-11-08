//! The fake job: no repository, only the worker boundary (process, event
//! protocol, crash and deadline handling).

use std::thread::sleep;
use std::time::Duration;

use super::convert::JobError;
use super::{Context, Done};
use crate::job::{FakeEndSpec, FakeJobSpec, OpResult, ProgressKind, WorkerEvent};

pub(crate) fn run(ctx: &Context, job: &FakeJobSpec) -> Result<Done, JobError> {
    ctx.start(job.deadline_budget)?;
    let events = &ctx.events;
    events.emit(&WorkerEvent::ProgressStart {
        key: 0,
        kind: ProgressKind::Counter,
        prefix: "fake".to_owned(),
    })?;
    events.emit(&WorkerEvent::ProgressLength {
        key: 0,
        len: u64::from(job.steps),
    })?;
    for warning in &job.warnings {
        events.emit(&WorkerEvent::Warning {
            item: warning.item.clone(),
            during: warning.during.clone(),
            code: warning.code,
            detail: warning.detail.clone(),
        })?;
    }
    for done in 0..job.steps {
        if let FakeEndSpec::Exit {
            after_steps,
            exit_code,
        } = &job.end
            && done == *after_steps
        {
            return Ok(Done::Abrupt(*exit_code));
        }
        sleep(Duration::from_millis(job.step_delay_ms));
        events.emit(&WorkerEvent::ProgressInc { key: 0, inc: 1 })?;
    }
    match &job.end {
        FakeEndSpec::Exit { exit_code, .. } => Ok(Done::Abrupt(*exit_code)),
        FakeEndSpec::Succeed => {
            events.emit(&WorkerEvent::ProgressFinish { key: 0 })?;
            Ok(Done::Result(OpResult::Fake {
                steps_completed: job.steps,
            }))
        }
        FakeEndSpec::Fail { detail } => {
            events.emit(&WorkerEvent::ProgressFinish { key: 0 })?;
            Err(JobError::failed(detail.clone()))
        }
    }
}
