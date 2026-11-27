//! The flush of the drive's own cache (technical design §7.3, I1; §16,
//! decision 178). Each commit reaches the disk before it returns
//! (decision 164), but on macOS `fsync` stops at the drive's own cache,
//! which a power cut can lose. A commit the other side acts on at once,
//! the hub's new grant and a machine's release, is flushed past it first.

use async_trait::async_trait;

use super::StoreError;

/// The drive the database is on.
#[async_trait]
pub trait DriveFlush: Send + Sync {
    /// Puts every commit made so far on the drive's permanent storage, past
    /// its own cache: `F_FULLFSYNC` on macOS, and nothing where the commit's
    /// own sync already does it. An error means the commits may still be in
    /// the cache, so what depends on them must not go out.
    async fn flush(&self) -> Result<(), StoreError>;
}
