//! The daily copy of the database (technical design §9; §16, decision 174):
//! `ladderback.db.daily` beside the database, dated by its file's
//! modification time.

use app::ports::{DailyCopy, DailyCopyError};
use async_trait::async_trait;
use domain::Timestamp;

use crate::SqliteStore;

#[async_trait]
impl DailyCopy for SqliteStore {
    async fn taken_at(&self) -> Result<Option<Timestamp>, DailyCopyError> {
        todo!()
    }

    async fn take(&self) -> Result<(), DailyCopyError> {
        todo!()
    }
}
