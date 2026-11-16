//! The proposal routes (§16, decision 89): an AI agent, or another API
//! client, proposes configuration changes for a person to confirm, lists
//! and reads its proposals and withdraws one. An API token needs the
//! `propose` scope, and sees and withdraws only the proposals it made; a
//! session, or any client while sign-in is off, sees them all. Nothing here
//! changes the configuration. A refusal names the change and its setting,
//! never a value (I4): a hook command may hold a secret written in it.

use async_trait::async_trait;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::planning::{ProposalStatus, Proposer};

use crate::api_tokens::TokenOf;
use crate::{AppState, refused_detailed as refused};

/// What the proposal routes ask of the daemon, each from a request as it
/// came, before anything in it was read as a domain value. `asker` is who
/// asks: the API token the request presented, or no token.
#[async_trait]
pub trait Proposals: Send + Sync {
    /// Keeps the proposal, once its changes apply to the configuration, and
    /// answers it pending.
    async fn propose(
        &self,
        asker: Proposer,
        proposal: api::NewProposalDto,
    ) -> Result<api::ProposalDto, ProposalsError>;

    /// The proposals `asker` may see, newest first.
    async fn list(&self, asker: Proposer) -> Result<api::ProposalsDto, ProposalsError>;

    /// The proposal `id`, if `asker` may see it.
    async fn proposal(
        &self,
        asker: Proposer,
        id: String,
    ) -> Result<api::ProposalDto, ProposalsError>;

    /// Withdraws the pending proposal `id`, if `asker` may see it, and
    /// answers it withdrawn.
    async fn withdraw(
        &self,
        asker: Proposer,
        id: String,
    ) -> Result<api::ProposalDto, ProposalsError>;
}

/// Why a proposal request was refused; nothing was kept or changed. None
/// holds a value of a change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalsError {
    /// The body is not a proposal: not JSON of its shape, or a plan's table
    /// the file's format can't hold, such as one with a null in it.
    InvalidBody,
    /// The proposal holds no change.
    Empty,
    /// The change at `change`, from 1, isn't one the configuration takes:
    /// `field` is the setting at fault and `problem` what is wrong.
    Invalid {
        change: usize,
        field: String,
        problem: api::ChangeProblem,
    },
    /// The proposal holds more than `limit` of `what`: `changes`, or
    /// characters of `rationale`.
    TooLarge { what: &'static str, limit: usize },
    /// The proposer has `limit` proposals pending already.
    Limit { limit: usize },
    /// No proposal `proposal` is kept that the asker may see.
    Unknown { proposal: String },
    /// The proposal is `status`, not pending, so it wasn't withdrawn.
    NotPending { status: ProposalStatus },
    /// The daemon couldn't do it now: the configuration or the proposals
    /// couldn't be read or kept. `code` says which, `args` are the
    /// arguments it declares, and `detail` is the store's own words, when
    /// there are some.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// Who asks: the API token the request presented, else no token.
fn asker(token: Option<axum::Extension<TokenOf>>) -> Proposer {
    let _ = token;
    todo!()
}

/// The proposals the asker may see.
pub(crate) async fn list(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
) -> Response {
    let _ = (state, asker(token));
    todo!()
}

/// Proposes the changes the body holds.
pub(crate) async fn propose(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    body: Result<Json<api::NewProposalDto>, JsonRejection>,
) -> Response {
    let _ = (state, asker(token), body);
    todo!()
}

/// The proposal the path names.
pub(crate) async fn read(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    Path(id): Path<String>,
) -> Response {
    let _ = (state, asker(token), id);
    todo!()
}

/// Withdraws the proposal the path names.
pub(crate) async fn withdraw(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    Path(id): Path<String>,
) -> Response {
    let _ = (state, asker(token), id);
    todo!()
}

/// The typed refusal of `error`, with the status of its kind.
fn refusal(error: &ProposalsError) -> Response {
    let _ = (error, refused, StatusCode::OK, Json(()).into_response());
    todo!()
}

#[cfg(test)]
mod tests;
