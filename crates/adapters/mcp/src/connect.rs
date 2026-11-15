//! Connect mode's [`Backend`] (§4.6): an HTTP client of a running daemon's
//! `/api/v1`, like any other API client. It depends on `api` alone, never on
//! `adapters/http` (§3.2).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use api::{
    ErrorCode, ErrorDto, MsgArg, OperationDetailDto, OperationsPageDto, PlansDto, RepositoriesDto,
    SessionDto, SignInStateDto, SnapshotsDto,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use reqwest::{Client, Url, redirect};
use serde::de::DeserializeOwned;

use crate::{
    Access, Backend, BackendStatus, DaemonStatus, PlanStatus, RECENT_OPERATIONS, not_cancellable,
    unfinished,
};

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
        let mut url = Url::parse(text).map_err(|_| InvalidDaemonUrl)?;
        let plain = matches!(url.scheme(), "http" | "https")
            && url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none();
        if !plain {
            return Err(InvalidDaemonUrl);
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(Self(url))
    }

    /// The URL of `route`, a path relative to the API's root.
    fn route(&self, route: &str) -> Option<Url> {
        self.0.join(route).ok()
    }

    /// Whether a token may be sent to this address: over `https`, or to this
    /// computer, a loopback address or `localhost`, as the daemon's request
    /// checks count it. Over plain HTTP to another machine, anyone on the way
    /// could read the token and use it (decision 87).
    fn keeps_a_token_private(&self) -> bool {
        if self.0.scheme() == "https" {
            return true;
        }
        let Some(host) = self.0.host_str() else {
            return false;
        };
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        match host.strip_prefix('[').and_then(|v6| v6.strip_suffix(']')) {
            Some(v6) => v6.parse::<Ipv6Addr>().is_ok_and(|addr| addr.is_loopback()),
            None => host
                .parse::<Ipv4Addr>()
                .is_ok_and(|addr| addr.is_loopback()),
        }
    }

    /// The URL of `route`, then `segments`, each percent-encoded as one
    /// segment of the path, so no text a tool is given reaches another
    /// route. A segment `.` or `..` would be left out: callers send none
    /// ([`segment`]).
    fn route_to(&self, route: &str, segments: &[&str]) -> Option<Url> {
        let mut url = self.route(route)?;
        url.path_segments_mut().ok()?.extend(segments);
        Some(url)
    }
}

/// `text` as one segment of a route's path, unless it is one no route takes
/// (empty, `.` or `..`), which names nothing the daemon holds.
fn segment(text: &str) -> Option<&str> {
    (!matches!(text, "" | "." | "..")).then_some(text)
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
        let text = text.trim();
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_graphic()) {
            return None;
        }
        let mut value = HeaderValue::try_from(format!("Bearer {text}")).ok()?;
        value.set_sensitive(true);
        Some(Self(value))
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
    token: Option<ApiToken>,
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
        Ok(Self {
            client,
            daemon,
            token: None,
        })
    }

    /// This backend, sending `token` with every request.
    ///
    /// # Errors
    ///
    /// The daemon's address is plain HTTP to another machine, where anyone
    /// on the way could read the token (decision 87). Without a token such
    /// an address is allowed.
    pub fn with_token(mut self, token: ApiToken) -> Result<Self, TokenOverPlainHttp> {
        if !self.daemon.keeps_a_token_private() {
            return Err(TokenOverPlainHttp);
        }
        self.token = Some(token);
        Ok(self)
    }

    /// The daemon's answer to `GET route/segments?query`, read as `T`, the
    /// segments as [`DaemonUrl::route_to`] joins them. A refusal is the
    /// daemon's own `{code, args}`; no answer, or one that isn't the API's,
    /// is `daemon-unreachable`.
    async fn get<T: DeserializeOwned>(
        &self,
        route: &str,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T, ErrorDto> {
        let mut url = self
            .daemon
            .route_to(route, segments)
            .ok_or_else(no_answer)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let mut request = self.client.get(url);
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, token.0.clone());
        }
        let response = request.send().await.map_err(|_| no_answer())?;
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
        let session: SessionDto = self.get("api/v1/session", &[], &[]).await?;
        // The session route answers a token `signed-out`, as it has no
        // session (decision 93), so access is told by how this program
        // connected: with sign-in off a token is ignored.
        let access = match (session.sign_in, &self.token) {
            (SignInStateDto::Off, _) => Access::SignInOff,
            (_, Some(_)) => Access::ApiToken,
            // Sign-in is on and no token was sent: every other route refuses.
            (_, None) => return Err(ErrorDto::new(ErrorCode::SignInRequired, [], None)),
        };
        let listed = self.plans().await?;
        let mut plans = Vec::with_capacity(listed.plans.len());
        for plan in listed.plans {
            let page = self.history(Some(plan.id.clone()), None, 1).await?;
            plans.push(PlanStatus {
                last: page.operations.into_iter().next(),
                plan,
            });
        }
        let recent = self.history(None, None, RECENT_OPERATIONS).await?;
        let running = unfinished(recent.operations);
        Ok(BackendStatus {
            access,
            daemon: Some(DaemonStatus {
                build: session.build,
                exposure: session.exposure,
            }),
            plans,
            running,
        })
    }

    async fn plans(&self) -> Result<PlansDto, ErrorDto> {
        self.get("api/v1/plans", &[], &[]).await
    }

    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto> {
        self.get("api/v1/repositories", &[], &[]).await
    }

    async fn snapshots(&self, repository: String) -> Result<SnapshotsDto, ErrorDto> {
        let Some(alias) = segment(&repository) else {
            return Err(ErrorDto::new(
                ErrorCode::RepositoryUnknown,
                [("alias", MsgArg::Text(repository))],
                None,
            ));
        };
        self.get("api/v1/repositories", &[alias, "snapshots"], &[])
            .await
    }

    async fn history(
        &self,
        plan: Option<String>,
        after: Option<String>,
        limit: u64,
    ) -> Result<OperationsPageDto, ErrorDto> {
        let limit = limit.to_string();
        let mut query = vec![("limit", limit.as_str())];
        query.extend(plan.as_deref().map(|plan| ("plan", plan)));
        query.extend(after.as_deref().map(|after| ("after", after)));
        self.get("api/v1/operations", &[], &query).await
    }

    async fn operation(&self, id: String) -> Result<OperationDetailDto, ErrorDto> {
        let Some(segment) = segment(&id) else {
            return Err(ErrorDto::new(
                ErrorCode::OperationUnknown,
                [("operation", MsgArg::Text(id))],
                None,
            ));
        };
        self.get("api/v1/operations", &[segment], &[]).await
    }
}

#[cfg(test)]
mod tests;
