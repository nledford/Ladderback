//! Where the daemon keeps its recent log (technical design §3.1, §4.2;
//! #618): the newest lines of the daemon log, from its start on, which the
//! Log page reads. The logger writes each line it writes to the log here too,
//! already masked (I4).

use domain::daemon_log::LogLine;

/// The recent log, kept within its caps (`domain::daemon_log::RecentLog`).
///
/// Both calls are synchronous and quick: the logger records from whatever
/// thread logs, so a record must never wait on anything but the lines
/// themselves, and never logs.
pub trait RecentLogStore: Send + Sync {
    /// Keeps `line` as the newest, letting go of the oldest lines past the
    /// caps.
    fn record(&self, line: LogLine);

    /// The lines kept, newest first.
    fn newest_first(&self) -> Vec<LogLine>;
}
