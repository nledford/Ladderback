//! The Full Disk Access routes, `/api/v1/full-disk-access` (§4.2, §16
//! decision NN): the start page reads what the agent's Full Disk Access
//! probe found last, asks it to check again, and, from a browser on the
//! agent's own Mac, asks it to open System Settings' Full Disk Access pane.
//! The routes aren't in the API tokens' scope table, so only a session (or
//! anyone, with sign-in off) reaches them.

use std::sync::Arc;

use api::{ErrorCode, FullDiskAccessDto, FullDiskAccessStateDto};
use app::full_disk_access::FullDiskAccessCheck;
use async_trait::async_trait;
use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::execution::full_disk_access::FullDiskAccess;

use crate::{AppState, refused};

/// The routes' paths.
pub(crate) const FULL_DISK_ACCESS_PATH: &str = "/api/v1/full-disk-access";
pub(crate) const CHECK_PATH: &str = "/api/v1/full-disk-access/check";
pub(crate) const SETTINGS_PATH: &str = "/api/v1/full-disk-access/settings";

/// Opens System Settings' Full Disk Access pane, and shows the installed
/// program in the Finder, ready to add to its list.
#[async_trait]
pub trait SettingsOpener: Send + Sync {
    async fn open(&self) -> Result<(), SettingsUnopened>;
}

/// The pane couldn't be opened: the platform's own words, for the daemon's
/// log only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsUnopened {
    pub detail: String,
}

/// The agent's Full Disk Access probe and what opens the settings pane:
/// neither where the agent isn't on macOS, which has no such grant.
#[derive(Clone, Default)]
pub struct FullDiskAccessRoute {
    check: Option<Arc<FullDiskAccessCheck>>,
    opener: Option<Arc<dyn SettingsOpener>>,
}

impl FullDiskAccessRoute {
    /// On macOS: `check` runs the probe, and `opener` opens the pane.
    pub fn new(check: Arc<FullDiskAccessCheck>, opener: Arc<dyn SettingsOpener>) -> Self {
        Self {
            check: Some(check),
            opener: Some(opener),
        }
    }
}

/// Whether `request` comes from a browser on the agent's own Mac: its
/// connection's address is a loopback one, and so is the host it names. A
/// proxy on the same Mac forwards for whoever reached it, from a loopback
/// address and perhaps under a loopback host (nginx's default): such a
/// browser is offered the button, and its request to open the pane is still
/// refused, since its `Origin` doesn't name the host the request does (the
/// request checks). A request without its connection's address is from
/// nowhere known.
fn from_same_mac(request: &Request) -> bool {
    let peer_is_loopback = request
        .extensions()
        .get::<ConnectInfo<crate::PeerAddr>>()
        .is_some_and(|ConnectInfo(crate::PeerAddr(peer))| peer.ip().to_canonical().is_loopback());
    peer_is_loopback && crate::request_checks::host_is_loopback(request.headers())
}

/// What the route answers: what the probe found last, and whether this
/// browser may ask for the settings pane.
fn answer(route: &FullDiskAccessRoute, request: &Request) -> Response {
    let state = match &route.check {
        None => FullDiskAccessStateDto::NotApplicable,
        Some(check) => match check.found() {
            Some(FullDiskAccess::Granted) => FullDiskAccessStateDto::Granted,
            Some(FullDiskAccess::Missing) => FullDiskAccessStateDto::Missing,
            Some(FullDiskAccess::Unknown) | None => FullDiskAccessStateDto::Unknown,
        },
    };
    let dto = FullDiskAccessDto {
        state,
        opens_settings: route.opener.is_some() && from_same_mac(request),
    };
    (StatusCode::OK, Json(dto)).into_response()
}

/// `GET /api/v1/full-disk-access`: what the probe found last, and whether
/// this browser may ask for the settings pane.
pub(crate) async fn read(State(state): State<AppState>, request: Request) -> Response {
    answer(&state.full_disk_access, &request)
}

/// `POST /api/v1/full-disk-access/check`: runs the probe again, then answers
/// as the read does.
pub(crate) async fn check(State(state): State<AppState>, request: Request) -> Response {
    if let Some(check) = &state.full_disk_access.check {
        check.check().await;
    }
    answer(&state.full_disk_access, &request)
}

/// `POST /api/v1/full-disk-access/settings`: opens the settings pane, only
/// for a browser on the agent's own Mac.
pub(crate) async fn open_settings(State(state): State<AppState>, request: Request) -> Response {
    if !from_same_mac(&request) {
        return refused(StatusCode::FORBIDDEN, ErrorCode::FullDiskAccessNotLocal);
    }
    let Some(opener) = &state.full_disk_access.opener else {
        return refused(
            StatusCode::CONFLICT,
            ErrorCode::FullDiskAccessSettingsUnopened,
        );
    };
    match opener.open().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SettingsUnopened { detail }) => {
            // The platform's words stay in the daemon's log.
            log::warn!("System Settings could not be opened at Full Disk Access: {detail}");
            refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::FullDiskAccessSettingsUnopened,
            )
        }
    }
}

#[cfg(test)]
mod tests;
