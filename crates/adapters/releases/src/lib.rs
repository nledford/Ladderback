//! The release feed adapter (technical design §3.2, §10; decision NN): the
//! [`ReleaseFeed`] port over GitHub's REST API. One `GET` of the newest
//! release, `/repos/nledford/Ladderback/releases/latest`, which carries no
//! cookie, no token and no referrer, only a `User-Agent` naming Ladderback
//! and the version it runs, as GitHub asks of every client, and the
//! `Accept` header GitHub documents.
//!
//! The client follows no redirect and gives up after its timeout. It reads
//! at most [`MAX_ANSWER_BYTES`] of the answer, and of it only the newest
//! release's tag and whether it is a draft or a prerelease: nothing else of
//! GitHub's text reaches the agent, its log or its pages. It honours the
//! standard proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`,
//! `NO_PROXY`, in either case), as the notifications adapter does.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use std::time::Duration;

use app::ports::{ReleaseFeed, ReleaseFeedError};
use async_trait::async_trait;
use domain::updates::{Release, Version};
use reqwest::{Client, Url};

/// Where Ladderback's newest release is read.
pub const LATEST_RELEASE: &str = "https://api.github.com/repos/nledford/Ladderback/releases/latest";

/// The most of an answer read, in bytes. GitHub's answer for one release
/// with its notes and a dozen assets is a few tens of kilobytes; an answer
/// past this cap is unreadable, never read whole.
pub const MAX_ANSWER_BYTES: usize = 1024 * 1024;

/// The HTTP client couldn't be built, or the feed's address isn't an `http`
/// or `https` URL. Holds no text of the library's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientError;

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the release feed's client couldn't be built")
    }
}

impl std::error::Error for ClientError {}

/// Reads Ladderback's newest release from GitHub Releases.
pub struct GitHubReleases {
    client: Client,
    url: Url,
}

impl GitHubReleases {
    /// A feed that asks `url` (normally [`LATEST_RELEASE`]) as Ladderback
    /// `running` does, each request ending within `timeout`.
    pub fn new(url: &str, running: &Version, timeout: Duration) -> Result<Self, ClientError> {
        todo!("{url} {running} {timeout:?} {:?}", MAX_ANSWER_BYTES)
    }
}

#[async_trait]
impl ReleaseFeed for GitHubReleases {
    async fn newest(&self) -> Result<Release, ReleaseFeedError> {
        todo!("{:?} {:?}", self.url, self.client)
    }
}
