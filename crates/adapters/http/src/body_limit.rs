//! Request body limits (technical design §10.1), always on.
//!
//! A route reads its whole body into memory before it runs, so a body of any
//! size would let one request take the daemon's memory from the backups it
//! owns. Each class of route has a limit of its own, the smallest that fits
//! the largest body that class takes today, and the limit is enforced here,
//! before a route runs: a route that reads its body some other way, or a new
//! one, is bounded without knowing about it.

use api::{ErrorCode, MsgArg};
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::CONTENT_LENGTH;
use axum::middleware::Next;
use axum::response::Response;
use http_body_util::{BodyExt, LengthLimitError, Limited};

use crate::refused_with;

/// The most a request to `/api/v1` may carry: twice the 64 KiB a secret field
/// may hold (`MAX_SECRET_BYTES`), so that the largest body any route takes
/// today, one repository's settings with a secret at its cap, fits with room
/// for the rest of the settings. A configuration import, which may need
/// more, gets a class of its own when its route exists.
pub(crate) const API_BODY_LIMIT: u64 = 128 * 1024;

/// The most a request for the web UI may carry: it reads files, so none.
pub(crate) const UI_BODY_LIMIT: u64 = 0;

/// The limit of the class of route `path` belongs to.
pub(crate) fn limit_for(path: &str) -> u64 {
    match path.strip_prefix("/api/v1") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => API_BODY_LIMIT,
        _ => UI_BODY_LIMIT,
    }
}

fn too_large(limit: u64) -> Response {
    refused_with(
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::BodyTooLarge,
        vec![("limit", MsgArg::Bytes(limit))],
    )
}

/// Reads the request's body up to its class's limit and hands the route the
/// bytes. A body over the limit is refused with `413` and the typed code; a
/// body the connection could not deliver is not JSON the route could take.
pub(crate) async fn enforce(request: Request, next: Next) -> Response {
    let limit = limit_for(request.uri().path());
    // A length the client declares over the limit is refused before a byte
    // of the body is read, which also keeps hyper from asking for it with
    // `100 Continue`. `Limited` below still bounds a length that lies, and a
    // body with none.
    let declared = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok());
    if declared.is_some_and(|declared| declared > limit) {
        return too_large(limit);
    }
    let (parts, body) = request.into_parts();
    let cap = usize::try_from(limit).unwrap_or(usize::MAX);
    match Limited::new(body, cap).collect().await {
        Ok(collected) => {
            let request = Request::from_parts(parts, Body::from(collected.to_bytes()));
            next.run(request).await
        }
        Err(error) if error.is::<LengthLimitError>() => too_large(limit),
        Err(_) => crate::refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_api_takes_a_body() {
        for path in [
            "/api/v1",
            "/api/v1/",
            "/api/v1/operations",
            "/api/v1/events",
        ] {
            assert_eq!(limit_for(path), API_BODY_LIMIT, "{path}");
        }
        for path in [
            "/",
            "/api",
            "/api/v10",
            "/api/v1x/operations",
            "/api/v2/operations",
            "//api/v1",
            "/sync/v1/connect",
            "/plans",
        ] {
            assert_eq!(limit_for(path), UI_BODY_LIMIT, "{path}");
        }
    }
}
