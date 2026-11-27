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
/// write-ahead log, which an open store always keeps: one that isn't there
/// fails the flush, since the commits it held may be off the drive.
#[cfg(target_os = "macos")]
async fn full_flush(path: &std::path::Path) -> Result<(), StoreError> {
    use std::fs::OpenOptions;

    let log = crate::copy::beside(path, "-wal");
    let synced = tokio::task::spawn_blocking(move || {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&log)
            .and_then(|file| file.sync_all())
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

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use app::ports::{DriveFlush as _, StoreError};

    use crate::SqliteStore;

    /// An open store in a scratch folder, and its write-ahead log.
    async fn scratch() -> (tempfile::TempDir, SqliteStore, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("ladderback.db");
        let store = SqliteStore::open(&path)
            .await
            .expect("the scratch database opens");
        let log = crate::copy::beside(&path, "-wal");
        assert!(log.exists(), "an open store keeps its log");
        (dir, store, log)
    }

    /// The log an open store writes its commits to is always there, so one
    /// gone means the commits may be off the drive: nothing that depends on
    /// them goes out (I1; §16, decision 178).
    #[tokio::test]
    async fn a_store_whose_log_is_gone_doesnt_flush() {
        let (_dir, store, log) = scratch().await;
        std::fs::remove_file(&log).expect("the log is removed");
        let flushed = store.flush().await;
        assert!(
            matches!(flushed, Err(StoreError::Unavailable { .. })),
            "{flushed:?}"
        );
    }

    /// A log the flush can't open isn't flushed, and the flush says so.
    #[tokio::test]
    async fn a_store_whose_log_cant_be_opened_doesnt_flush() {
        let (_dir, store, log) = scratch().await;
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o000))
            .expect("the log's permissions change");
        let opens_anyway = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&log)
            .is_ok();
        let flushed = store.flush().await;
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644))
            .expect("the log's permissions change back");
        if opens_anyway {
            testkit::skip!("running as root: permissions don't stop opening a file");
        }
        assert!(
            matches!(flushed, Err(StoreError::Unavailable { .. })),
            "{flushed:?}"
        );
    }
}
