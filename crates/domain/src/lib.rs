//! Ladderback's domain model (technical design §3.1).
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

pub mod execution;
mod ids;
pub mod maintenance;
pub mod planning;
#[cfg(test)]
mod properties;
pub mod secrets;
pub mod service;
pub mod snapshot;

pub use ids::{
    AgentId, IdKind, InvalidId, OperationId, PlanId, RepositoryAlias, RepositoryId, SnapshotId,
};
pub use jiff::tz::TimeZone;
pub use jiff::{SignedDuration, Timestamp};
pub use uuid::Uuid;
