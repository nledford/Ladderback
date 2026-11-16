//! The in-memory recent log for tests: the daemon's real
//! [`RecentLogRing`](crate::recent_log::RecentLogRing), which keeps its lines
//! in memory already, so the contract suite covers both.

pub use crate::recent_log::RecentLogRing as InMemoryRecentLog;
