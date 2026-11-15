//! The [`ReleaseFeed`](crate::ports::ReleaseFeed) contract (decision NN):
//! the same checks run against the scripted fake and the real GitHub
//! adapter, whose harness serves the answers on loopback, never the
//! network. A check fails on the first violation, as the other suites do.

use std::sync::Arc;

use domain::updates::{Release, Version};

use crate::ports::{ReleaseFeed, ReleaseFeedError};

/// How the feed the harness serves answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The newest release is the one tagged `tag`, marked a prerelease when
    /// `prerelease` is.
    Release { tag: &'static str, prerelease: bool },
    /// It answers with `status` and no release.
    Status(u16),
    /// It answers success with a body that isn't the feed's JSON.
    Garbage,
    /// It answers success with a release whose body runs past any cap a
    /// reader keeps.
    Oversized,
    /// Nothing listens at its address.
    Refuses,
    /// It takes the connection and never answers.
    Hangs,
}

/// Serves a feed for the reader under test.
pub trait ReleaseFeedHarness: Send + Sync {
    /// A feed reader asking a feed that answers as `answer`, with a timeout
    /// short enough for the hang check.
    fn feed(&self, answer: Answer) -> Arc<dyn ReleaseFeed>;
}

/// Runs the whole suite against `harness`.
pub async fn run(harness: &dyn ReleaseFeedHarness) {
    a_release_is_read_with_its_version(harness).await;
    a_release_marked_a_prerelease_is_told_as_one(harness).await;
    a_tag_that_is_no_version_is_unreadable(harness).await;
    a_status_other_than_success_is_a_rejection(harness).await;
    an_answer_that_is_not_the_feeds_is_unreadable(harness).await;
    an_answer_past_the_cap_is_unreadable(harness).await;
    a_feed_nobody_listens_at_is_unreachable(harness).await;
    a_feed_that_never_answers_times_out(harness).await;
}

fn version(text: &str) -> Option<Version> {
    let parsed = Version::parse(text);
    assert!(parsed.is_ok(), "the suite's own versions are valid: {text}");
    parsed.ok()
}

async fn newest(
    harness: &dyn ReleaseFeedHarness,
    answer: Answer,
) -> Result<Release, ReleaseFeedError> {
    harness.feed(answer).newest().await
}

async fn a_release_is_read_with_its_version(harness: &dyn ReleaseFeedHarness) {
    for (tag, text) in [("v0.3.0", "0.3.0"), ("v0.4.0-rc.1", "0.4.0-rc.1")] {
        let Some(version) = version(text) else { return };
        let read = newest(
            harness,
            Answer::Release {
                tag,
                prerelease: false,
            },
        )
        .await;
        assert_eq!(
            read,
            Ok(Release {
                version,
                prerelease: false
            }),
            "{tag}"
        );
    }
}

async fn a_release_marked_a_prerelease_is_told_as_one(harness: &dyn ReleaseFeedHarness) {
    let Some(version) = version("0.3.0") else {
        return;
    };
    let read = newest(
        harness,
        Answer::Release {
            tag: "v0.3.0",
            prerelease: true,
        },
    )
    .await;
    assert_eq!(
        read,
        Ok(Release {
            version,
            prerelease: true
        })
    );
}

async fn a_tag_that_is_no_version_is_unreadable(harness: &dyn ReleaseFeedHarness) {
    for tag in ["nightly", "0.3.0", "v0.3"] {
        let read = newest(
            harness,
            Answer::Release {
                tag,
                prerelease: false,
            },
        )
        .await;
        assert_eq!(read, Err(ReleaseFeedError::Unreadable), "{tag}");
    }
}

async fn a_status_other_than_success_is_a_rejection(harness: &dyn ReleaseFeedHarness) {
    for status in [404, 403, 500, 301] {
        let read = newest(harness, Answer::Status(status)).await;
        assert_eq!(read, Err(ReleaseFeedError::Rejected { status }), "{status}");
    }
}

async fn an_answer_that_is_not_the_feeds_is_unreadable(harness: &dyn ReleaseFeedHarness) {
    let read = newest(harness, Answer::Garbage).await;
    assert_eq!(read, Err(ReleaseFeedError::Unreadable));
}

async fn an_answer_past_the_cap_is_unreadable(harness: &dyn ReleaseFeedHarness) {
    let read = newest(harness, Answer::Oversized).await;
    assert_eq!(read, Err(ReleaseFeedError::Unreadable));
}

async fn a_feed_nobody_listens_at_is_unreachable(harness: &dyn ReleaseFeedHarness) {
    let read = newest(harness, Answer::Refuses).await;
    assert_eq!(read, Err(ReleaseFeedError::Unreachable));
}

async fn a_feed_that_never_answers_times_out(harness: &dyn ReleaseFeedHarness) {
    let read = newest(harness, Answer::Hangs).await;
    assert_eq!(read, Err(ReleaseFeedError::TimedOut));
}
