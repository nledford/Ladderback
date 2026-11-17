//! The proposal routes (§16, decision 89): an AI agent, or another API
//! client, proposes configuration changes for a person to confirm, lists
//! and reads its proposals and withdraws one; a person reviews one on the
//! "Pending changes" page, reads its forget preview, then approves or
//! rejects it. An API token needs the `propose` scope, and sees and
//! withdraws only the proposals it made; a session, or any client while
//! sign-in is off, sees them all. Review, the forget preview, approval and
//! rejection are a session's only, never a token's (point 7): the preview
//! reads the repositories, resolving their secrets. A refusal names the
//! change and its setting, never a value (I4): a hook command may hold a
//! secret written in it.

use std::net::IpAddr;

use async_trait::async_trait;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::access::step_up::StepUpConfirmation;
use domain::planning::{ProposalStatus, Proposer};

use crate::api_tokens::TokenOf;
use crate::{AppState, refused_detailed as refused};

/// What the proposal routes ask of the daemon, each from a request as it
/// came, before anything in it but a step-up confirmation was read as a
/// domain value. `asker` is who asks: the API token the request presented,
/// or no token.
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

    /// The proposal `id` as the "Pending changes" page shows it.
    async fn review(&self, id: String) -> Result<api::ProposalReviewDto, ProposalsError>;

    /// The forget preview of the pending proposal `id` (decision 89, point
    /// 8): what it would change about the snapshots retention forgets in
    /// each repository it touches. A read: it changes nothing.
    async fn forget_preview(
        &self,
        id: String,
    ) -> Result<api::ProposalForgetPreviewDto, ProposalsError>;

    /// Confirms `approval` of the pending proposal `id` by step-up and
    /// applies it, answering it applied.
    async fn approve(
        &self,
        id: String,
        approval: Approval,
    ) -> Result<api::ProposalDto, ProposalsError>;

    /// Rejects the pending proposal `id` for `reasons`, one per change,
    /// answering it rejected.
    async fn reject(
        &self,
        id: String,
        reasons: Vec<String>,
    ) -> Result<api::ProposalDto, ProposalsError>;
}

/// An approval a request asks for: what the page showed, as the review
/// gave it, and the step-up confirmation.
#[derive(Debug)]
pub struct Approval {
    /// The file's revision the diff was rebuilt against.
    pub revision: String,
    /// The hash of the diff shown.
    pub diff_hash: String,
    /// The digest of the forget preview shown, when the page showed one.
    pub forget_digest: Option<String>,
    /// The connection's address, whose sign-in waits a wrong step-up
    /// password counts against.
    pub address: IpAddr,
    /// The admin password with sign-in on, the proposal's short ID
    /// otherwise. Never shown (I4).
    pub confirmation: StepUpConfirmation,
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
    /// The proposal is `status`, not pending, so nothing was done to it.
    NotPending { status: ProposalStatus },
    /// The proposer made as many proposals in the last hour as it may, or
    /// a person rejected one of its proposals a short while ago
    /// (`after_rejection`): it may propose again in `seconds`.
    TooSoon { after_rejection: bool, seconds: u64 },
    /// The configuration changed since the page showed the proposal, or
    /// by hand during its apply: nothing was applied, and it is pending.
    Changed,
    /// The proposal holds a change that could delete snapshots, approved
    /// only with the forget preview, and the approval brought none:
    /// nothing was applied.
    NeedsForgetPreview,
    /// What the proposal would forget isn't what the forget preview the
    /// approval brought showed: nothing was applied, and it is pending.
    ForgetPreviewChanged,
    /// A rejection gave another number of reasons than the proposal's
    /// `changes`.
    ReasonCount { changes: usize },
    /// A rejection's reason for `change`, from 1, is too long.
    ReasonTooLong { change: usize },
    /// The step-up confirmation wasn't the admin password, or the
    /// proposal's short ID with sign-in off.
    StepUpFailed,
    /// The address's wait after failed sign-ins has `seconds` left: the
    /// password wasn't checked.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own words,
    /// for the daemon's log only.
    StepUpUnavailable(String),
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
    match token {
        Some(axum::Extension(token)) => Proposer::Token {
            id: token.grant.id,
            label: token.grant.label,
        },
        None => Proposer::NoToken,
    }
}

/// The proposals the asker may see.
pub(crate) async fn list(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
) -> Response {
    match state.proposals.list(asker(token)).await {
        Ok(proposals) => (StatusCode::OK, Json(proposals)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Proposes the changes the body holds.
pub(crate) async fn propose(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    body: Result<Json<api::NewProposalDto>, JsonRejection>,
) -> Response {
    let Ok(Json(proposal)) = body else {
        return refusal(&ProposalsError::InvalidBody);
    };
    match state.proposals.propose(asker(token), proposal).await {
        Ok(made) => (StatusCode::CREATED, Json(made)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// The proposal the path names.
pub(crate) async fn read(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    Path(id): Path<String>,
) -> Response {
    match state.proposals.proposal(asker(token), id).await {
        Ok(proposal) => (StatusCode::OK, Json(proposal)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Withdraws the proposal the path names.
pub(crate) async fn withdraw(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    Path(id): Path<String>,
) -> Response {
    match state.proposals.withdraw(asker(token), id).await {
        Ok(proposal) => (StatusCode::OK, Json(proposal)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// `GET /api/v1/proposals/{id}/review`: the proposal as the page shows it.
pub(crate) async fn review(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.proposals.review(id).await {
        Ok(review) => (StatusCode::OK, Json(review)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// `POST /api/v1/proposals/{id}/forget-preview`: what the proposal would
/// change about the snapshots retention forgets, read from each repository
/// it touches. A POST, since it reads the repositories as a forget's
/// preview does, though it changes nothing.
pub(crate) async fn forget_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.proposals.forget_preview(id).await {
        Ok(preview) => (StatusCode::OK, Json(preview)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// `POST /api/v1/proposals/{id}/approve`: confirms the body's approval by
/// step-up and applies the proposal.
pub(crate) async fn approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    // A wrong step-up password counts in the waits of the connection's
    // address, as a sign-in does; without it, refuse as a sign-in does.
    let Some(ConnectInfo(crate::PeerAddr(peer))) = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .copied()
    else {
        log::error!(
            "an approval arrived without its connection's address; the server must be served with its connect info"
        );
        return refusal(&ProposalsError::StepUpUnavailable(
            "no connection address".to_owned(),
        ));
    };
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return refusal(&ProposalsError::InvalidBody);
    };
    let Ok(api::ApproveProposalDto {
        revision,
        diff_hash,
        forget_digest,
        confirmation,
    }) = serde_json::from_slice(&body)
    else {
        return refusal(&ProposalsError::InvalidBody);
    };
    // Nothing typed, or more than any password, is no confirmation: refused
    // as a wrong one, before the daemon is asked and in no sign-in wait.
    let Ok(confirmation) = StepUpConfirmation::parse(confirmation) else {
        return refusal(&ProposalsError::StepUpFailed);
    };
    let approval = Approval {
        revision,
        diff_hash,
        forget_digest,
        address: peer.ip(),
        confirmation,
    };
    match state.proposals.approve(id, approval).await {
        Ok(proposal) => (StatusCode::OK, Json(proposal)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// `POST /api/v1/proposals/{id}/reject`: rejects the proposal for the
/// body's reasons.
pub(crate) async fn reject(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<api::RejectProposalDto>, JsonRejection>,
) -> Response {
    let Ok(Json(api::RejectProposalDto { reasons })) = body else {
        return refusal(&ProposalsError::InvalidBody);
    };
    match state.proposals.reject(id, reasons).await {
        Ok(proposal) => (StatusCode::OK, Json(proposal)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// A count as a message argument carries it.
fn count(value: usize) -> api::MsgArg {
    api::MsgArg::Count(u64::try_from(value).unwrap_or(u64::MAX))
}

/// The typed refusal of `error`, with the status of its kind: a body that
/// isn't a proposal is 400; a proposal the configuration wouldn't take, or
/// a rejection's reasons that don't fit it, 422; a proposer at its limit, a
/// proposal no longer pending, or a configuration changed since the page
/// showed it, 409; a proposer that must wait, 429 with `Retry-After`; a
/// step-up that failed 403, during a wait 429, as a sign-in's; a proposal
/// the asker can't see, 404; and the daemon unable to read or keep them
/// now, 503.
fn refusal(error: &ProposalsError) -> Response {
    use api::{ErrorCode, MsgArg};
    let text = |value: &str| MsgArg::Text(value.to_owned());
    let (status, code, args) = match error {
        ProposalsError::InvalidBody => {
            (StatusCode::BAD_REQUEST, ErrorCode::InvalidBody, Vec::new())
        }
        ProposalsError::Empty => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProposalEmpty,
            Vec::new(),
        ),
        ProposalsError::Invalid {
            change,
            field,
            problem,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProposalInvalid,
            vec![
                ("change", count(*change)),
                ("field", text(field)),
                ("problem", text(problem.as_str())),
            ],
        ),
        ProposalsError::TooLarge { what, limit } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProposalTooLarge,
            vec![("what", text(what)), ("limit", count(*limit))],
        ),
        ProposalsError::Limit { limit } => (
            StatusCode::CONFLICT,
            ErrorCode::ProposalLimit,
            vec![("limit", count(*limit))],
        ),
        ProposalsError::Unknown { proposal } => (
            StatusCode::NOT_FOUND,
            ErrorCode::ProposalUnknown,
            vec![("proposal", text(proposal))],
        ),
        ProposalsError::NotPending { status } => (
            StatusCode::CONFLICT,
            ErrorCode::ProposalNotPending,
            vec![("status", text(status.as_str()))],
        ),
        ProposalsError::TooSoon {
            after_rejection,
            seconds,
        } => {
            let code = if *after_rejection {
                ErrorCode::ProposalAfterRejection
            } else {
                ErrorCode::ProposalRate
            };
            let mut response = refused(
                StatusCode::TOO_MANY_REQUESTS,
                code,
                vec![("seconds", MsgArg::Count(*seconds))],
                None,
            );
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from(*seconds),
            );
            return response;
        }
        ProposalsError::Changed => (StatusCode::CONFLICT, ErrorCode::ProposalChanged, Vec::new()),
        ProposalsError::NeedsForgetPreview => (
            StatusCode::CONFLICT,
            ErrorCode::ProposalNeedsForgetPreview,
            Vec::new(),
        ),
        ProposalsError::ForgetPreviewChanged => todo!(),
        ProposalsError::ReasonCount { changes } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProposalReasons,
            vec![("problem", text("count")), ("change", count(*changes))],
        ),
        ProposalsError::ReasonTooLong { change } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::ProposalReasons,
            vec![("problem", text("too-long")), ("change", count(*change))],
        ),
        ProposalsError::StepUpFailed => {
            (StatusCode::FORBIDDEN, ErrorCode::StepUpFailed, Vec::new())
        }
        ProposalsError::StepUpWait { seconds } => return crate::sign_in::wait_refused(*seconds),
        ProposalsError::StepUpUnavailable(detail) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be checked: {detail}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::SignInUnavailable,
                Vec::new(),
            )
        }
        ProposalsError::Unavailable { code, args, detail } => {
            return refused(
                StatusCode::SERVICE_UNAVAILABLE,
                *code,
                args.clone(),
                detail.clone(),
            );
        }
    };
    refused(status, code, args, None)
}

#[cfg(test)]
mod tests;
