//! Connect mode's [`Backend`] (§4.6): an HTTP client of a running daemon's
//! `/api/v1`, like any other API client. It depends on `api` alone, never on
//! `adapters/http` (§3.2).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use api::{
    ErrorCode, ErrorDto, LocationsTestDto, MaintenancePreviewDto, MsgArg, NewProposalDto,
    OperationDetailDto, OperationSummaryDto, OperationsPageDto, PlanDryRunDto, PlanDto, PlansDto,
    ProfileExportDto, ProfileImportDto, ProposalDto, RepositoriesDto, SessionDto, SignInStateDto,
    SnapshotsDto,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue};
use reqwest::{Client, Url, redirect};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::{Access, Backend, BackendStatus, CANCELLABLE_KIND, Proposals, not_cancellable};

/// The daemon `--connect` reaches when it names none: `serve`'s default
/// address.
pub const DEFAULT_DAEMON_URL: &str = "http://127.0.0.1:9898";

/// How long the daemon may take to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one request may take, answer included. Most routes connect
/// mode calls answer from the daemon's store or memory; a snapshot listing,
/// a test and a forget preview read the repository, and one that takes
/// longer is `daemon-unreachable`, which a read can ask again (§4.6).
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
    /// segment of the path. A segment `.` or `..` would be left out, and a
    /// tab, line feed or carriage return stripped before that is checked, so
    /// callers send only what [`segment`] passes: then no text a tool is
    /// given reaches another route or names another item.
    fn route_to(&self, route: &str, segments: &[&str]) -> Option<Url> {
        let mut url = self.route(route)?;
        url.path_segments_mut().ok()?.extend(segments);
        Some(url)
    }
}

/// `text` as one segment of a route's path, unless it is one no route takes
/// (empty, `.` or `..`) or holds an ASCII control character, which names
/// nothing the daemon holds: the path would lose a tab, line feed or
/// carriage return, so `na\tme` would name `name`.
fn segment(text: &str) -> Option<&str> {
    (!matches!(text, "" | "." | "..") && !text.chars().any(|c| c.is_ascii_control()))
        .then_some(text)
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
            // A proxy the environment names (`HTTP_PROXY`, `ALL_PROXY`) would
            // be sent a plain `http` request to loopback, its token included
            // (decision 87): the daemon is reached directly or not at all.
            .no_proxy()
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
    /// daemon's own `{code, args}`; an answer over [`MAX_ANSWER_BYTES`] is
    /// [`too_large`]; no answer, or one that isn't the API's, is
    /// `daemon-unreachable`.
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
        let body = read_capped(response).await.map_err(|unread| match unread {
            Unread::TooLarge => too_large(),
            Unread::Broken => no_answer(),
        })?;
        if succeeded {
            serde_json::from_slice(&body).map_err(|_| no_answer())
        } else {
            Err(serde_json::from_slice(&body).unwrap_or_else(|_| no_answer()))
        }
    }

    /// The daemon's answer to `POST route/segments` with the JSON `body`,
    /// read as `T`. A refusal is the daemon's own `{code, args}`; no
    /// connection is `daemon-unreachable`. Anything else, once the request
    /// may have reached the daemon, is `lost()`: `daemon-answer-lost` for a
    /// request that changes something, as whether it took effect is unknown
    /// and it is never sent again (§4.6); `daemon-unreachable` for a read.
    async fn post<T: DeserializeOwned>(
        &self,
        route: &str,
        segments: &[&str],
        body: &serde_json::Value,
        lost: fn() -> ErrorDto,
    ) -> Result<T, ErrorDto> {
        let url = self
            .daemon
            .route_to(route, segments)
            .ok_or_else(no_answer)?;
        let mut request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string());
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, token.0.clone());
        }
        let response = request.send().await.map_err(|error| {
            if error.is_connect() {
                no_answer()
            } else {
                lost()
            }
        })?;
        let succeeded = response.status().is_success();
        let body = read_capped(response).await.map_err(|_| lost())?;
        if succeeded {
            serde_json::from_slice(&body).map_err(|_| lost())
        } else {
            Err(serde_json::from_slice(&body).unwrap_or_else(|_| lost()))
        }
    }
}

/// `daemon-unreachable`: no answer from a Ladderback API at the address.
fn no_answer() -> ErrorDto {
    ErrorDto::new(ErrorCode::DaemonUnreachable, [], None)
}

/// `repository-unknown` for `alias`, which names no repository a daemon
/// could hold.
fn unknown_repository(alias: String) -> ErrorDto {
    ErrorDto::new(
        ErrorCode::RepositoryUnknown,
        [("alias", MsgArg::Text(alias))],
        None,
    )
}

/// `operation-unknown` for `id`: the daemon holds no operation it names.
fn unknown_operation(id: String) -> ErrorDto {
    ErrorDto::new(
        ErrorCode::OperationUnknown,
        [("operation", MsgArg::Text(id))],
        None,
    )
}

/// `proposal-unknown` for `id`: the daemon holds no proposal it names that
/// the token sees.
fn unknown_proposal(id: String) -> ErrorDto {
    ErrorDto::new(
        ErrorCode::ProposalUnknown,
        [("proposal", MsgArg::Text(id))],
        None,
    )
}

/// `daemon-answer-lost`: a request that changes something may have reached
/// the daemon, and no answer of the API's came back.
fn answer_lost() -> ErrorDto {
    ErrorDto::new(ErrorCode::DaemonAnswerLost, [], None)
}

/// `daemon-answer-too-large`: the daemon answered with more than
/// [`MAX_ANSWER_BYTES`], which was left unread.
fn too_large() -> ErrorDto {
    ErrorDto::new(ErrorCode::DaemonAnswerTooLarge, [answer_limit()], None)
}

/// [`too_large`] for the detail of the operation whose ID is `id`, which
/// the message names and points to in the web UI.
fn too_large_operation(id: String) -> ErrorDto {
    ErrorDto::new(
        ErrorCode::DaemonAnswerTooLarge,
        [answer_limit(), ("operation", MsgArg::Text(id))],
        None,
    )
}

/// The `limit` argument of `daemon-answer-too-large`: [`MAX_ANSWER_BYTES`].
fn answer_limit() -> (&'static str, MsgArg) {
    (
        "limit",
        MsgArg::Bytes(u64::try_from(MAX_ANSWER_BYTES).unwrap_or(u64::MAX)),
    )
}

/// What `POST /api/v1/operations` answers: the started operation's ID.
#[derive(Deserialize)]
struct StartedOperation {
    operation: String,
}

/// What `POST /api/v1/operations/{id}/cancel` answers: an empty object.
#[derive(Deserialize)]
struct Nothing {}

/// Why an answer's body was left unread.
enum Unread {
    /// It is larger than [`MAX_ANSWER_BYTES`].
    TooLarge,
    /// The connection failed before its end.
    Broken,
}

/// The body of `response`, unless it is larger than [`MAX_ANSWER_BYTES`] or
/// can't be read.
async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>, Unread> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Unread::Broken)? {
        if body.len().saturating_add(chunk.len()) > MAX_ANSWER_BYTES {
            return Err(Unread::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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
        let _ = (access, session);
        todo!()
    }

    async fn plans(&self) -> Result<PlansDto, ErrorDto> {
        self.get("api/v1/plans", &[], &[]).await
    }

    async fn repositories(&self) -> Result<RepositoriesDto, ErrorDto> {
        self.get("api/v1/repositories", &[], &[]).await
    }

    async fn snapshots(&self, repository: String) -> Result<SnapshotsDto, ErrorDto> {
        let Some(alias) = segment(&repository) else {
            return Err(unknown_repository(repository));
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
            return Err(unknown_operation(id));
        };
        let answer = self.get("api/v1/operations", &[segment], &[]).await;
        answer.map_err(|error| {
            if error.code == ErrorCode::DaemonAnswerTooLarge {
                too_large_operation(id)
            } else {
                error
            }
        })
    }

    /// The first of the history page of one just past `id`: the history is
    /// newest first by ID, and its cursor need not name an operation that
    /// exists, so that page starts with the operation `id` names, when the
    /// daemon holds it (§4.6). Unlike the detail it is one summary long.
    async fn summary(&self, id: String) -> Result<OperationSummaryDto, ErrorDto> {
        let Ok(wanted) = id.parse::<Uuid>() else {
            return Err(unknown_operation(id));
        };
        // No ID is past the last one: the first page then starts with it.
        let past = wanted
            .as_u128()
            .checked_add(1)
            .map(|past| Uuid::from_u128(past).hyphenated().to_string());
        let mut query = vec![("limit", "1")];
        query.extend(past.as_deref().map(|past| ("after", past)));
        let page: OperationsPageDto = self.get("api/v1/operations", &[], &query).await?;
        page.operations
            .into_iter()
            .next()
            .filter(|first| first.id.parse::<Uuid>().ok() == Some(wanted))
            .ok_or_else(|| unknown_operation(id))
    }

    async fn run(&self, plan: String) -> Result<String, ErrorDto> {
        let started: StartedOperation = self
            .post(
                "api/v1/operations",
                &[],
                &serde_json::json!({ "plan": plan }),
                answer_lost,
            )
            .await?;
        Ok(started.operation)
    }

    async fn cancel(&self, id: String) -> Result<(), ErrorDto> {
        let operation = self.summary(id.clone()).await?;
        // A daemon refuses a token's cancel of anything but a backup; with
        // sign-in off there is no token, so the rule is kept here too.
        if operation.kind != CANCELLABLE_KIND {
            return Err(not_cancellable());
        }
        let Some(segment) = segment(&id) else {
            return Err(unknown_operation(id));
        };
        let Nothing {} = self
            .post(
                "api/v1/operations",
                &[segment, "cancel"],
                &serde_json::json!({}),
                answer_lost,
            )
            .await?;
        Ok(())
    }

    async fn test_repository(&self, repository: String) -> Result<LocationsTestDto, ErrorDto> {
        let Some(alias) = segment(&repository) else {
            return Err(unknown_repository(repository));
        };
        self.post(
            "api/v1/repositories",
            &[alias, "test"],
            &serde_json::json!({}),
            no_answer,
        )
        .await
    }

    async fn preview_forget(&self, repository: String) -> Result<MaintenancePreviewDto, ErrorDto> {
        let Some(alias) = segment(&repository) else {
            return Err(unknown_repository(repository));
        };
        self.post(
            "api/v1/repositories",
            &[alias, "maintenance", "preview"],
            &serde_json::json!({ "kind": "forget" }),
            no_answer,
        )
        .await
    }

    async fn preview_import(
        &self,
        content: String,
        repository: String,
    ) -> Result<ProfileImportDto, ErrorDto> {
        self.post(
            "api/v1/profiles/preview",
            &[],
            &serde_json::json!({ "content": content, "repository": repository }),
            no_answer,
        )
        .await
    }

    async fn export_profile(&self, plan: String) -> Result<ProfileExportDto, ErrorDto> {
        let Some(segment) = segment(&plan) else {
            return Err(ErrorDto::new(
                ErrorCode::PlanUnknown,
                [("plan", MsgArg::Text(plan))],
                None,
            ));
        };
        // With sign-in off the daemon can't tell this server from a browser,
        // so it is asked to leave the commands out (#856).
        self.get(
            "api/v1/plans",
            &[segment, "profile"],
            &[("commands", "left-out")],
        )
        .await
    }
}

/// The proposal routes, as the API token sent sees them: only its own
/// proposals, and never an approval (decision 89).
#[async_trait]
impl Proposals for Connect {
    async fn propose(&self, proposal: NewProposalDto) -> Result<ProposalDto, ErrorDto> {
        let body = serde_json::to_value(proposal).map_err(|_| no_answer())?;
        self.post("api/v1/proposals", &[], &body, answer_lost).await
    }

    async fn proposal(&self, id: String) -> Result<ProposalDto, ErrorDto> {
        let Some(segment) = segment(&id) else {
            return Err(unknown_proposal(id));
        };
        self.get("api/v1/proposals", &[segment], &[]).await
    }

    async fn withdraw(&self, id: String) -> Result<ProposalDto, ErrorDto> {
        let Some(segment) = segment(&id) else {
            return Err(unknown_proposal(id));
        };
        let body = serde_json::json!({});
        self.post(
            "api/v1/proposals",
            &[segment, "withdraw"],
            &body,
            answer_lost,
        )
        .await
    }

    // A read, as a preview is: an answer lost leaves nothing done.
    async fn dry_run(&self, plan: PlanDto) -> Result<PlanDryRunDto, ErrorDto> {
        let body = serde_json::json!({ "plan": plan });
        self.post("api/v1/dry-runs", &[], &body, no_answer).await
    }
}

#[cfg(test)]
mod tests;
