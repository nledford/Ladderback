//! The release feed adapter (technical design §3.2, §10; decision 107): the
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
use reqwest::header::{ACCEPT, HeaderValue};
use reqwest::{Client, Url, redirect};
use serde::Deserialize;

/// Where Ladderback's newest release is read.
pub const LATEST_RELEASE: &str = "https://api.github.com/repos/nledford/Ladderback/releases/latest";

/// The most of an answer read, in bytes. GitHub's answer for one release
/// with its notes and a dozen assets is a few tens of kilobytes; an answer
/// past this cap is unreadable, never read whole.
pub const MAX_ANSWER_BYTES: usize = 1024 * 1024;

/// The media type GitHub documents for its REST API.
const GITHUB_JSON: &str = "application/vnd.github+json";

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
        let url = Url::parse(url).map_err(|_| ClientError)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ClientError);
        }
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(redirect::Policy::none())
            .referer(false)
            .user_agent(format!("ladderback/{running}"))
            .build()
            .map_err(|_| ClientError)?;
        Ok(Self { client, url })
    }
}

/// The part of GitHub's answer read: everything else is left unread.
#[derive(Deserialize)]
struct Answer {
    tag_name: String,
    prerelease: bool,
    draft: bool,
}

#[async_trait]
impl ReleaseFeed for GitHubReleases {
    async fn newest(&self) -> Result<Release, ReleaseFeedError> {
        let mut response = self
            .client
            .get(self.url.clone())
            .header(ACCEPT, HeaderValue::from_static(GITHUB_JSON))
            .send()
            .await
            .map_err(|error| failure(&error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ReleaseFeedError::Rejected {
                status: status.as_u16(),
            });
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| failure(&error))? {
            if body.len().saturating_add(chunk.len()) > MAX_ANSWER_BYTES {
                return Err(ReleaseFeedError::Unreadable);
            }
            body.extend_from_slice(&chunk);
        }
        let answer: Answer =
            serde_json::from_slice(&body).map_err(|_| ReleaseFeedError::Unreadable)?;
        let version =
            Version::of_tag(&answer.tag_name).map_err(|_| ReleaseFeedError::Unreadable)?;
        Ok(Release {
            version,
            // A draft is no published release: it counts as a prerelease.
            prerelease: answer.prerelease || answer.draft,
        })
    }
}

/// Why a request that got no whole answer failed.
fn failure(error: &reqwest::Error) -> ReleaseFeedError {
    if error.is_timeout() {
        ReleaseFeedError::TimedOut
    } else {
        ReleaseFeedError::Unreachable
    }
}
