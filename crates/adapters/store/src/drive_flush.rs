//! The flush of the drive's own cache (technical design §7.3, I1; §16,
//! decision 178). Each commit's log is synced before the commit returns
//! (`synchronous = FULL`, decision 164), but on macOS that sync stops at
//! the drive's own cache, which a power cut can lose. `F_FULLFSYNC`, which
//! the standard library's `File::sync_all` issues on macOS, asks the drive
//! to put everything it holds on its permanent storage: the store issues it
//! on the write-ahead log, which holds every commit since the last
//! checkpoint, and a checkpoint issues it itself (`checkpoint_fullfsync`,
//! set in `connect`). Elsewhere the commit's own sync reaches the disk, and
//! the flush only checks the database is open.

use app::ports::{DriveFlush, StoreError};
use async_trait::async_trait;

use crate::{SqliteStore, unavailable};

#[async_trait]
impl DriveFlush for SqliteStore {
    async fn flush(&self) -> Result<(), StoreError> {
        let _call = self.call().await;
        // A closed database may have commits no flush will reach: the
        // caller learns it, and sends nothing that depends on them.
        let _conn = self.pool.acquire().await.map_err(unavailable)?;
        #[cfg(target_os = "macos")]
        full_flush(&self.path).await?;
        Ok(())
    }
}

/// Flushes the drive the database at `path` is on, through its
/// write-ahead log. A log that isn't there holds nothing to flush: a
/// checkpoint moved it into the database file and flushed that.
#[cfg(target_os = "macos")]
async fn full_flush(path: &std::path::Path) -> Result<(), StoreError> {
    use std::fs::OpenOptions;
    use std::io;

    let log = crate::copy::beside(path, "-wal");
    let synced = tokio::task::spawn_blocking(move || {
        match OpenOptions::new().read(true).write(true).open(&log) {
            Ok(file) => file.sync_all(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    })
    .await;
    let failed = |detail: String| StoreError::Unavailable {
        detail: crate::report::cap(detail),
    };
    match synced {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(failed(error.to_string())),
        Err(error) => Err(failed(error.to_string())),
    }
}
