//! The port the release check asks for Ladderback's newest release through
//! (technical design §3.2, §5; decision NN): one request, which ends within
//! [`RELEASE_TIMEOUT`] whatever the other end does.

use std::time::Duration;

use async_trait::async_trait;
use domain::updates::Release;

/// The longest one request for the newest release takes, connecting,
/// sending and reading the answer together. A feed that hasn't answered by
/// then has [`ReleaseFeedError::TimedOut`].
pub const RELEASE_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the newest release couldn't be read. None holds the answer's text:
/// it is outside text, which the log never repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseFeedError {
    /// No connection could be made, or it broke before an answer.
    Unreachable,
    /// No answer within [`RELEASE_TIMEOUT`].
    TimedOut,
    /// The feed answered with a status other than success. A redirect is
    /// one: it is never followed.
    Rejected { status: u16 },
    /// The answer isn't one the feed sends: too large, not its JSON, or a
    /// tag that isn't `v` and a version.
    Unreadable,
}

impl ReleaseFeedError {
    /// The reason, as the debug log line says it.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::TimedOut => "timed-out",
            Self::Rejected { .. } => "rejected",
            Self::Unreadable => "unreadable",
        }
    }
}

/// Tells Ladderback's newest published release.
#[async_trait]
pub trait ReleaseFeed: Send + Sync {
    /// The newest release, asked for once. It is not asked again: the next
    /// check, a day later, asks anew.
    async fn newest(&self) -> Result<Release, ReleaseFeedError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_has_the_reason_the_log_line_names() {
        for (error, reason) in [
            (ReleaseFeedError::Unreachable, "unreachable"),
            (ReleaseFeedError::TimedOut, "timed-out"),
            (ReleaseFeedError::Rejected { status: 302 }, "rejected"),
            (ReleaseFeedError::Unreadable, "unreadable"),
        ] {
            assert_eq!(error.reason(), reason, "{error:?}");
        }
    }
}
