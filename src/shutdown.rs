//! The daemon's stop (§4.2, shut-down order): each step is waited for
//! within a bound of its own and within what is left of the stop's limit,
//! so no step can keep the daemon from exiting.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use crate::composition::LOG_CLOSE_BOUND;

/// How long the requests still open are waited for. A request is answered
/// in far less; one still open after this is one a client never finishes.
pub(crate) const REQUESTS_BOUND: Duration = Duration::from_secs(5);

/// How long the workers are waited for: the grace a stopped worker gets
/// before SIGKILL, and some to reap it.
pub(crate) const WORKERS_BOUND: Duration = Duration::from_secs(engine::STOP_GRACE.as_secs() + 5);

/// The stop's limit: the sum of the steps' bounds, so each step gets all of
/// its own whatever the others took. The tasks get what is left.
pub(crate) const LIMIT: Duration = Duration::from_secs(
    REQUESTS_BOUND.as_secs() + LOG_CLOSE_BOUND.as_secs() + WORKERS_BOUND.as_secs(),
);

// The service manager waits longer before it kills the daemon, so it never
// cuts a stop short.
const _: () = assert!(LIMIT.as_secs() < platform::service::KILL_AFTER.as_secs());

/// One step of the stop, as the log names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The requests still open when the daemon was told to stop.
    OpenRequests,
    /// The operation log's calls in flight, and its close.
    OperationLog,
    /// The workers, stopped as a cancel stops one.
    Workers,
    /// What the daemon's tasks were still doing.
    Tasks,
}

impl Step {
    /// The step's name in the log.
    fn name(self) -> &'static str {
        match self {
            Self::OpenRequests => "open requests",
            Self::OperationLog => "operation log",
            Self::Workers => "workers",
            Self::Tasks => "tasks",
        }
    }

    /// What giving the step up leaves.
    fn given_up(self) -> &'static str {
        match self {
            Self::OpenRequests => "the requests still open end with the daemon",
            Self::OperationLog => "an operation it was still recording is ended at the next start",
            Self::Workers => "a worker still running is killed with the daemon",
            Self::Tasks => "what they were still doing is left behind with the daemon",
        }
    }
}

/// A stop under way: when it began, its limit, and how long each step was
/// waited for.
pub(crate) struct Stopping {
    began: Instant,
    limit: Duration,
    waited: Vec<Waited>,
}

/// How long one step was waited for, and whether it ended in that time.
struct Waited {
    step: Step,
    took: Duration,
    ended: bool,
}

impl Stopping {
    /// A stop that begins now and has `limit` in all.
    pub(crate) fn new(limit: Duration) -> Self {
        Self {
            began: Instant::now(),
            limit,
            waited: Vec::with_capacity(4),
        }
    }

    /// Waits for `work`, `step` of the stop, for `bound` at most and no
    /// longer than what is left of the limit. `None` when it was given up,
    /// which the log says.
    pub(crate) async fn wait<T>(
        &mut self,
        step: Step,
        bound: Duration,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let began = Instant::now();
        let ended = tokio::time::timeout(bound.min(self.left()), work)
            .await
            .ok();
        self.waited(step, began.elapsed(), ended.is_some());
        ended
    }

    /// What is left of the limit.
    pub(crate) fn left(&self) -> Duration {
        self.limit.saturating_sub(self.began.elapsed())
    }

    /// Records that `step` was waited for `took`, and whether it ended; the
    /// log says when it didn't.
    pub(crate) fn waited(&mut self, step: Step, took: Duration, ended: bool) {
        if !ended {
            log::warn!(
                "stopping: gave up waiting for the {} after {:.1} s: {}",
                step.name(),
                took.as_secs_f64(),
                step.given_up()
            );
        }
        self.waited.push(Waited { step, took, ended });
    }

    /// Logs the line a stop ends with: how long it took, and each step. At
    /// WARN when a step was given up.
    pub(crate) fn finish(&self) {
        let said = self.said(self.began.elapsed());
        if self.waited.iter().all(|waited| waited.ended) {
            log::info!("{said}");
        } else {
            log::warn!("{said}");
        }
    }

    /// The line the log ends a stop with, for a stop that took `took`.
    fn said(&self, took: Duration) -> String {
        let mut said = format!("stopped after {:.1} s:", took.as_secs_f64());
        for (at, waited) in self.waited.iter().enumerate() {
            let _ = write!(
                said,
                "{} {} {:.1} s{}",
                if at == 0 { "" } else { "," },
                waited.step.name(),
                waited.took.as_secs_f64(),
                if waited.ended { "" } else { " (gave up)" }
            );
        }
        said
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generous next to the bounds below: only a stop that waited the step
    /// out would take it.
    const WAITED_OUT: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn a_step_that_never_ends_is_given_up_at_its_bound() {
        let mut stopping = Stopping::new(Duration::from_secs(60));
        let began = Instant::now();
        let ended = stopping
            .wait(
                Step::OpenRequests,
                Duration::from_millis(50),
                std::future::pending::<()>(),
            )
            .await;
        assert_eq!(ended, None);
        assert!(began.elapsed() < WAITED_OUT, "the step was waited out");
        let said = stopping.said(Duration::from_millis(50));
        assert!(
            said.split(", ")
                .any(|step| step.contains("open requests") && step.ends_with("(gave up)")),
            "{said}"
        );
    }

    #[tokio::test]
    async fn a_step_gets_no_more_than_what_is_left_of_the_limit() {
        let mut stopping = Stopping::new(Duration::from_millis(50));
        let began = Instant::now();
        let ended = stopping
            .wait(Step::Workers, WAITED_OUT * 2, std::future::pending::<()>())
            .await;
        assert_eq!(ended, None);
        assert!(began.elapsed() < WAITED_OUT, "the step outlived the limit");
        assert_eq!(stopping.left(), Duration::ZERO);
    }

    #[tokio::test]
    async fn a_step_that_ends_in_time_is_waited_for_and_gives_its_result() {
        let mut stopping = Stopping::new(Duration::from_secs(60));
        let ended = stopping
            .wait(Step::OperationLog, Duration::from_secs(20), async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                7
            })
            .await;
        assert_eq!(ended, Some(7));
        assert!(stopping.left() <= Duration::from_secs(60) - Duration::from_millis(20));
        let said = stopping.said(Duration::from_millis(20));
        assert!(said.contains(": operation log "), "{said}");
        assert!(!said.contains("gave up"), "{said}");
    }

    #[test]
    fn the_last_line_says_how_long_the_stop_and_each_step_took() {
        let mut stopping = Stopping::new(Duration::from_secs(65));
        stopping.waited(Step::OpenRequests, Duration::from_millis(5_000), false);
        stopping.waited(Step::OperationLog, Duration::from_millis(120), true);
        stopping.waited(Step::Workers, Duration::from_millis(2_340), true);
        stopping.waited(Step::Tasks, Duration::ZERO, true);
        assert_eq!(
            stopping.said(Duration::from_millis(7_470)),
            "stopped after 7.5 s: open requests 5.0 s (gave up), operation log 0.1 s, workers 2.3 s, tasks 0.0 s"
        );
    }
}
