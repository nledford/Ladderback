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
use axum::response::{IntoResponse, Response};

use crate::{AppState, refused_detailed as refused};

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
    match state.plans.plan(id).await {
        Ok(plan) => (StatusCode::OK, Json(plan)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Creates the plan the body holds.
pub(crate) async fn create(
    State(state): State<AppState>,
    body: Result<Json<api::PlanDto>, JsonRejection>,
) -> Response {
    let Ok(Json(plan)) = body else {
        return refusal(&PlansError::InvalidBody);
    };
    match state.plans.create(plan).await {
        Ok(saved) => (StatusCode::CREATED, Json(saved)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Changes the plan the path names to the one the body holds.
pub(crate) async fn change(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<api::PlanDto>, JsonRejection>,
) -> Response {
    let Ok(Json(plan)) = body else {
        return refusal(&PlansError::InvalidBody);
    };
    match state.plans.change(id, plan).await {
        Ok(saved) => (StatusCode::OK, Json(saved)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Deletes the plan the path names.
pub(crate) async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.plans.delete(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => refusal(&error),
    }
}

/// The typed refusal of `error`, with the status of its kind: a body that
/// isn't a plan's table is 400; a plan the configuration doesn't hold, 404;
/// a plan the file wouldn't load, 422; a plan a rule names, 409; and the
/// configuration not being usable now, 503.
fn refusal(error: &PlansError) -> Response {
    use api::{ErrorCode, MsgArg};
    let text = |value: &str| MsgArg::Text(value.to_owned());
    match error {
        PlansError::InvalidBody => refused(
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidBody,
            Vec::new(),
            None,
        ),
        PlansError::Invalid { field, problem } => refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::PlanInvalid,
            vec![("field", text(field)), ("problem", text(problem.as_str()))],
            None,
        ),
        PlansError::Unknown { plan } => refused(
            StatusCode::NOT_FOUND,
            ErrorCode::PlanUnknown,
            vec![("plan", text(plan))],
            None,
        ),
        PlansError::InUse { plan, rule } => refused(
            StatusCode::CONFLICT,
            ErrorCode::PlanInUse,
            vec![("plan", text(plan)), ("rule", text(rule))],
            None,
        ),
        PlansError::Unavailable { code, args, detail } => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            *code,
            args.clone(),
            detail.clone(),
        ),
    }
}

#[cfg(test)]
mod tests;
