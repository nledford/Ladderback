//! Platform adapter: the operating system's clocks, what powers the machine
//! and whether it is online, and the agent ID file. The identity key arrives
//! with a later milestone (technical design §3.2).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod agent_id;
pub mod data_dir_lock;
pub mod dirs;
pub mod network;
pub mod power;
pub mod service;
mod source;

use app::ports::Clock;
use async_trait::async_trait;
use domain::{SignedDuration, Timestamp};

/// Wall-clock time from the OS, timers from tokio.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn monotonic(&self) -> SignedDuration {
        // `Instant` is CLOCK_MONOTONIC on Linux and CLOCK_UPTIME_RAW on
        // macOS: neither counts time the machine spends asleep. That is the
        // standard library's choice, not its promise (its `Instant` docs
        // leave time asleep to the platform), and a toolchain whose
        // `Instant` counted it would hide every wake (decision 63).
        static START: std::sync::LazyLock<std::time::Instant> =
            std::sync::LazyLock::new(std::time::Instant::now);
        let start = *START;
        SignedDuration::try_from(start.elapsed()).unwrap_or(SignedDuration::MAX)
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

    async fn sleep(&self, duration: SignedDuration) {
        // tokio timers run on the same clock as `Instant`.
        if let Ok(duration) = std::time::Duration::try_from(duration)
            && !duration.is_zero()
        {
            tokio::time::sleep(duration).await;
        }
    }
}

/// The longest a sleeper goes without re-reading wall-clock time.
const MAX_NAP: std::time::Duration = std::time::Duration::from_secs(30);

/// The host name this machine's backups carry: what a plan without a host of
/// its own records, and what plain rustic on this machine recorded of an
/// untagged snapshot. Read where the environment is read: in an adapter.
pub fn hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_default()
}

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
