//! Platform adapter: the operating system's clocks. Power, network and
//! identity arrive with later milestones (technical design §3.2).

use app::ports::Clock;
use async_trait::async_trait;
use domain::Timestamp;

/// Wall-clock time from the OS, timers from tokio.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    async fn sleep_until(&self, deadline: Timestamp) {
        // tokio timers run on the monotonic clock, which stops while the
        // machine is suspended, so one long sleep would wake late by however
        // long the laptop slept. Waking at least every MAX_NAP to re-check
        // wall-clock time bounds that lateness, and handles clock jumps too.
        loop {
            let remaining = deadline.duration_since(Timestamp::now());
            let Ok(remaining) = std::time::Duration::try_from(remaining) else {
                return; // negative: the deadline has passed
            };
            if remaining.is_zero() {
                return;
            }
            tokio::time::sleep(remaining.min(MAX_NAP)).await;
        }
    }
}

/// The longest a sleeper goes without re-reading wall-clock time.
const MAX_NAP: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use app::contract;
    use app::contract::clock::ClockHarness;
    use domain::SignedDuration;

    use super::*;

    struct RealTime;

    #[async_trait]
    impl ClockHarness for RealTime {
        fn clock(&self) -> Arc<dyn Clock> {
            Arc::new(SystemClock)
        }

        async fn let_time_pass(&self, by: SignedDuration) {
            tokio::time::sleep(by.unsigned_abs()).await;
        }
    }

    #[tokio::test]
    async fn system_clock_meets_the_clock_contract() {
        contract::clock::run(&RealTime, SignedDuration::from_millis(150)).await;
    }
}
