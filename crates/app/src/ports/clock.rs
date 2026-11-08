use async_trait::async_trait;
use domain::Timestamp;

/// Wall-clock time and timers. The only source of "now" for `app`.
#[async_trait]
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;

    /// Returns once `now()` is at or after `deadline`; at once if it already is.
    async fn sleep_until(&self, deadline: Timestamp);
}
