//! The daily copy of the database (technical design §9; §16, decision 174):
//! the loop that keeps it, taking a copy whenever the one there is is a day
//! old, and none while it isn't, across restarts too. A copy that can't be
//! taken is told and tried again an hour later; it never touches an
//! operation.

use std::sync::Arc;

use domain::Timestamp;

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
    /// is due, then sleeps until the next is.
    pub async fn run(&self) {
        loop {
            let next = self.keep().await;
            self.clock.sleep_until(next).await;
        }
    }

    /// Takes a copy when one is due now, and answers when to look again: a
    /// day after the copy there is, or an hour from now when the copy
    /// there is couldn't be read or a copy couldn't be taken, which is
    /// told.
    pub async fn keep(&self) -> Timestamp {
        let _ = (&self.clock, &self.copies, &self.reports);
        todo!()
    }
}
