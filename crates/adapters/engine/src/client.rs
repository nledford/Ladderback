//! The daemon side: spawn a worker per operation and translate its events.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use api::{ErrorCode, WarningCode};
use app::ports::{
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, ProgressEvent,
    ProgressKind as AppProgressKind,
};
use async_trait::async_trait;
use domain::OperationId;
use domain::execution::{FailureReason, Warning, WarningKind};
use futures::stream::{self, BoxStream, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};

use crate::job::{
    FakeEndSpec, FakeJobSpec, FakeWarning, JobKind, JobSpec, OpResult, ProgressKind, WorkerEvent,
};

/// Runs each operation in a new worker process: `program args…`, normally
/// `steading worker`. Dropping an operation's event stream before its result
/// kills the worker; after the result, the worker gets a grace period to exit.
pub struct WorkerEngine {
    program: PathBuf,
    args: Vec<OsString>,
}

impl WorkerEngine {
    pub fn new(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[async_trait]
impl Engine for WorkerEngine {
    async fn run(
        &self,
        op: OperationId,
        job: Job,
    ) -> Result<BoxStream<'static, EngineEvent>, EngineError> {
        let spec = serde_json::to_vec(&JobSpec {
            op_id: op,
            job: job_kind(job)?,
        })
        .map_err(|e| failed_to_start(&self.program, e))?;
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| failed_to_start(&self.program, e))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin
            .write_all(&spec)
            .await
            .map_err(|e| failed_to_start(&self.program, e))?;
        // Closing stdin tells the worker the job spec is complete.
        drop(stdin);
        let stdout = child.stdout.take().expect("stdout is piped");
        let reading = Reading {
            child: Some(child),
            lines: BufReader::new(stdout).lines(),
            done: false,
        };
        Ok(stream::unfold(reading, Reading::next).boxed())
    }
}

fn failed_to_start(program: &std::path::Path, error: impl std::fmt::Display) -> EngineError {
    EngineError {
        detail: format!("{}: {error}", program.display()),
    }
}

/// How long a worker may keep running after it reported its result, for
/// cleanup, before it is killed.
const AFTER_RESULT_GRACE: Duration = Duration::from_secs(10);

struct Reading {
    /// `None` once the worker has been handed to a reaper.
    child: Option<Child>,
    lines: Lines<BufReader<ChildStdout>>,
    done: bool,
}

impl Reading {
    async fn next(mut self) -> Option<(EngineEvent, Self)> {
        if self.done {
            return None;
        }
        loop {
            let line = match self.lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => return Some((self.exited().await, self)),
                Err(error) => {
                    return Some((self.broken(format!("reading worker output: {error}")), self));
                }
            };
            let event = match serde_json::from_str::<WorkerEvent>(&line) {
                Ok(event) => event,
                Err(error) => {
                    return Some((
                        self.broken(format!("malformed worker event: {error}")),
                        self,
                    ));
                }
            };
            if let Some(event) = translate(event) {
                if matches!(event, EngineEvent::Succeeded(_) | EngineEvent::Failed(_)) {
                    self.done = true;
                    self.reap();
                }
                return Some((event, self));
            }
        }
    }

    /// The worker reported its result. Dropping the stream must not kill it
    /// while it cleans up, so a task waits for it, killing it only after
    /// [`AFTER_RESULT_GRACE`].
    fn reap(&mut self) {
        if let Some(mut child) = self.child.take() {
            tokio::spawn(async move {
                if tokio::time::timeout(AFTER_RESULT_GRACE, child.wait())
                    .await
                    .is_err()
                {
                    let _ = child.kill().await;
                }
            });
        }
    }

    /// The worker closed stdout without a result: it crashed or was killed.
    async fn exited(&mut self) -> EngineEvent {
        self.done = true;
        let exit_code = match self.child.as_mut() {
            Some(child) => child.wait().await.ok().and_then(|status| status.code()),
            None => None,
        };
        EngineEvent::Failed(FailureReason::EngineExited { exit_code })
    }

    fn broken(&mut self, detail: String) -> EngineEvent {
        // Dropping the stream kills the worker (`kill_on_drop`).
        self.done = true;
        EngineEvent::Failed(FailureReason::EngineFailed { detail })
    }
}

fn translate(event: WorkerEvent) -> Option<EngineEvent> {
    Some(match event {
        WorkerEvent::Started { .. } => EngineEvent::Started,
        WorkerEvent::ProgressStart { key, kind, prefix } => {
            EngineEvent::Progress(ProgressEvent::Begin {
                key,
                kind: match kind {
                    ProgressKind::Spinner => AppProgressKind::Spinner,
                    ProgressKind::Counter => AppProgressKind::Counter,
                    ProgressKind::Bytes => AppProgressKind::Bytes,
                },
                prefix,
            })
        }
        WorkerEvent::ProgressLength { key, len } => {
            EngineEvent::Progress(ProgressEvent::Length { key, len })
        }
        WorkerEvent::ProgressInc { key, inc } => {
            EngineEvent::Progress(ProgressEvent::Advance { key, by: inc })
        }
        WorkerEvent::ProgressFinish { key } => EngineEvent::Progress(ProgressEvent::Finish { key }),
        WorkerEvent::Warning {
            item,
            during,
            code,
            detail,
        } => EngineEvent::Warning(Warning {
            kind: match code {
                WarningCode::SourceUnreadable => WarningKind::SourceUnreadable,
                WarningCode::Unknown => WarningKind::Other,
            },
            item,
            during,
            detail,
        }),
        // Logs are for the daemon's own log, which arrives with M1; until
        // then they go to its stderr.
        WorkerEvent::Log { level, message } => {
            eprintln!("worker {level:?}: {message}");
            return None;
        }
        WorkerEvent::Result(OpResult::Fake { steps_completed }) => {
            EngineEvent::Succeeded(EngineResult::Fake { steps_completed })
        }
        WorkerEvent::Failed { code, detail, .. } => {
            let detail = detail.unwrap_or_default();
            EngineEvent::Failed(match code {
                ErrorCode::InvalidJobSpec => FailureReason::JobRejected { detail },
                ErrorCode::OperationFailed | ErrorCode::Unknown => {
                    FailureReason::EngineFailed { detail }
                }
            })
        }
    })
}

fn job_kind(job: Job) -> Result<JobKind, EngineError> {
    let Job::Fake(FakeJob {
        steps,
        step_delay,
        warnings,
        end,
    }) = job;
    Ok(JobKind::Fake(FakeJobSpec {
        steps,
        step_delay_ms: u64::try_from(step_delay.as_millis()).map_err(|_| EngineError {
            detail: "step delay out of range".to_owned(),
        })?,
        warnings: warnings
            .into_iter()
            .map(|w| FakeWarning {
                code: match w.kind {
                    WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
                    WarningKind::Other => WarningCode::Unknown,
                },
                item: w.item,
                during: w.during,
                detail: w.detail,
            })
            .collect(),
        end: match end {
            FakeEnd::Succeed => FakeEndSpec::Succeed,
            FakeEnd::Fail { detail } => FakeEndSpec::Fail { detail },
            FakeEnd::Exit {
                after_steps,
                exit_code,
            } => FakeEndSpec::Exit {
                after_steps,
                exit_code,
            },
        },
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use domain::Uuid;

    use super::*;

    fn failed(code: ErrorCode) -> WorkerEvent {
        WorkerEvent::Failed {
            code,
            args: Vec::new(),
            detail: Some("bad spec".to_owned()),
        }
    }

    #[test]
    fn a_worker_that_rejects_its_job_spec_rejects_the_job() {
        assert_eq!(
            translate(failed(ErrorCode::InvalidJobSpec)),
            Some(EngineEvent::Failed(FailureReason::JobRejected {
                detail: "bad spec".to_owned()
            }))
        );
    }

    #[test]
    fn other_worker_failures_carry_the_engine_message_verbatim() {
        for code in [ErrorCode::OperationFailed, ErrorCode::Unknown] {
            assert_eq!(
                translate(failed(code)),
                Some(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "bad spec".to_owned()
                }))
            );
        }
    }

    /// A worker may still be cleaning up after it reports its result; the
    /// daemon dropping the event stream must not kill it mid-way.
    #[tokio::test]
    async fn a_worker_may_finish_after_reporting_its_result() {
        let scratch = std::env::temp_dir().join(format!("steading-reap-{}", std::process::id()));
        let _ = std::fs::remove_file(&scratch);
        let result =
            serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 }))
                .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{result}'; sleep 0.3; touch '{}'",
            scratch.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = Job::Fake(FakeJob {
            steps: 0,
            step_delay: Duration::ZERO,
            warnings: Vec::new(),
            end: FakeEnd::Succeed,
        });
        let mut events = engine
            .run(OperationId::from_uuid(Uuid::from_u128(1)), job)
            .await
            .unwrap();
        assert!(matches!(
            events.next().await,
            Some(EngineEvent::Succeeded(_))
        ));
        drop(events);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(scratch.exists(), "the worker was killed after its result");
        let _ = std::fs::remove_file(&scratch);
    }
}
