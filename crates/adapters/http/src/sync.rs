//! The agent routes under `/sync/` (technical design §7.1), which only a
//! build with the `hub` feature serves (§16, decision 122). A build without
//! it answers 404 on every path there, so a release never shows the UI's
//! page, or anything of the hub, at the hub's address.

use axum::Router;

use crate::AppState;

/// The routes under `/sync/` this build has.
pub(crate) fn routes() -> Router<AppState> {
    #[cfg(feature = "hub")]
    {
        // The handshake's routes join here (#1040).
        Router::new()
    }
    #[cfg(not(feature = "hub"))]
    {
        todo!()
    }
}
