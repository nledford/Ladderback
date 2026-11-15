//! The routes the Plans page edits plans by (technical design §4.2, decision
//! 96): read one plan's table, create a plan, change one and delete one. A
//! plan travels as its table in the configuration file's keys, which the
//! daemon reads as the file is read when it loads. A refusal names the
//! plan's setting and what is wrong with it, never a value (I4): a hook
//! command may hold a secret written in it.

use async_trait::async_trait;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;

use crate::AppState;

/// What the plan routes ask of the daemon, each from a request as it came,
/// before anything in it was read as a domain value.
#[async_trait]
pub trait Plans: Send + Sync {
    /// The plan `id`'s table, as the file writes it.
    async fn plan(&self, id: String) -> Result<api::PlanDto, PlansError>;

    /// Adds the plan after the configuration's last one and answers it as
    /// saved.
    async fn create(&self, plan: api::PlanDto) -> Result<api::PlanDto, PlansError>;

    /// Puts the plan in place of the plan `id` and answers it as saved. A
    /// plan that leaves out its ID is the plan `id`; one that gives another
    /// ID is refused at `id`, for renaming is not a change of settings.
    async fn change(&self, id: String, plan: api::PlanDto) -> Result<api::PlanDto, PlansError>;

    /// Removes the plan `id`. Its snapshots stay in the repository.
    async fn delete(&self, id: String) -> Result<(), PlansError>;
}

/// Why a plan request was refused; nothing was saved. None holds a value
/// of the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlansError {
    /// The body is not a plan's table: not a JSON object, or one the
    /// file's format can't hold, such as one with a null in it.
    InvalidBody,
    /// The plan isn't one the file would load: `field` is the setting at
    /// fault, as its key in the plan's table, and `problem` what is wrong.
    Invalid {
        field: String,
        problem: api::PlanProblem,
    },
    /// The configuration holds no plan `plan`.
    Unknown { plan: String },
    /// The notification rule `rule` names the plan `plan`, so it stays.
    InUse { plan: String, rule: String },
    /// The daemon couldn't do it now: the configuration couldn't be read
    /// or written. `code` says which, `args` are the arguments it declares,
    /// and `detail` is the store's own words, when there are some.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// The plan the path names, as the Plans page's form shows it.
pub(crate) async fn read(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let _ = (state, id);
    todo!()
}

/// Creates the plan the body holds.
pub(crate) async fn create(
    State(state): State<AppState>,
    body: Result<Json<api::PlanDto>, JsonRejection>,
) -> Response {
    let _ = (state, body);
    todo!()
}

/// Changes the plan the path names to the one the body holds.
pub(crate) async fn change(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<api::PlanDto>, JsonRejection>,
) -> Response {
    let _ = (state, id, body);
    todo!()
}

/// Deletes the plan the path names.
pub(crate) async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let _ = (state, id);
    todo!()
}

#[cfg(test)]
mod tests;
