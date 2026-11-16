//! The daemon's recent log: the real [`RecentLogStore`] behind the port
//! (technical design §4.2; #618), the lines in memory, within the caps the
//! domain's [`RecentLog`] keeps.

use std::sync::{Mutex, PoisonError};

use domain::daemon_log::{LogLine, RecentLog};

use crate::ports::RecentLogStore;

/// The recent log in memory, shared by the logger that records and the
/// routes that read.
#[derive(Debug, Default)]
pub struct RecentLogRing {
    log: Mutex<RecentLog>,
}

impl RecentLogRing {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RecentLogStore for RecentLogRing {
    fn record(&self, _line: LogLine) {
        let _held = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        todo!()
    }

    fn newest_first(&self) -> Vec<LogLine> {
        todo!()
    }
}
