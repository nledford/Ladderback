//! The worker side: read one [`JobSpec`] from stdin, run it, and report
//! [`WorkerEvent`]s on stdout, one JSON object per line.

use std::io::{self, Read, Write};
use std::time::Duration;

use api::ErrorCode;

use crate::job::{FakeEndSpec, FakeJobSpec, JobKind, JobSpec, OpResult, ProgressKind, WorkerEvent};

/// How the worker process should exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The job ran and its result or failure was reported.
    Reported { failed: bool },
    /// Stop at once with this code, reporting nothing more: a fake job
    /// imitating a crashed engine.
    Abrupt(u8),
}

impl Exit {
    pub fn code(self) -> i32 {
        match self {
            Self::Reported { failed: false } => 0,
            Self::Reported { failed: true } => 1,
            Self::Abrupt(code) => i32::from(code),
        }
    }
}

/// Runs the job read from `input`, writing events to `output`. Fails only if
/// `output` can't be written, for example because the daemon went away.
pub fn run(mut input: impl Read, output: impl Write, pid: u32) -> io::Result<Exit> {
    let mut out = Events(output);
    let mut raw = String::new();
    let spec = match input
        .read_to_string(&mut raw)
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_str::<JobSpec>(&raw).map_err(|e| e.to_string()))
    {
        Ok(spec) => spec,
        Err(detail) => {
            out.emit(&WorkerEvent::Failed {
                code: ErrorCode::InvalidJobSpec,
                args: Vec::new(),
                detail: Some(detail),
            })?;
            return Ok(Exit::Reported { failed: true });
        }
    };
    out.emit(&WorkerEvent::Started {
        op_id: spec.op_id,
        pid,
    })?;
    match spec.job {
        JobKind::Fake(job) => run_fake(&job, &mut out),
    }
}

struct Events<W>(W);

impl<W: Write> Events<W> {
    fn emit(&mut self, event: &WorkerEvent) -> io::Result<()> {
        serde_json::to_writer(&mut self.0, event)?;
        self.0.write_all(b"\n")?;
        // The daemon reads events live; never leave one in a buffer.
        self.0.flush()
    }
}

fn run_fake(job: &FakeJobSpec, out: &mut Events<impl Write>) -> io::Result<Exit> {
    let key = 0;
    out.emit(&WorkerEvent::ProgressStart {
        key,
        kind: ProgressKind::Counter,
        prefix: "fake".to_owned(),
    })?;
    out.emit(&WorkerEvent::ProgressLength {
        key,
        len: u64::from(job.steps),
    })?;
    for warning in &job.warnings {
        out.emit(&WorkerEvent::Warning {
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
        } = job.end
            && after_steps == done
        {
            return Ok(Exit::Abrupt(exit_code));
        }
        std::thread::sleep(Duration::from_millis(job.step_delay_ms));
        out.emit(&WorkerEvent::ProgressInc { key, inc: 1 })?;
    }
    let end = match &job.end {
        FakeEndSpec::Exit { exit_code, .. } => return Ok(Exit::Abrupt(*exit_code)),
        FakeEndSpec::Succeed => WorkerEvent::Result(OpResult::Fake {
            steps_completed: job.steps,
        }),
        FakeEndSpec::Fail { detail } => WorkerEvent::Failed {
            code: ErrorCode::OperationFailed,
            args: Vec::new(),
            detail: Some(detail.clone()),
        },
    };
    out.emit(&WorkerEvent::ProgressFinish { key })?;
    let failed = matches!(end, WorkerEvent::Failed { .. });
    out.emit(&end)?;
    Ok(Exit::Reported { failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(output: &[u8]) -> Vec<WorkerEvent> {
        std::str::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn an_unreadable_job_spec_is_reported_as_a_failure() {
        let mut output = Vec::new();
        let exit = run(&b"{not json"[..], &mut output, 42).unwrap();
        assert_eq!(exit.code(), 1);
        let events = events(&output);
        let [WorkerEvent::Failed { code, detail, .. }] = events.as_slice() else {
            panic!("expected exactly one failure event");
        };
        assert_eq!(*code, ErrorCode::InvalidJobSpec);
        assert!(detail.is_some());
    }

    #[test]
    fn every_event_is_one_line_of_json() {
        let spec = JobSpec {
            op_id: domain::OperationId::from_uuid(domain::Uuid::from_u128(9)),
            job: JobKind::Fake(FakeJobSpec {
                steps: 2,
                step_delay_ms: 0,
                warnings: Vec::new(),
                end: FakeEndSpec::Succeed,
            }),
        };
        let mut output = Vec::new();
        let input = serde_json::to_vec(&spec).unwrap();
        let exit = run(input.as_slice(), &mut output, 42).unwrap();
        assert_eq!(exit, Exit::Reported { failed: false });
        let events = events(&output);
        assert_eq!(
            events.first(),
            Some(&WorkerEvent::Started {
                op_id: spec.op_id,
                pid: 42
            })
        );
        assert_eq!(
            events.last(),
            Some(&WorkerEvent::Result(OpResult::Fake { steps_completed: 2 }))
        );
    }
}
