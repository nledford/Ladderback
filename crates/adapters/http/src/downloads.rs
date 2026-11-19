//! The download routes (technical design §10.4): the confirmation of a
//! download by step-up, answered with its ticket, and the hand-over of the
//! download waiting under a ticket, streamed a piece at a time. Both are
//! for a session only: no API token scope opens them (§10.5, decision 93),
//! so the scope table names neither.

use std::net::IpAddr;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use domain::SnapshotId;
use domain::access::step_up::StepUpConfirmation;
use domain::browse::FolderPath;
use domain::execution::FailureReason;
use domain::restore::download::{ArchiveFormat, DownloadShape, DownloadTicket};
use futures::StreamExt;
use futures::stream::BoxStream;

use crate::maintenance::{self, MaintenanceRefusal};
use crate::{AppState, refused, refused_with, snapshot_unknown};

/// What the download routes ask of the daemon, the snapshot, the entry and
/// the ticket already read as domain values.
#[async_trait]
pub trait Downloads: Send + Sync {
    /// Confirms `start` by step-up and readies it, returning the ticket its
    /// browser fetches it by.
    async fn start(
        &self,
        alias: String,
        start: DownloadStart,
    ) -> Result<DownloadTicket, DownloadRefusal>;

    /// The download waiting under `ticket`, which no one can fetch again;
    /// `None` when none waits under it.
    async fn fetch(&self, ticket: DownloadTicket) -> Option<HandOff>;
}

/// A download a request asks for, read from its path and body.
#[derive(Debug)]
pub struct DownloadStart {
    pub snapshot: SnapshotId,
    pub entry: FolderPath,
    pub format: ArchiveFormat,
    /// The connection's address, whose sign-in waits a wrong step-up
    /// password counts against.
    pub address: IpAddr,
    /// The admin password with sign-in on, the snapshot's short ID
    /// otherwise. Never shown (I4).
    pub confirmation: StepUpConfirmation,
}

/// A download handed over: the name to save it under, what it is, and its
/// pieces. An error ends a download the engine couldn't finish.
pub struct HandOff {
    pub file_name: String,
    pub shape: DownloadShape,
    pub pieces: BoxStream<'static, Result<Vec<u8>, FailureReason>>,
}

/// Why a download wasn't confirmed. None holds a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadRefusal {
    /// The step-up confirmation wasn't the admin password, or the
    /// snapshot's short ID.
    StepUpFailed,
    /// The address's wait after failed sign-ins has `seconds` left: the
    /// password wasn't checked.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own words,
    /// for the daemon's log only.
    StepUpUnavailable(String),
    /// No ticket could be drawn, the platform giving no random bytes: its
    /// own words, for the daemon's log only.
    TicketUnavailable(String),
    /// Two downloads are under way on the agent.
    Busy,
    /// The repository holds no snapshot by that ID.
    SnapshotUnknown,
    /// The snapshot holds no file or folder at the entry's path.
    EntryUnknown,
    /// Refused as a read of the repository is: an unknown repository, a
    /// configuration or a secret that can't be used, a read that failed or
    /// wasn't answered in time.
    Repository(MaintenanceRefusal),
}

/// `POST …/snapshots/{snapshot}/download`: confirms the body's download by
/// step-up. The answer is the ticket its browser fetches it by.
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
            "a download arrived without its connection's address; the server must be served with its connect info"
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
    let Ok(api::DownloadRequestDto {
        path,
        format,
        confirmation,
    }) = serde_json::from_slice::<api::DownloadRequestDto>(&body)
    else {
        return refused(StatusCode::BAD_REQUEST, api::ErrorCode::InvalidBody);
    };
    // Read at once, so the text is wiped on every way out. Nothing typed,
    // or more than any password, is no confirmation: refused as a wrong one,
    // before the daemon is asked and in no sign-in wait.
    let confirmation = StepUpConfirmation::parse(confirmation);
    let Ok(id) = SnapshotId::parse(&snapshot) else {
        return snapshot_unknown(snapshot);
    };
    let Ok(entry) = FolderPath::parse(&path) else {
        return entry_unknown(path);
    };
    let Ok(confirmation) = confirmation else {
        return refusal(&alias, &snapshot, &path, DownloadRefusal::StepUpFailed);
    };
    let start = DownloadStart {
        snapshot: id,
        entry,
        format: match format {
            api::ArchiveFormatDto::Tar => ArchiveFormat::Tar,
            api::ArchiveFormatDto::Zip => ArchiveFormat::Zip,
        },
        address: peer.ip(),
        confirmation,
    };
    match state.downloads.start(alias.clone(), start).await {
        Ok(ticket) => (
            StatusCode::CREATED,
            axum::Json(api::DownloadStartedDto {
                ticket: ticket.encode(),
            }),
        )
            .into_response(),
        Err(error) => refusal(&alias, &snapshot, &path, error),
    }
}

/// `GET /api/v1/downloads/{ticket}`: hands over the download waiting under
/// the ticket, once, as an attachment the browser saves.
pub(crate) async fn fetch(State(state): State<AppState>, Path(ticket): Path<String>) -> Response {
    let unknown = || refused(StatusCode::NOT_FOUND, api::ErrorCode::DownloadUnknown);
    let Some(ticket) = DownloadTicket::parse(&ticket) else {
        return unknown();
    };
    let Some(HandOff {
        file_name,
        shape,
        pieces,
    }) = state.downloads.fetch(ticket).await
    else {
        return unknown();
    };
    // A piece the engine couldn't give ends the body short, so the browser
    // fails the download rather than keep a part as the whole.
    let pieces = pieces.map(|piece| {
        piece.map_err(|reason| {
            log::warn!("a download ended before its end: {reason:?}");
            std::io::Error::other("the download ended before its end")
        })
    });
    let mut response = Body::from_stream(pieces).into_response();
    let headers = response.headers_mut();
    let content_type = match shape {
        DownloadShape::File { .. } => "application/octet-stream",
        DownloadShape::Archive(ArchiveFormat::Tar) => "application/x-tar",
        DownloadShape::Archive(ArchiveFormat::Zip) => "application/zip",
    };
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    // A body of a length is taken as sent once that many bytes are out, and
    // one of none is never read: an empty file goes without its length, so
    // its body is read to its end and the download's end is recorded.
    if let DownloadShape::File { size } = shape
        && size > 0
    {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    }
    if let Ok(disposition) = HeaderValue::from_str(&content_disposition(&file_name)) {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    response
}

/// `Content-Disposition` for a file saved as `name` (RFC 6266): an ASCII
/// `filename` for clients that read only it, every other character as `_`,
/// and the name itself in `filename*`, percent-encoded UTF-8 (RFC 8187).
fn content_disposition(name: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let ascii: String = name
        .chars()
        .map(|c| {
            if c == ' ' || (c.is_ascii_graphic() && c != '"' && c != '\\') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = name.bytes().fold(String::new(), |mut text, byte| {
        // RFC 8187's attr-char goes as it is; every other byte as `%XX`.
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            text.push(char::from(byte));
        } else {
            text.push('%');
            for nibble in [byte >> 4, byte & 0x0f] {
                if let Some(digit) = HEX.get(usize::from(nibble)) {
                    text.push(char::from(*digit));
                }
            }
        }
        text
    });
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// The refusal of an entry the snapshot doesn't hold, naming it as the
/// request did.
fn entry_unknown(path: String) -> Response {
    refused_with(
        StatusCode::NOT_FOUND,
        api::ErrorCode::EntryUnknown,
        vec![("path", api::MsgArg::Text(path))],
    )
}

/// The refusal of `error`, for the entry `path` of the snapshot `snapshot`
/// of the repository `alias` as the request named them: a step-up that
/// failed is 403, during a wait 429 with `Retry-After`, as a restore's; a
/// ticket that couldn't be drawn 503; a third download beside two under way
/// 409; a snapshot or an entry the repository doesn't hold 404; the rest as
/// a read of the repository is.
fn refusal(alias: &str, snapshot: &str, path: &str, error: DownloadRefusal) -> Response {
    use api::ErrorCode;
    match error {
        DownloadRefusal::StepUpFailed => refused(StatusCode::FORBIDDEN, ErrorCode::StepUpFailed),
        DownloadRefusal::StepUpWait { seconds } => crate::sign_in::wait_refused(seconds),
        DownloadRefusal::StepUpUnavailable(detail) => {
            // The platform's words name files of the data folder: the log
            // keeps them for whoever runs the daemon, not for whoever asked.
            log::warn!("a download could not be confirmed: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::SignInUnavailable,
            )
        }
        DownloadRefusal::TicketUnavailable(detail) => {
            log::warn!("a download could not be confirmed: {detail}");
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::DownloadsUnavailable,
            )
        }
        DownloadRefusal::Busy => refused(StatusCode::CONFLICT, ErrorCode::DownloadsBusy),
        DownloadRefusal::SnapshotUnknown => snapshot_unknown(snapshot.to_owned()),
        DownloadRefusal::EntryUnknown => entry_unknown(path.to_owned()),
        DownloadRefusal::Repository(refusal) => maintenance::refusal(alias, refusal),
    }
}

#[cfg(test)]
mod tests;
