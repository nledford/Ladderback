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

mod keys;
#[cfg(test)]
mod parity;
mod schema;
mod store;

pub use schema::{
    CURRENT_SCHEMA, KEPT_VERSIONS, MAX_CONFIG_BYTES, MigrationError, Parsed, Schemas, Step, parse,
    render,
};
pub use store::FileConfigStore;
