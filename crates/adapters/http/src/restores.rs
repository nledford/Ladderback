//! The restore routes (technical design §10.4): what restoring an entry of a
//! snapshot in place would do, from a dry run that writes nothing, and the
//! start of a restore, confirmed by step-up. Both are for a session only: no
//! API token scope opens them (§10.5, decision 93), so the scope table names
//! neither.

use std::net::IpAddr;

use async_trait::async_trait;
use axum::Json;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::SnapshotId;
use domain::browse::FolderPath;
use domain::restore::Destination;

use crate::maintenance::{self, MaintenanceRefusal};
use crate::{AppState, folder_unknown, refused, refused_with, snapshot_unknown};

/// What the restore routes ask of the daemon, the snapshot and the entry
/// already read as domain values.
#[async_trait]
pub trait Restores: Send + Sync {
    /// What restoring `entry` of `snapshot`, in the repository `alias`, in
    /// place would do: a dry run that writes nothing and records no
    /// operation.
    async fn preview(
        &self,
        alias: String,
        snapshot: SnapshotId,
        entry: FolderPath,
    ) -> Result<api::RestorePreviewDto, RestoreRefusal>;

    /// Confirms `start` by step-up and starts it, asked for in the agent's
    /// own web UI, returning the operation's ID and the new folder it
    /// writes into, if it makes one.
    async fn start(
        &self,
        alias: String,
        start: RestoreStart,
    ) -> Result<api::RestoreStartedDto, RestoreRefusal>;
}

/// A restore a request asks for, read from its path and body.
#[derive(Clone)]
pub struct RestoreStart {
    pub snapshot: SnapshotId,
    pub entry: FolderPath,
    pub destination: Destination,
    /// The connection's address, whose sign-in waits a wrong step-up
    /// password counts against.
    pub address: IpAddr,
    /// The admin password with sign-in on, the snapshot's short ID
    /// otherwise. Never logged or shown (I4): `Debug` hides it.
    pub confirmation: String,
}

impl std::fmt::Debug for RestoreStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreStart")
            .field("snapshot", &self.snapshot)
            .field("entry", &self.entry)
            .field("destination", &self.destination)
            .field("address", &self.address)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// Why a restore wasn't started or previewed. None holds a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreRefusal {
    /// The step-up confirmation wasn't the admin password, or the
    /// snapshot's short ID.
    StepUpFailed,
    /// The address's wait after failed sign-ins has `seconds` left: the
    /// password wasn't checked.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own words,
    /// for the daemon's log only.
    StepUpUnavailable(String),
    /// In place is offered only in the agent's own web UI.
    InPlaceNotHere,
    /// The agent doesn't take restores from the hub.
    HubNotAllowed,
    /// A restore into a new folder, and the agent has no folder to make it
    /// in: no home folder, and `[agent] restore-folder` unset.
    NoRestoreFolder,
    /// A restore runs on the agent already: names it.
    Running { operation: String },
    /// The repository holds no snapshot by that ID.
    SnapshotUnknown,
    /// Refused as a read of the repository is: an unknown repository, a
    /// configuration or a secret that can't be used, a read that failed or
    /// wasn't answered in time, or the daemon not able to start it now.
    Repository(MaintenanceRefusal),
}

/// `POST …/snapshots/{snapshot}/restore/preview`: what restoring the body's
/// entry in place would do.
pub(crate) async fn preview(
    State(state): State<AppState>,
    Path((alias, snapshot)): Path<(String, String)>,
    body: Result<Json<api::RestorePreviewRequestDto>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(api::RestorePreviewRequestDto { path })) = body else {
        return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody);
    };
    let Ok(id) = SnapshotId::parse(&snapshot) else {
        return snapshot_unknown(snapshot);
    };
    let Ok(entry) = FolderPath::parse(&path) else {
        return folder_unknown(path);
    };
    match state.restores.preview(alias.clone(), id, entry).await {
        Ok(preview) => (StatusCode::OK, Json(preview)).into_response(),
        Err(error) => refusal(&alias, &snapshot, error),
    }
}

/// `POST …/snapshots/{snapshot}/restore`: confirms the body's restore by
/// step-up and starts it. The answer is the operation's ID and, into a new
/// folder, that folder.
pub(crate) async fn start(
    State(state): State<AppState>,
    Path((alias, snapshot)): Path<(String, String)>,
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
            "a restore arrived without its connection's address; the server must be served with its connect info"
        );
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            api::ErrorCode::SignInUnavailable,
        );
    };
    // The body limit has already read the body, bounded by the API's limit
    // (`body_limit`), so this only takes the bytes it holds.
    let Ok(body) = axum::body::to_bytes(request.into_body(), usize::MAX).await else {
        return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody);
    };
    let Ok(api::RestoreRequestDto {
        path,
        destination,
        confirmation,
    }) = serde_json::from_slice::<api::RestoreRequestDto>(&body)
    else {
        return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody);
    };
    let Ok(id) = SnapshotId::parse(&snapshot) else {
        return snapshot_unknown(snapshot);
    };
    let Ok(entry) = FolderPath::parse(&path) else {
        return folder_unknown(path);
    };
    let start = RestoreStart {
        snapshot: id,
        entry,
        destination: match destination {
            api::RestoreDestinationDto::NewFolder => Destination::NewFolder,
            api::RestoreDestinationDto::InPlace => Destination::InPlace,
        },
        address: peer.ip(),
        confirmation,
    };
    match state.restores.start(alias.clone(), start).await {
        Ok(started) => (StatusCode::ACCEPTED, Json(started)).into_response(),
        Err(error) => refusal(&alias, &snapshot, error),
    }
}

/// The refusal of `error`, for the snapshot `snapshot` of the repository
/// `alias` as the request named them: a step-up that failed is 403, during a
/// wait 429 with `Retry-After`, as a sign-in's; a restore the agent doesn't
/// take here 403; one beside a running restore 409; one to a new folder on
/// an agent with no folder for restores 409, as a request sign-in being off
/// refuses is: the agent's configuration doesn't allow it now, and setting
/// it does; a snapshot the repository doesn't hold 404; the rest as a read
/// of the repository is.
fn refusal(alias: &str, snapshot: &str, error: RestoreRefusal) -> Response {
    use api::{ErrorCode, MsgArg};
    match error {
        RestoreRefusal::StepUpFailed => refused(StatusCode::FORBIDDEN, ErrorCode::StepUpFailed),
        RestoreRefusal::StepUpWait { seconds } => crate::sign_in::wait_refused(seconds),
        RestoreRefusal::StepUpUnavailable(detail) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("the admin password could not be checked: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::SignInUnavailable,
            )
        }
        RestoreRefusal::InPlaceNotHere => {
            refused(StatusCode::FORBIDDEN, ErrorCode::RestoreInPlaceNotHere)
        }
        RestoreRefusal::HubNotAllowed => {
            refused(StatusCode::FORBIDDEN, ErrorCode::RestoreHubNotAllowed)
        }
        RestoreRefusal::NoRestoreFolder => {
            refused(StatusCode::CONFLICT, ErrorCode::RestoreFolderUnset)
        }
        RestoreRefusal::Running { operation } => refused_with(
            StatusCode::CONFLICT,
            ErrorCode::RestoreRunning,
            vec![("operation", MsgArg::Text(operation))],
        ),
        RestoreRefusal::SnapshotUnknown => snapshot_unknown(snapshot.to_owned()),
        RestoreRefusal::Repository(refusal) => maintenance::refusal(alias, refusal),
    }
}

#[cfg(test)]
mod tests;
