//! The routes the Snapshots page speaks to for maintenance (technical design
//! §4.2, §7.3): a repository's history grouped by who owns it, a preview of
//! each run before it starts, and the start itself. A preview and the groups
//! are reads: they take no maintenance slot and write nothing. A start that
//! carries the selection its preview named is refused when the repository has
//! changed since.

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::repositories::SecretRefusal;
use crate::{AppState, StartedOperation, refused_detailed};

/// What a maintenance request asks for, parsed: the trait's own type, so
/// adapters never see the wire's strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaintenanceRequest {
    Forget,
    Prune,
    Check,
    /// Rewrites the group `from` names with the tags of the plan `into`.
    Adopt {
        into: String,
        from: api::AdoptionFromDto,
    },
}

/// Why maintenance couldn't be started, previewed or listed. One refusal is
/// one typed code with the status of its kind; none holds a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaintenanceRefusal {
    /// No repository by that alias is configured.
    UnknownRepository,
    /// This agent doesn't maintain the repository (I1): nothing was read or
    /// run.
    NotOwner,
    /// The configuration holds no plan to adopt into. Names the plan.
    UnknownPlan { plan: String },
    /// Another maintenance operation holds the repository (I7). Names the
    /// one that does.
    Busy { running: String },
    /// A plan's retention policy would keep nothing; nothing was removed.
    /// Names the plan.
    KeepsNothing { plan: String },
    /// The group an adoption named matches no plan of this agent; nothing
    /// was written.
    AdoptionNoMatch,
    /// More than one plan matches the group an adoption named. Names them.
    AdoptionAmbiguous { plans: Vec<String> },
    /// The request parsed as JSON of the right shape but holds a value no
    /// run can take: a selection that is not a token.
    InvalidBody,
    /// What the run would act on is not what the selection it carried
    /// names: the repository changed after the preview. Nothing was done.
    SelectionChanged,
    /// A secret of the repository's settings couldn't be resolved, or a
    /// setting with a reference isn't valid: the refusal says which, as the
    /// repository routes and the snapshot listing tell it.
    SecretRefused(SecretRefusal),
    /// The engine ran the read and it failed: the wrong password, no
    /// repository at the location. `detail` is the engine's own words.
    OperationFailed { detail: String },
    /// The repository didn't answer a read in time.
    TimedOut,
    /// The daemon couldn't do it now: the engine, the configuration or the
    /// operation log is unavailable. `code` says which, `args` are the
    /// arguments it declares, and `detail` is the platform's own words, when
    /// there are some.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// Why a body couldn't be read as a request: the refusals only the route
/// that parses it makes.
#[derive(Debug, PartialEq, Eq)]
enum Unreadable {
    /// Not the JSON the route takes, or a kind that takes other fields.
    InvalidBody,
    /// A kind this version doesn't run. Carries it as it was sent.
    KindUnknown(String),
}

/// The request a body asks for, and the selection it holds the run to. An
/// adoption names its plan and group and no other kind takes either; a
/// selection belongs to a forget or an adoption that a person reviewed, and
/// only a start (`selects`) can be held to one. A body that carries what its
/// kind doesn't take is a body this route doesn't take, rather than a request
/// to ignore part of.
fn read(
    body: Result<Json<api::MaintenanceRequestDto>, JsonRejection>,
    selects: bool,
) -> Result<(MaintenanceRequest, Option<String>), Unreadable> {
    let Json(api::MaintenanceRequestDto {
        kind,
        into,
        from,
        selection,
    }) = body.map_err(|_| Unreadable::InvalidBody)?;
    let request = match (kind.as_str(), into, from) {
        ("forget", None, None) => MaintenanceRequest::Forget,
        ("prune", None, None) => MaintenanceRequest::Prune,
        ("check", None, None) => MaintenanceRequest::Check,
        ("adopt", Some(into), Some(from)) => MaintenanceRequest::Adopt { into, from },
        ("forget" | "prune" | "check" | "adopt", _, _) => return Err(Unreadable::InvalidBody),
        _ => return Err(Unreadable::KindUnknown(kind)),
    };
    let selectable = matches!(
        request,
        MaintenanceRequest::Forget | MaintenanceRequest::Adopt { .. }
    );
    match selection {
        Some(_) if !(selects && selectable) => Err(Unreadable::InvalidBody),
        selection => Ok((request, selection)),
    }
}

/// The refusal for a body that couldn't be read.
fn unreadable(error: Unreadable) -> Response {
    match error {
        Unreadable::InvalidBody => refused_detailed(
            StatusCode::BAD_REQUEST,
            api::ErrorCode::InvalidBody,
            Vec::new(),
            None,
        ),
        Unreadable::KindUnknown(kind) => refused_detailed(
            StatusCode::NOT_FOUND,
            api::ErrorCode::MaintenanceKindUnknown,
            vec![("kind", api::MsgArg::Text(kind))],
            None,
        ),
    }
}

/// One repository's snapshots grouped by who owns them: the plans of this
/// agent, the history no plan of this agent owns, and for each group the
/// plans that could take it. Read live, like the snapshot listing (§9).
pub(crate) async fn groups(State(state): State<AppState>, Path(alias): Path<String>) -> Response {
    match state.operations.groups(alias.clone()).await {
        Ok(groups) => (StatusCode::OK, Json(groups)).into_response(),
        Err(error) => refusal(&alias, error),
    }
}

/// What the run the body names would do to the repository the path names,
/// told before it starts. The answer is the preview; nothing runs.
pub(crate) async fn preview(
    State(state): State<AppState>,
    Path(alias): Path<String>,
    body: Result<Json<api::MaintenanceRequestDto>, JsonRejection>,
) -> Response {
    let request = match read(body, false) {
        Ok((request, _)) => request,
        Err(error) => return unreadable(error),
    };
    match state.operations.preview(alias.clone(), request).await {
        Ok(preview) => (StatusCode::OK, Json(preview)).into_response(),
        Err(error) => refusal(&alias, error),
    }
}

/// Starts maintenance on the repository the path names, of the kind the body
/// names: forget, prune, check or adopt. The answer is the operation's ID,
/// and the refusals are typed: an unknown repository or kind, a busy
/// repository (I7), a retention policy that would keep nothing, or a
/// selection the repository no longer matches.
pub(crate) async fn start(
    State(state): State<AppState>,
    Path(alias): Path<String>,
    body: Result<Json<api::MaintenanceRequestDto>, JsonRejection>,
) -> Response {
    let (request, selection) = match read(body, true) {
        Ok(read) => read,
        Err(error) => return unreadable(error),
    };
    match state
        .operations
        .start_maintenance(alias.clone(), request, selection)
        .await
    {
        Ok(operation) => {
            (StatusCode::ACCEPTED, Json(StartedOperation { operation })).into_response()
        }
        Err(error) => refusal(&alias, error),
    }
}

/// The typed refusal of `error` on the repository `alias`, with the status of
/// its kind: a body that can't be taken is 400; something unknown 404; a run
/// refused because of what the repository holds, or a read the engine ran
/// and failed, 422; a repository that is busy or has changed since its
/// preview 409; a read the repository didn't answer in time 504; and the
/// daemon not being able to do it now, 503.
fn refusal(alias: &str, error: MaintenanceRefusal) -> Response {
    use api::{ErrorCode, MsgArg};
    let text = |name, value: String| vec![(name, MsgArg::Text(value))];
    let (status, code, args, detail) = match error {
        MaintenanceRefusal::UnknownRepository => (
            StatusCode::NOT_FOUND,
            ErrorCode::RepositoryUnknown,
            text("alias", alias.to_owned()),
            None,
        ),
        MaintenanceRefusal::NotOwner => {
            todo!("the code and status of a repository maintained elsewhere")
        }
        MaintenanceRefusal::UnknownPlan { plan } => (
            StatusCode::NOT_FOUND,
            ErrorCode::PlanUnknown,
            text("plan", plan),
            None,
        ),
        MaintenanceRefusal::Busy { running } => (
            StatusCode::CONFLICT,
            ErrorCode::MaintenanceBusy,
            text("operation", running),
            None,
        ),
        MaintenanceRefusal::KeepsNothing { plan } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::RetentionKeepsNothing,
            text("plan", plan),
            None,
        ),
        MaintenanceRefusal::AdoptionNoMatch => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::AdoptionNoMatch,
            Vec::new(),
            None,
        ),
        MaintenanceRefusal::AdoptionAmbiguous { plans } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::AdoptionAmbiguous,
            text("plans", plans.join(", ")),
            None,
        ),
        MaintenanceRefusal::InvalidBody => (
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidBody,
            Vec::new(),
            None,
        ),
        MaintenanceRefusal::SelectionChanged => (
            StatusCode::CONFLICT,
            ErrorCode::MaintenanceSelectionChanged,
            Vec::new(),
            None,
        ),
        MaintenanceRefusal::SecretRefused(secret) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            secret.code,
            secret.args,
            secret.detail,
        ),
        MaintenanceRefusal::OperationFailed { detail } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::OperationFailed,
            Vec::new(),
            Some(detail),
        ),
        MaintenanceRefusal::TimedOut => (
            StatusCode::GATEWAY_TIMEOUT,
            ErrorCode::RepositoryReadTimedOut,
            Vec::new(),
            None,
        ),
        MaintenanceRefusal::Unavailable { code, args, detail } => {
            (StatusCode::SERVICE_UNAVAILABLE, code, args, detail)
        }
    };
    refused_detailed(status, code, args, detail)
}
