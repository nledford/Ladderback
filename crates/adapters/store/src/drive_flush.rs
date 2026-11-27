//! The flush of the drive's own cache (technical design §7.3, I1; §16,
//! decision 178).

use app::ports::{DriveFlush, StoreError};
use async_trait::async_trait;

use crate::SqliteStore;

#[async_trait]
impl DriveFlush for SqliteStore {
    async fn flush(&self) -> Result<(), StoreError> {
        todo!()
    }
}
