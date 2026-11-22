//! The precompressed copies of the web UI's files (technical design §10.1):
//! which one a request's `Accept-Encoding` lets the daemon send.

use axum::http::HeaderMap;

/// A content coding the web UI's build keeps a copy of a file in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    Brotli,
    Gzip,
}

/// The codings the request accepts, the one it prefers first.
pub(crate) fn preferred(headers: &HeaderMap) -> [Option<Encoding>; 2] {
    let _ = headers;
    todo!()
}

#[cfg(test)]
mod tests;
