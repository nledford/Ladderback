use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::deadline::Deadline;
use domain::{OperationId, SignedDuration};
use futures::stream::{self, StreamExt};

use crate::ports::{
    Clock, Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, Job, JobControl,
    ProgressEvent, ProgressKind, RunningJob,
};

/// An in-process engine that plays [`FakeJob`]s exactly as the worker does,
/// waiting on the given clock between steps. Like the worker's watchdog, it
/// stops without a result once a fake job's deadline has passed.
pub struct SimulatedEngine {
    clock: Arc<dyn Clock>,
}

impl SimulatedEngine {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }
}

enum Step {
    Emit(EngineEvent),
    Wait(SignedDuration),
}

struct Playing {
    steps: VecDeque<Step>,
    clock: Arc<dyn Clock>,
    deadline: Option<Deadline>,
    stopped: Arc<AtomicBool>,
}

struct Stop(Arc<AtomicBool>);

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl Engine for SimulatedEngine {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let Job::Fake(job) = job else {
            return Err(EngineError::Unavailable {
                detail: "the simulated engine runs fake jobs only".to_owned(),
            });
        };
        let now = self.clock.now();
        let stopped = Arc::new(AtomicBool::new(false));
        let playing = Playing {
            steps: script(&job)?,
            clock: Arc::clone(&self.clock),
            deadline: job
                .deadline_budget
                .map(|budget| Deadline::after_index_read(now, budget)),
            stopped: Arc::clone(&stopped),
        };
        let started = EngineEvent::Started {
            index_read_at: now,
            repository: None,
        };
        let events = stream::iter([started])
            .chain(stream::unfold(playing, |mut playing| async move {
                loop {
                    if playing.stopped.load(Ordering::SeqCst) {
                        return None;
                    }
                    if playing
                        .deadline
                        .is_some_and(|d| d.has_passed(playing.clock.now()))
                    {
                        // Like the worker's watchdog: nothing more, then the failure.
                        playing.steps.clear();
                        playing.stopped.store(true, Ordering::SeqCst);
                        let failed = EngineEvent::Failed(FailureReason::DeadlinePassed);
                        return Some((failed, playing));
                    }
                    match playing.steps.pop_front()? {
                        Step::Wait(delay) => {
                            let until = playing.clock.now() + delay;
                            playing.clock.sleep_until(until).await;
                        }
                        Step::Emit(event) => return Some((event, playing)),
                    }
                }
            }))
            .boxed();
        Ok(RunningJob {
            events,
            control: Box::new(Stop(stopped)),
        })
    }
}

fn script(job: &FakeJob) -> Result<VecDeque<Step>, EngineError> {
    let delay = SignedDuration::try_from(job.step_delay).map_err(|e| EngineError::Unavailable {
        detail: format!("step delay out of range: {e}"),
    })?;
    let key = 0;
    let mut steps = VecDeque::from([
        Step::Emit(EngineEvent::Progress(ProgressEvent::Begin {
            key,
            kind: ProgressKind::Counter,
            prefix: "fake".to_owned(),
        })),
        Step::Emit(EngineEvent::Progress(ProgressEvent::Length {
            key,
            len: u64::from(job.steps),
        })),
    ]);
    steps.extend(
        job.warnings
            .iter()
            .cloned()
            .map(|w| Step::Emit(EngineEvent::Warning(w))),
    );
    let exit = |exit_code: u8| {
        Step::Emit(EngineEvent::Failed(FailureReason::EngineExited {
            exit_code: Some(i32::from(exit_code)),
        }))
    };
    for done in 0..job.steps {
        if let FakeEnd::Exit {
            after_steps,
            exit_code,
        } = job.end
            && after_steps == done
        {
            steps.push_back(exit(exit_code));
            return Ok(steps);
        }
        steps.push_back(Step::Wait(delay));
        steps.push_back(Step::Emit(EngineEvent::Progress(ProgressEvent::Advance {
            key,
            by: 1,
        })));
    }
    let end = match &job.end {
        FakeEnd::Exit { exit_code, .. } => {
            steps.push_back(exit(*exit_code));
            return Ok(steps);
        }
        FakeEnd::Fail { detail } => EngineEvent::Failed(FailureReason::EngineFailed {
            detail: detail.clone(),
        }),
        FakeEnd::Succeed => EngineEvent::Succeeded(EngineResult::Fake {
            steps_completed: job.steps,
        }),
    };
    steps.push_back(Step::Emit(EngineEvent::Progress(ProgressEvent::Finish {
        key,
    })));
    steps.push_back(Step::Emit(end));
    Ok(steps)
}
