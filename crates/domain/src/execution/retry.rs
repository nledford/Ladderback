//! Retries of a failed backup (technical design §5): when a plan retries a
//! failed backup, how long it waits, and when its retries stop. Nothing here
//! runs anything or waits: the scheduler in `app` starts each retry as this
//! decides, in a place of the persistent queue.

use crate::SignedDuration;
use crate::Timestamp;
use crate::execution::Trigger;
use crate::planning::RetryPolicy;

/// A retry a plan has planned after a failed backup: which of the policy's
/// attempts it is, when it runs, and the scheduled run its series stops at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryScheduled {
    /// 1 for the first retry after a failed run, 2 for the one after that
    /// retry failed too, and so on, to at most the policy's `attempts`.
    pub attempt: u32,
    /// When the retry runs.
    pub at: Timestamp,
    /// The plan's next scheduled run when the run that began the series
    /// failed: no retry of the series runs at or after it. `None` for a plan
    /// with no schedule, whose retries only the attempts end.
    pub until: Option<Timestamp>,
}

/// Why a failed backup isn't retried, or a retry planned doesn't run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoRetry {
    /// The policy's attempts are spent.
    Exhausted,
    /// The plan's next scheduled run comes first, or has come: retries stop
    /// at it (§5).
    NextRun,
    /// A run of the plan is going, running or waiting for its turn: it
    /// takes the failed run's place, whatever the plan's overlap policy.
    RunGoing,
}

/// The domain service that plans retries (§5): the wait doubles from the
/// policy's `backoff` after each failed retry, to at most an hour, and the
/// series stops when its attempts are spent or at the plan's next scheduled
/// run.
pub struct RetrySchedule;

impl RetrySchedule {
    /// The longest wait before a retry, however many have failed.
    pub const MAX_WAIT: SignedDuration = RetryPolicy::MAX_BACKOFF;

    /// How long retry `attempt` waits after the run before it failed:
    /// `backoff` × 2^(`attempt` − 1), at most [`Self::MAX_WAIT`]. Attempt 0
    /// waits as attempt 1 does.
    pub fn wait(policy: &RetryPolicy, attempt: u32) -> SignedDuration {
        2_i32
            .checked_pow(attempt.saturating_sub(1))
            .and_then(|factor| policy.backoff().checked_mul(factor))
            .map_or(Self::MAX_WAIT, |wait| wait.min(Self::MAX_WAIT))
    }

    /// The first retry after a backup that isn't a retry failed at
    /// `failed_at`, its series stopping at `next_run`, the plan's next
    /// scheduled run then.
    pub fn first(
        policy: &RetryPolicy,
        failed_at: Timestamp,
        next_run: Option<Timestamp>,
    ) -> Result<RetryScheduled, NoRetry> {
        Self::planned(policy, 1, failed_at, next_run)
    }

    /// The retry after `failed`, a retry of the series that failed at
    /// `failed_at`.
    pub fn after(
        policy: &RetryPolicy,
        failed: &RetryScheduled,
        failed_at: Timestamp,
    ) -> Result<RetryScheduled, NoRetry> {
        let attempt = failed
            .attempt
            .checked_add(1)
            .filter(|attempt| *attempt <= policy.attempts())
            .ok_or(NoRetry::Exhausted)?;
        Self::planned(policy, attempt, failed_at, failed.until)
    }

    /// Whether `retry`, due at `now`, is past the scheduled run its series
    /// stops at: that run has come, whether it ran, caught up, or was
    /// skipped while the agent was away.
    pub fn superseded(retry: &RetryScheduled, now: Timestamp) -> bool {
        retry.until.is_some_and(|until| until <= now)
    }

    /// Whether a failed run that `trigger` started is retried: one the
    /// schedule started, on time or to catch up, or a retry of one. A run a
    /// person or another program asked for is theirs to retry: they see it
    /// fail and decide (decision 76).
    pub fn retried(trigger: Trigger) -> bool {
        todo!("whether a failed run that {trigger:?} started is retried")
    }

    /// Retry `attempt`, waiting after a failure at `failed_at`, unless it
    /// would run at or after `until` or past the last instant there is.
    fn planned(
        policy: &RetryPolicy,
        attempt: u32,
        failed_at: Timestamp,
        until: Option<Timestamp>,
    ) -> Result<RetryScheduled, NoRetry> {
        let at = failed_at
            .checked_add(Self::wait(policy, attempt))
            .map_err(|_| NoRetry::NextRun)?;
        if until.is_some_and(|until| at >= until) {
            return Err(NoRetry::NextRun);
        }
        Ok(RetryScheduled { attempt, at, until })
    }
}

#[cfg(test)]
mod tests {
    use super::{NoRetry, RetrySchedule, RetryScheduled};
    use crate::execution::Trigger;
    use crate::planning::RetryPolicy;
    use crate::{SignedDuration, Timestamp};

    fn policy(attempts: i64, backoff: SignedDuration) -> RetryPolicy {
        RetryPolicy::new(attempts, backoff).unwrap()
    }

    fn at(minutes: i64) -> Timestamp {
        Timestamp::constant(1_800_000_000 + minutes * 60, 0)
    }

    #[test]
    fn only_a_run_the_schedule_started_is_retried() {
        for trigger in [Trigger::Schedule, Trigger::CatchUp, Trigger::Retry] {
            assert!(RetrySchedule::retried(trigger), "{trigger:?}");
        }
        for trigger in [Trigger::Manual, Trigger::Hub, Trigger::Mcp] {
            assert!(!RetrySchedule::retried(trigger), "{trigger:?}");
        }
    }

    #[test]
    fn the_wait_doubles_from_the_back_off_to_at_most_an_hour() {
        let retry = policy(100, SignedDuration::from_mins(7));
        let waits: Vec<i64> = (1..=5)
            .map(|attempt| RetrySchedule::wait(&retry, attempt).as_mins())
            .collect();
        assert_eq!(waits, vec![7, 14, 28, 56, 60]);
        assert_eq!(RetrySchedule::MAX_WAIT, SignedDuration::from_hours(1));
    }

    #[test]
    fn the_wait_never_overflows_however_many_retries_failed() {
        for backoff in [RetryPolicy::MIN_BACKOFF, RetryPolicy::MAX_BACKOFF] {
            let retry = policy(100, backoff);
            for attempt in [13, 31, 32, 33, 100, u32::MAX] {
                assert_eq!(
                    RetrySchedule::wait(&retry, attempt),
                    RetrySchedule::MAX_WAIT,
                    "{backoff:?} {attempt}"
                );
            }
        }
        // 2^11 s is just under the hour, and 2^12 s past it.
        let retry = policy(100, RetryPolicy::MIN_BACKOFF);
        assert_eq!(
            RetrySchedule::wait(&retry, 12),
            SignedDuration::from_secs(2_048)
        );
        assert_eq!(RetrySchedule::wait(&retry, 13), RetrySchedule::MAX_WAIT);
    }

    #[test]
    fn attempt_zero_waits_as_the_first() {
        let retry = policy(3, SignedDuration::from_mins(10));
        assert_eq!(
            RetrySchedule::wait(&retry, 0),
            SignedDuration::from_mins(10)
        );
    }

    #[test]
    fn the_first_retry_waits_the_back_off_and_keeps_the_next_run() {
        let retry = policy(3, SignedDuration::from_mins(10));
        assert_eq!(
            RetrySchedule::first(&retry, at(0), Some(at(60))),
            Ok(RetryScheduled {
                attempt: 1,
                at: at(10),
                until: Some(at(60)),
            })
        );
        assert_eq!(
            RetrySchedule::first(&retry, at(0), None),
            Ok(RetryScheduled {
                attempt: 1,
                at: at(10),
                until: None,
            })
        );
    }

    #[test]
    fn a_first_retry_that_would_reach_the_next_run_is_none() {
        let retry = policy(3, SignedDuration::from_mins(10));
        assert_eq!(
            RetrySchedule::first(&retry, at(0), Some(at(10))),
            Err(NoRetry::NextRun)
        );
        assert_eq!(
            RetrySchedule::first(&retry, at(0), Some(at(5))),
            Err(NoRetry::NextRun)
        );
        assert!(RetrySchedule::first(&retry, at(0), Some(at(11))).is_ok());
    }

    #[test]
    fn each_retry_after_a_failed_one_doubles_the_wait_and_keeps_the_series_end() {
        let retry = policy(3, SignedDuration::from_mins(10));
        let first = RetryScheduled {
            attempt: 1,
            at: at(10),
            until: Some(at(600)),
        };
        let second = RetrySchedule::after(&retry, &first, at(12)).unwrap();
        assert_eq!(
            second,
            RetryScheduled {
                attempt: 2,
                at: at(32),
                until: Some(at(600)),
            }
        );
        assert_eq!(
            RetrySchedule::after(&retry, &second, at(32)),
            Ok(RetryScheduled {
                attempt: 3,
                at: at(72),
                until: Some(at(600)),
            })
        );
    }

    #[test]
    fn the_retries_stop_once_the_attempts_are_spent() {
        let retry = policy(3, SignedDuration::from_mins(10));
        let last = RetryScheduled {
            attempt: 3,
            at: at(70),
            until: None,
        };
        assert_eq!(
            RetrySchedule::after(&retry, &last, at(70)),
            Err(NoRetry::Exhausted)
        );
        let past = RetryScheduled {
            attempt: u32::MAX,
            ..last
        };
        assert_eq!(
            RetrySchedule::after(&retry, &past, at(70)),
            Err(NoRetry::Exhausted)
        );
    }

    #[test]
    fn a_retry_that_would_reach_the_series_end_is_none() {
        let retry = policy(5, SignedDuration::from_mins(20));
        let first = RetryScheduled {
            attempt: 1,
            at: at(20),
            until: Some(at(60)),
        };
        assert_eq!(
            RetrySchedule::after(&retry, &first, at(20)),
            Err(NoRetry::NextRun)
        );
        assert!(RetrySchedule::after(&retry, &first, at(19)).is_ok());
    }

    #[test]
    fn a_retry_past_the_last_time_there_is_never_comes() {
        let retry = policy(3, SignedDuration::from_mins(10));
        assert_eq!(
            RetrySchedule::first(&retry, Timestamp::MAX, None),
            Err(NoRetry::NextRun)
        );
    }

    #[test]
    fn a_retry_is_superseded_once_its_series_end_has_come() {
        let retry = RetryScheduled {
            attempt: 1,
            at: at(10),
            until: Some(at(60)),
        };
        assert!(!RetrySchedule::superseded(&retry, at(59)));
        assert!(RetrySchedule::superseded(&retry, at(60)));
        assert!(RetrySchedule::superseded(&retry, at(90)));
        let unscheduled = RetryScheduled {
            until: None,
            ..retry
        };
        assert!(!RetrySchedule::superseded(&unscheduled, at(90)));
    }
}
