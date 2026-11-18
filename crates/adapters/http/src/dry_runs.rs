//! The plan dry run's routes (§16, decision 89, point 13): an AI agent, or
//! another API client with the `propose` scope, has a draft plan's sources
//! walked through its source filter and is told counts and sizes, never a
//! path; a person reads and sets the dry-run allowlist, the folders a dry
//! run may read, from a browser session only: the scope table names no
//! scope for it, so no token, and so no proposal, ever sets it.

use async_trait::async_trait;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::planning::Proposer;

use crate::api_tokens::TokenOf;
use crate::plans::PlansError;
use crate::proposals::asker;
use crate::{AppState, refused_detailed as refused};

/// What the dry-run routes ask of the daemon, each from a request as it
/// came. `asker` is who asks: the API token the request presented, or no
/// token.
#[async_trait]
pub trait DryRuns: Send + Sync {
    /// Walks the draft `plan`'s sources within the allowlist, through its
    /// source filter, and counts what it would back up.
    async fn dry_run(
        &self,
        asker: Proposer,
        plan: api::PlanDto,
    ) -> Result<api::PlanDryRunDto, DryRunsError>;

    /// The dry-run allowlist, and the daemon's home folder.
    async fn allowlist(&self) -> Result<api::DryRunAllowlistDto, DryRunsError>;

    /// Saves `folders` as the dry-run allowlist, and answers it.
    async fn set_allowlist(
        &self,
        folders: Vec<String>,
    ) -> Result<api::DryRunAllowlistDto, DryRunsError>;
}

/// Why a dry-run request was refused; nothing was read or changed. None
/// names a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DryRunsError {
    /// The body is not the request's shape.
    InvalidBody,
    /// The draft isn't a plan the configuration file would load.
    Plan(PlansError),
    /// Another dry run is running.
    Busy,
    /// A backup holds a place in the queue.
    GivesWay,
    /// The proposer started its limit of dry runs in the last hour: it may
    /// again in `seconds`.
    TooSoon { seconds: u64 },
    /// The plan's `setting` entry `entry`, from 1, isn't within the
    /// allowlist once resolved, or can't be resolved.
    NotAllowed { setting: &'static str, entry: usize },
    /// For the plan's `setting`, the walk would read files in the folders
    /// above its sources, outside the allowlist.
    ReadsAbove { setting: &'static str },
    /// The engine can't apply the plan's source filter.
    Filter,
    /// The walk failed; the daemon's log has the engine's words.
    Failed,
    /// The allowlist's folder `entry`, from 1, isn't an absolute path.
    FolderInvalid { entry: usize },
    /// The allowlist holds more than `limit` folders.
    TooLong { limit: usize },
    /// The configuration couldn't be read or written: `code` says which,
    /// with its `args`, and `detail` is the store's own words.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// `POST /api/v1/plans/dry-run`: dry-runs the body's plan.
pub(crate) async fn dry_run(
    State(state): State<AppState>,
    token: Option<axum::Extension<TokenOf>>,
    body: Result<Json<api::DryRunPlanDto>, JsonRejection>,
) -> Response {
    todo!(
        "{:?} {}",
        state.dry_runs.as_ref() as *const _,
        token.is_some() && body.is_ok()
    )
}

/// `GET /api/v1/settings/dry-run-allowlist`: the allowlist.
pub(crate) async fn allowlist(State(state): State<AppState>) -> Response {
    todo!("{:?}", state.dry_runs.as_ref() as *const _)
}

/// `PUT /api/v1/settings/dry-run-allowlist`: saves the body's folders.
pub(crate) async fn set_allowlist(
    State(state): State<AppState>,
    body: Result<Json<api::DryRunAllowlistDto>, JsonRejection>,
) -> Response {
    todo!("{:?} {}", state.dry_runs.as_ref() as *const _, body.is_ok())
}

/// A count as a message argument carries it.
fn count(value: usize) -> api::MsgArg {
    api::MsgArg::Count(u64::try_from(value).unwrap_or(u64::MAX))
}

/// The typed refusal of `error`: a body that isn't the request's, 400; a
/// draft the file wouldn't load, as the plan routes answer it; a path
/// outside the allowlist, 403; a filter the engine can't apply or an
/// allowlist that can't be saved, 422; another dry run or a backup going,
/// 409; a proposer that must wait, 429 with `Retry-After`; a walk that
/// failed or a configuration that can't be read, 503.
fn refusal(error: &DryRunsError) -> Response {
    todo!("{error:?}")
}

#[cfg(test)]
mod tests;
