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
use axum::middleware::Next;
use axum::response::Response;
use http_body_util::{BodyExt, LengthLimitError, Limited};

use crate::refused_with;

/// The most a request to `/api/v1` may carry. The largest body any of its
/// routes takes is one repository's settings, a few hundred bytes with a
/// password and a handful of options. A configuration import, which may need
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

/// Reads the request's body up to its class's limit and hands the route the
/// bytes. A body over the limit is refused with `413` and the typed code; a
/// body the connection could not deliver is not JSON the route could take.
pub(crate) async fn enforce(request: Request, next: Next) -> Response {
    let limit = limit_for(request.uri().path());
    let (parts, body) = request.into_parts();
    let cap = usize::try_from(limit).unwrap_or(usize::MAX);
    match Limited::new(body, cap).collect().await {
        Ok(collected) => {
            let request = Request::from_parts(parts, Body::from(collected.to_bytes()));
            next.run(request).await
        }
        Err(error) if error.is::<LengthLimitError>() => refused_with(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::BodyTooLarge,
            vec![("limit", MsgArg::Bytes(limit))],
        ),
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
