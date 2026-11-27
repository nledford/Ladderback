//! The daily copy of the database (technical design §9; §16, decision 174):
//! the loop that keeps it, taking a copy whenever the one there is is a day
//! old, and none while it isn't, across restarts too. A copy that can't be
//! taken is told and tried again an hour later; it never touches an
//! operation.

use std::sync::Arc;

use domain::execution::daily_copy::{COPY_EVERY, RETRY_AFTER, next_copy};
use domain::{SignedDuration, Timestamp};

use crate::ports::{Clock, DailyCopy, DailyCopyError};

/// The loop that keeps the daily copy. It owns nothing but its ports; the
/// composition root runs [`run`](Self::run) while the daemon serves and
/// drops it when the daemon stops.
pub struct KeepDailyCopy {
    pub clock: Arc<dyn Clock>,
    pub copies: Arc<dyn DailyCopy>,
    /// Where it says why a copy wasn't taken, for the daemon's log.
    pub reports: Arc<dyn Fn(DailyCopyError) + Send + Sync>,
}

impl KeepDailyCopy {
    /// Keeps the daily copy until the future is dropped: takes one when it
    /// is due, then sleeps until the next is, but never more than an hour
    /// of monotonic time, so a clock set back while it sleeps, which
    /// leaves the copy dated after now, is noticed within the hour.
    pub async fn run(&self) {
        loop {
            let next = self.keep().await;
            let due = std::pin::pin!(self.clock.sleep_until(next));
            let nap = std::pin::pin!(self.clock.sleep(RETRY_AFTER));
            futures::future::select(due, nap).await;
        }
    }

    /// Takes a copy when one is due now, and answers when to look again: a
    /// day after the copy there is, or an hour from now when the copy
    /// there is couldn't be read or a copy couldn't be taken, which is
    /// told.
    pub async fn keep(&self) -> Timestamp {
        let now = self.clock.now();
        let taken = match self.copies.taken_at().await {
            Ok(taken) => taken,
            Err(error) => return self.not_taken(error, now),
        };
        let due = next_copy(taken, now);
        if due > now {
            return due;
        }
        match self.copies.take().await {
            Ok(()) => later(now, COPY_EVERY),
            Err(error) => self.not_taken(error, now),
        }
    }

    /// Tells `error`, and answers when to try again, at `now`.
    fn not_taken(&self, error: DailyCopyError, now: Timestamp) -> Timestamp {
        (self.reports)(error);
        later(now, RETRY_AFTER)
    }
}

/// `duration` after `moment`, or the last moment there is.
fn later(moment: Timestamp, duration: SignedDuration) -> Timestamp {
    moment.checked_add(duration).unwrap_or(Timestamp::MAX)
}
