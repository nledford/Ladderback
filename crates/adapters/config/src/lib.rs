//! Config adapter: the agent's `config.toml` behind the `ConfigStore` port
//! (technical design §6.1). It owns the file format, its schema versions and
//! their migrations, the atomic write and the history of earlier versions;
//! `domain` keeps only the rules that hold whatever the format is.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
// `ConfigError` is the port's, and its shape is fixed there: a located error
// carries the file, the key, the place and the detail, which makes it larger
// than clippy likes for a `Result`. Loading a file is not a hot path.
#![allow(clippy::result_large_err)]

#[cfg(test)]
mod keys;
mod locate;
#[cfg(test)]
mod parity;
mod read;
mod schema;
mod store;
mod tables;
mod write;

pub use schema::{
    CURRENT_SCHEMA, KEPT_VERSIONS, MAX_CONFIG_BYTES, MigrationError, Parsed, Schemas, Step, parse,
    render,
};
pub use store::FileConfigStore;
