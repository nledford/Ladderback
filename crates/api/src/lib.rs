//! Wire types shared by the daemon, the hub, the worker and the web UI: the
//! published language (technical design §3.1). Builds for `wasm32`.
//!
//! Errors and warnings cross every boundary as codes with typed arguments,
//! never as sentences; the presentation layer renders them (§8.4).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod access;
mod codes;
mod events;
mod execution;
mod hub;
mod interop;
mod maintenance;
mod planning;
mod repositories;
mod restore;
mod snapshots;
mod status;
pub mod sync;
#[cfg(test)]
mod tests;
mod updates;

pub use access::*;
pub use codes::*;
pub use events::*;
pub use execution::*;
pub use hub::*;
pub use interop::*;
pub use maintenance::*;
pub use planning::*;
pub use repositories::*;
pub use restore::*;
pub use snapshots::*;
pub use status::*;
pub use updates::*;
