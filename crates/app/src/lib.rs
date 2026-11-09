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

pub mod execution;
pub mod history;
pub mod ports;
pub mod repositories;
pub mod secrets;
pub mod snapshots;

#[cfg(feature = "fakes")]
pub mod contract;
#[cfg(feature = "fakes")]
pub mod fakes;
