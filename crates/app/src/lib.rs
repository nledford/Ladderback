//! Ladderback's application ring (technical design §3.2): use cases and
//! the port traits they drive. Depends only on `domain`, `api`, `async-trait`
//! and `futures`; time comes from the [`Clock`](ports::Clock) port.
//!
//! The `fakes` feature adds in-memory port fakes and the port contract
//! suites that every real adapter runs too.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod access;
pub mod announce;
pub mod browse;
pub mod conditions;
pub mod download;
pub mod event_bus;
pub mod execution;
pub mod full_disk_access;
pub mod globs;
pub mod history;
pub mod interop;
pub mod locations;
pub mod log_write;
pub mod maintenance;
pub mod notifications;
pub mod plans;
pub mod ports;
pub mod progress;
pub mod proposals;
pub mod queue;
pub mod recent_log;
pub mod recovery;
pub mod repositories;
pub mod restore;
pub mod retry;
pub mod schedule;
pub mod secrets;
pub mod snapshots;
pub mod stats;
pub mod updates;

#[cfg(feature = "fakes")]
pub mod contract;
#[cfg(feature = "fakes")]
pub mod fakes;
