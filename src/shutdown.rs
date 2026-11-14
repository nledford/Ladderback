//! The daemon's stop (§4.2, shut-down order): each step is waited for
//! within a bound of its own and within what is left of the stop's limit,
//! so no step can keep the daemon from exiting.

use std::time::{Duration, Instant};

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
        let _ = limit;
        todo!()
    }

    /// Waits for `work`, `step` of the stop, for `bound` at most and no
    /// longer than what is left of the limit. `None` when it was given up.
    pub(crate) async fn wait<T>(
        &mut self,
        step: Step,
        bound: Duration,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let _ = (step, bound, work);
        todo!()
    }

    /// What is left of the limit.
    pub(crate) fn left(&self) -> Duration {
        todo!()
    }

    /// Records that `step` was waited for `took`, and whether it ended.
    pub(crate) fn waited(&mut self, step: Step, took: Duration, ended: bool) {
        let _ = (step, took, ended);
        todo!()
    }

    /// The line the log ends a stop with, for a stop that took `took`.
    fn said(&self, took: Duration) -> String {
        let _ = took;
        todo!()
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
