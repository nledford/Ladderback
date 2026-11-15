//! Connect mode's [`Backend`] (§4.6): an HTTP client of a running daemon's
//! `/api/v1`, like any other API client. It depends on `api` alone, never on
//! `adapters/http` (§3.2).

use std::time::Duration;

use api::{ErrorCode, ErrorDto, OperationsPageDto, PlansDto, SessionDto};
use async_trait::async_trait;
use reqwest::header::HeaderValue;
use reqwest::{Client, Url, redirect};
use serde::de::DeserializeOwned;

use crate::{Access, Backend, BackendStatus, DaemonStatus, PlanStatus, not_cancellable};

/// The daemon `--connect` reaches when it names none: `serve`'s default
/// address.
pub const DEFAULT_DAEMON_URL: &str = "http://127.0.0.1:9898";

/// How long the daemon may take to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one request may take, answer included. Every route connect mode
/// reads answers from the daemon's store or memory.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The largest answer read, in bytes: a full history page with every
/// location tried is far below it.
const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;

/// How many of the newest operations `status` looks through for those
/// queued or running: the history's largest page.
const RECENT: &str = "200";

/// The address of a daemon's API: `http` or `https`, with a host, and no
/// user name, password, query or fragment. A password in the address would
/// be in the command line, which other users can read (I4). Routes are
/// joined below its path, so a daemon behind a reverse proxy at a path is
/// reached there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonUrl(Url);

/// A `--connect` address that isn't a [`DaemonUrl`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDaemonUrl;

impl DaemonUrl {
    /// Parses `text` as a daemon's address.
    ///
    /// # Errors
    ///
    /// It isn't an `http` or `https` URL with a host, or it holds a user
    /// name, a password, a query or a fragment.
    pub fn parse(text: &str) -> Result<Self, InvalidDaemonUrl> {
        todo!()
    }

    /// The URL of `route`, a path relative to the API's root.
    fn route(&self, route: &str) -> Option<Url> {
        self.0.join(route).ok()
    }
}

/// The HTTP client could not be built: its TLS could not start.
#[derive(Debug)]
pub struct ClientUnavailable(String);

impl std::fmt::Display for ClientUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ClientUnavailable {}

/// An API token (decision 93), as `--token-file` holds it: sent only as
/// `Authorization: Bearer`, and never shown, its `Debug` included.
#[derive(Clone)]
pub struct ApiToken(HeaderValue);

impl ApiToken {
    /// The token `text` holds, the spaces and line ends around it left out,
    /// unless it holds nothing else or a character no token has: anything
    /// but printable ASCII. Whether the daemon knows it is the daemon's to
    /// say.
    pub fn parse(text: &str) -> Option<Self> {
        let _ = text;
        todo!()
    }
}

impl std::fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiToken(…)")
    }
}

/// A token was to be sent over plain HTTP to another machine, which
/// [`Connect::with_token`] refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenOverPlainHttp;

/// The [`Backend`] of connect mode: a running daemon, through its API.
pub struct Connect {
    client: Client,
    daemon: DaemonUrl,
}

impl Connect {
    /// A backend that drives the daemon at `daemon`. Nothing is sent until a
    /// tool is called, so the daemon need not be up yet.
    ///
    /// # Errors
    ///
    /// The HTTP client could not be built.
    pub fn new(daemon: DaemonUrl) -> Result<Self, ClientUnavailable> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(redirect::Policy::none())
            .referer(false)
            .build()
            .map_err(|error| ClientUnavailable(error.to_string()))?;
        Ok(Self { client, daemon })
    }

    /// This backend, sending `token` with every request.
    ///
    /// # Errors
    ///
    /// The daemon's address is plain HTTP to another machine, where anyone
    /// on the way could read the token (decision 87). Without a token such
    /// an address is allowed.
    pub fn with_token(self, token: ApiToken) -> Result<Self, TokenOverPlainHttp> {
        let _ = token;
        todo!()
    }

    /// The daemon's answer to `GET route?query`, read as `T`. A refusal is
    /// the daemon's own `{code, args}`; no answer, or one that isn't the
    /// API's, is `daemon-unreachable`.
    async fn get<T: DeserializeOwned>(
        &self,
        route: &str,
        query: &[(&str, &str)],
    ) -> Result<T, ErrorDto> {
        let mut url = self.daemon.route(route).ok_or_else(no_answer)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| no_answer())?;
        let succeeded = response.status().is_success();
        let body = read_capped(response).await.ok_or_else(no_answer)?;
        if succeeded {
            serde_json::from_slice(&body).map_err(|_| no_answer())
        } else {
            Err(serde_json::from_slice(&body).unwrap_or_else(|_| no_answer()))
        }
    }
}

/// `daemon-unreachable`: no answer from a Ladderback API at the address.
fn no_answer() -> ErrorDto {
    ErrorDto::new(ErrorCode::DaemonUnreachable, [], None)
}

/// The body of `response`, unless it is larger than [`MAX_ANSWER_BYTES`] or
/// can't be read.
async fn read_capped(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len().saturating_add(chunk.len()) > MAX_ANSWER_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

#[async_trait]
impl Backend for Connect {
    async fn status(&self) -> Result<BackendStatus, ErrorDto> {
        todo!()
    }
}

#[cfg(test)]
mod tests;
