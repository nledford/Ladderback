//! The machine routes, `/api/v1/machine` (§7.1, §16 decisions 144 and 150):
//! the start page reads whether this machine's data folder was copied from
//! another computer, or its hub refused it as a second computer, and, for
//! either, asks for a new machine ID. The routes aren't in the API tokens'
//! scope table, so only a session (or anyone, with sign-in off) reaches
//! them, and the request checks hold the change to the page's own origin.

use std::sync::Arc;

use api::{ErrorCode, MachineDto, MachineStandingDto};
use app::machine::{NewIdError, Standing, TakeNewId};
use app::ports::config_unusable;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::{AppState, refused, refused_detailed, refused_with};

/// The routes' paths.
pub(crate) const MACHINE_PATH: &str = "/api/v1/machine";
pub(crate) const NEW_ID_PATH: &str = "/api/v1/machine/new-id";

/// Where this machine stands, and what takes a new machine ID for a copied
/// data folder or a machine its hub refused. Without one, as in a test's
/// daemon that never checked, the machine stands as usual and no new ID
/// can be taken.
#[derive(Clone, Default)]
pub struct MachineRoute {
    take: Option<Arc<TakeNewId>>,
}

impl MachineRoute {
    pub fn new(take: Arc<TakeNewId>) -> Self {
        Self { take: Some(take) }
    }

    fn standing(&self) -> Standing {
        self.take
            .as_ref()
            .map_or(Standing::Same, |take| take.standing())
    }
}

fn answer(standing: Standing) -> Response {
    let standing = match standing {
        Standing::Same => MachineStandingDto::Same,
        Standing::Copied => MachineStandingDto::Copied,
        Standing::NewIdTaken => MachineStandingDto::NewIdTaken,
        Standing::RefusedByHub => MachineStandingDto::RefusedByHub,
        Standing::RemovedByHub => todo!(),
        Standing::Unread => MachineStandingDto::Unread,
    };
    (StatusCode::OK, Json(MachineDto { standing })).into_response()
}

/// `GET /api/v1/machine`: where this machine stands.
pub(crate) async fn read(State(state): State<AppState>) -> Response {
    answer(state.machine.standing())
}

/// `POST /api/v1/machine/new-id`: takes a new machine ID for a copied data
/// folder, or a machine its hub refused as a second computer, giving up its
/// maintenance first, then answers as the read does. Any other machine, or
/// one that took a new ID already, keeps its ID.
pub(crate) async fn take_new_id(State(state): State<AppState>) -> Response {
    let Some(take) = &state.machine.take else {
        return refused(StatusCode::CONFLICT, ErrorCode::MachineNotCopied);
    };
    match take.take().await {
        Ok(id) => {
            log::warn!(
                "took the new machine ID {} for this computer, as its data folder was copied from another computer or its hub refused it as a second computer, and turned off its maintenance of every repository: restart Ladderback to start using it",
                id.as_uuid()
            );
            answer(take.standing())
        }
        Err(NewIdError::NotCopied) => refused(StatusCode::CONFLICT, ErrorCode::MachineNotCopied),
        // The configuration's refusal, `503` with its `config-*` code, as
        // every route that changes the configuration answers it.
        Err(NewIdError::Config(error)) => config_unusable(error, |code, args, detail| {
            refused_detailed(StatusCode::SERVICE_UNAVAILABLE, code, args, detail)
        }),
        // As turning maintenance off answers it: the operation that keeps
        // this machine maintaining a repository (I1).
        Err(NewIdError::Busy { running }) => refused_with(
            StatusCode::CONFLICT,
            ErrorCode::MaintenanceBusy,
            vec![(
                "operation",
                api::MsgArg::Text(running.as_uuid().hyphenated().to_string()),
            )],
        ),
        // The log's own words say what it couldn't read, as turning
        // maintenance off tells it.
        Err(NewIdError::Log(
            app::ports::StoreError::Unavailable { detail }
            | app::ports::StoreError::Corrupt { detail },
        )) => refused_detailed(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::OperationFailed,
            Vec::new(),
            Some(detail),
        ),
        Err(NewIdError::Machine(error)) => {
            // The system's words stay in the daemon's log.
            log::warn!("no new machine ID was taken: {}", error.detail);
            refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::OperationFailed,
            )
        }
    }
}

#[cfg(test)]
mod tests;
