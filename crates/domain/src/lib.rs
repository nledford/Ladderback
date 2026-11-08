//! Steading's domain model (technical design §3.1).
//!
//! No I/O, no async, and no ambient time or randomness: times and IDs are
//! always passed in. This crate also builds for `wasm32-unknown-unknown`, so
//! the web UI can reuse its validation.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
// Red-first stubs: removed when the real code returns.
#![allow(unused, clippy::todo)]

pub mod execution;
mod ids;
pub mod secrets;
pub mod snapshot;

pub use ids::{AgentId, IdKind, InvalidId, OperationId, PlanId, RepositoryId, SnapshotId};
pub use jiff::{SignedDuration, Timestamp};
pub use uuid::Uuid;
