//! Store adapter: the SQLite database behind the operation log and history
//! (technical design §8.3 and §9). Queries are checked at run time by the
//! contract suite, not at compile time (§8.2).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::Path;

use app::ports::{
    EngineResult, HistoryPage, HistoryQuery, OperationDetail, OperationStore, StoreError,
};
use async_trait::async_trait;
use domain::OperationId;
use domain::execution::Operation;
use sqlx::SqlitePool;

#[cfg(test)]
mod pool_tests;

/// Why the database couldn't be opened.
#[derive(Debug)]
pub enum OpenError {
    /// The file can't be created or opened as a database. `detail` is SQLite's own message.
    Database { detail: String },
    /// The database was written by a newer version: it holds a migration this
    /// build doesn't know. Opening it would risk its data.
    NewerSchema { version: i64 },
    /// A migration this build ships failed or doesn't match the one applied.
    Migration { detail: String },
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for OpenError {}

/// The operation log in one SQLite file, in WAL mode.
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Opens the database at `path`, creating it when missing, and brings its
    /// schema up to date.
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        todo!()
    }

    /// Closes every connection, waiting for writes in flight.
    pub async fn close(&self) {
        todo!()
    }
}

#[async_trait]
impl OperationStore for SqliteStore {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        todo!()
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        todo!()
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        todo!()
    }
}
