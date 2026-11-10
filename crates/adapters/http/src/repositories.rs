//! The routes the Repositories page speaks to (technical design §4.2): test a
//! repository, connect it, initialise one. A request carries the password a
//! person typed, so no answer carries it back (I4): an answer is an ID, or a
//! typed refusal whose arguments are names, never values.

use async_trait::async_trait;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::{AppState, refused_detailed as refused};

/// What the repository routes ask of the daemon: the three things the
/// Repositories page does, each from a request as it came, before anything
/// in it was read as a domain value.
#[async_trait]
pub trait Repositories: Send + Sync {
    /// Opens the repository the settings describe and reports its ID. Saves
    /// nothing.
    async fn test(
        &self,
        request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError>;

    /// Tests the repository and, once the test passed, saves it under the
    /// alias.
    async fn connect(
        &self,
        request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError>;

    /// Creates the repository where there is none, and saves it when the
    /// request names an alias.
    async fn init(
        &self,
        request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, RepositoriesError>;
}

/// Why a repository request was refused. Each variant is one typed refusal;
/// none holds a secret: a location is as the daemon redacts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositoriesError {
    /// The request parsed as JSON of the right shape, but holds a value the
    /// settings can't take: a relative password file, a keyring entry that
    /// is not `service/name`.
    InvalidBody,
    /// The alias is not a name.
    AliasInvalid,
    /// The configuration already holds a repository under the alias.
    AliasTaken { alias: String },
    /// A repository already exists at `location`, which is redacted.
    Exists { location: String },
    /// A secret reference couldn't be resolved: `field` says where it is.
    SecretUnresolved { field: String },
    /// The engine ran the job and it failed: the wrong password, no
    /// repository at the location. `detail` is the engine's own words.
    OperationFailed { detail: String },
    /// The daemon couldn't do it now: the engine or the configuration is
    /// unavailable. `code` says which, `args` are the arguments it declares,
    /// and `detail` is the platform's own words, when there are some.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

/// Tests the repository the body describes.
pub(crate) async fn test_repository(
    State(state): State<AppState>,
    body: Result<Json<api::TestRepositoryDto>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return refusal(&RepositoriesError::InvalidBody);
    };
    match state.repositories.test(request).await {
        Ok(id) => (StatusCode::OK, Json(id)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Connects the repository the body describes under its alias.
pub(crate) async fn connect_repository(
    State(state): State<AppState>,
    body: Result<Json<api::ConnectRepositoryDto>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return refusal(&RepositoriesError::InvalidBody);
    };
    match state.repositories.connect(request).await {
        Ok(id) => (StatusCode::CREATED, Json(id)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// Initialises the repository the body describes.
pub(crate) async fn init_repository(
    State(state): State<AppState>,
    body: Result<Json<api::InitRepositoryDto>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return refusal(&RepositoriesError::InvalidBody);
    };
    match state.repositories.init(request).await {
        Ok(initialised) => (StatusCode::CREATED, Json(initialised)).into_response(),
        Err(error) => refusal(&error),
    }
}

/// The typed refusal of `error`, with the status of its kind: a body that
/// can't be read is 400; a name the configuration can't take, or a job the
/// engine ran and failed, 422; something that is already there, 409; and the
/// daemon not being able to do it now, 503.
fn refusal(error: &RepositoriesError) -> Response {
    use api::{ErrorCode, MsgArg};
    let text = |name, value: &String| vec![(name, MsgArg::Text(value.clone()))];
    match error {
        RepositoriesError::InvalidBody => refused(
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidBody,
            Vec::new(),
            None,
        ),
        RepositoriesError::AliasInvalid => refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::RepositoryAliasInvalid,
            Vec::new(),
            None,
        ),
        RepositoriesError::AliasTaken { alias } => refused(
            StatusCode::CONFLICT,
            ErrorCode::RepositoryAliasTaken,
            text("alias", alias),
            None,
        ),
        RepositoriesError::Exists { location } => refused(
            StatusCode::CONFLICT,
            ErrorCode::RepositoryExists,
            text("location", location),
            None,
        ),
        RepositoriesError::SecretUnresolved { field } => refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::SecretUnresolved,
            text("field", field),
            None,
        ),
        RepositoriesError::OperationFailed { detail } => refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::OperationFailed,
            Vec::new(),
            Some(detail.clone()),
        ),
        RepositoriesError::Unavailable { code, args, detail } => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            *code,
            args.clone(),
            detail.clone(),
        ),
    }
}
