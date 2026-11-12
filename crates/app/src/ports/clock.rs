use async_trait::async_trait;
use domain::{SignedDuration, Timestamp};

/// Wall-clock and monotonic time, and timers. The only source of "now" for
/// `app`.
#[async_trait]
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;

    /// Monotonic time, from any start: it never goes back, and stands still
    /// while the machine sleeps. Compared with [`now`](Self::now), it tells
    /// a wake from sleep (`domain::execution::wake`).
    fn monotonic(&self) -> SignedDuration;

    /// Returns once `now()` is at or after `deadline`; at once if it already is.
    async fn sleep_until(&self, deadline: Timestamp);

    /// Returns once `duration` of monotonic time has passed since it was
    /// first polled; at once if `duration` isn't positive. Time the machine
    /// spends asleep doesn't count, and a clock set back or forward changes
    /// nothing.
    async fn sleep(&self, duration: SignedDuration);
}
