//! The daily copy of the database (technical design §9; §16, decision 174):
//! `ladderback.db.daily` beside the database, written by the store's own
//! copy (`copy::copy_to`) and dated by its file's modification time, so
//! the copy itself says when it was taken, across restarts too.

use std::io;
use std::path::PathBuf;

use app::ports::{DailyCopy, DailyCopyError};
use async_trait::async_trait;
use domain::Timestamp;

use crate::SqliteStore;
use crate::copy::{beside, copy_to};
use crate::report::cap;

/// The suffix of the daily copy, after the database's name.
const DAILY: &str = ".daily";

impl SqliteStore {
    /// Where the daily copy of this database is kept.
    fn daily_copy(&self) -> PathBuf {
        beside(&self.path, DAILY)
    }
}

#[async_trait]
impl DailyCopy for SqliteStore {
    async fn taken_at(&self) -> Result<Option<Timestamp>, DailyCopyError> {
        let failed = |error: io::Error| DailyCopyError::Failed {
            detail: cap(error.to_string()),
        };
        let modified = match std::fs::metadata(self.daily_copy()) {
            Ok(metadata) => metadata.modified().map_err(failed)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(failed(error)),
        };
        Timestamp::try_from(modified)
            .map(Some)
            .map_err(|error| DailyCopyError::Failed {
                detail: cap(error.to_string()),
            })
    }

    /// The copy holds the store for as long as it runs, so a `close`
    /// waits for it, and a copy asked for after one is
    /// [`DailyCopyError::Unavailable`]. It reads a snapshot, so the log's
    /// writes go on beside it.
    async fn take(&self) -> Result<(), DailyCopyError> {
        let _call = self.call().await;
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|error| DailyCopyError::Unavailable {
                detail: cap(error.to_string()),
            })?;
        copy_to(&mut conn, &self.path, &self.daily_copy())
            .await
            .map_err(|failed| DailyCopyError::Failed {
                detail: failed.detail,
            })
    }
}
