//! Engine adapter (technical design §4.1): every operation runs in its own
//! `ladderback worker` process. The daemon writes a [`job::JobSpec`] to the
//! worker's stdin and reads [`job::WorkerEvent`]s, one JSON object per line,
//! from its stdout.
//!
//! - [`worker`] is the worker side: it runs one job and reports events.
//! - [`WorkerEngine`] is the daemon side: it implements `app`'s `Engine`
//!   port by spawning workers and translating their events.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod cache_lock;
mod client;
mod environment;
pub mod job;
mod retention;
pub mod worker;

pub use client::WorkerEngine;
pub use retention::{RusticRetention, RusticSources};
