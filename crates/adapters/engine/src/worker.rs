//! The worker side: read one [`JobSpec`] from stdin, run it, and report
//! [`WorkerEvent`]s on stdout, one JSON object per line.

use std::io::{self, Read, Write};
use std::time::Duration;

use api::ErrorCode;

use crate::job::{
    FakeEndSpec, FakeJobSpec, JobKind, JobSpec, MAX_MESSAGE_BYTES, MAX_MESSAGE_READ, OpResult,
    ProgressKind, WorkerEvent,
};

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

/// Reads the job spec from `input`: at most [`MAX_MESSAGE_BYTES`] of it, so a
/// peer that never stops writing can't grow the worker's memory without bound.
fn read_spec(input: impl Read) -> Result<JobSpec, String> {
    let mut raw = Vec::new();
    input
        .take(MAX_MESSAGE_READ)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > MAX_MESSAGE_BYTES {
        return Err(format!("job spec exceeds {MAX_MESSAGE_BYTES} bytes"));
    }
    serde_json::from_slice(&raw).map_err(|e| e.to_string())
}

/// Runs the job read from `input`, writing events to `output`. Fails only if
/// `output` can't be written, for example because the daemon went away.
pub fn run(input: impl Read, output: impl Write, pid: u32) -> io::Result<Exit> {
    let mut out = Events(output);
    let spec = match read_spec(input) {
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
    use std::cell::Cell;

    use super::*;
    use crate::job::MAX_MESSAGE_BYTES;

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

    fn invalid_spec_failure(output: &[u8]) {
        let events = events(output);
        let [WorkerEvent::Failed { code, detail, .. }] = events.as_slice() else {
            panic!("expected exactly one failure event, got {events:?}");
        };
        assert_eq!(*code, ErrorCode::InvalidJobSpec);
        let detail = detail.as_deref().unwrap_or_default();
        assert!(
            detail.contains(&MAX_MESSAGE_BYTES.to_string()),
            "the failure should name the cap: {detail}"
        );
    }

    fn a_valid_spec() -> Vec<u8> {
        let spec = JobSpec {
            op_id: domain::OperationId::from_uuid(domain::Uuid::from_u128(9)),
            job: JobKind::Fake(FakeJobSpec {
                steps: 0,
                step_delay_ms: 0,
                warnings: Vec::new(),
                end: FakeEndSpec::Succeed,
            }),
        };
        serde_json::to_vec(&spec).unwrap()
    }

    #[test]
    fn a_job_spec_past_the_cap_is_rejected_even_when_it_parses() {
        let mut input = a_valid_spec();
        // Trailing whitespace is still valid JSON.
        input.resize(MAX_MESSAGE_BYTES + 1, b' ');
        let mut output = Vec::new();
        let exit = run(input.as_slice(), &mut output, 42).unwrap();
        assert_eq!(exit.code(), 1);
        invalid_spec_failure(&output);
    }

    #[test]
    fn a_job_spec_of_exactly_the_cap_is_accepted() {
        let mut input = a_valid_spec();
        input.resize(MAX_MESSAGE_BYTES, b' ');
        let mut output = Vec::new();
        let exit = run(input.as_slice(), &mut output, 42).unwrap();
        assert_eq!(exit, Exit::Reported { failed: false });
    }

    /// Serves up to 64 MiB of spaces and counts how much was asked for.
    struct Endless<'a> {
        served: &'a Cell<usize>,
        left: usize,
    }

    impl Read for Endless<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.left);
            buf[..n].fill(b' ');
            self.left -= n;
            self.served.set(self.served.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn a_job_spec_that_never_ends_is_not_read_past_the_cap() {
        let served = Cell::new(0);
        let input = Endless {
            served: &served,
            left: 64 << 20,
        };
        let mut output = Vec::new();
        let exit = run(input, &mut output, 42).unwrap();
        assert_eq!(exit.code(), 1);
        invalid_spec_failure(&output);
        assert!(
            served.get() <= MAX_MESSAGE_BYTES + 1,
            "read {} bytes of a job spec capped at {MAX_MESSAGE_BYTES}",
            served.get()
        );
    }
}
