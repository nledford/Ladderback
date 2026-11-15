//! The admin's sessions (technical design §10.1, §9 `sessions`; decision 70):
//! the `SessionStore` port on the store's own file.

use app::ports::{SessionStore, StoreError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::access::{KeptSession, SessionDigest, SessionLength};

use crate::SqliteStore;

#[async_trait]
impl SessionStore for SqliteStore {
    async fn add(
        &self,
        session: &KeptSession,
        forget: &[SessionDigest],
    ) -> Result<(), StoreError> {
        let _ = (session, forget);
        todo!()
    }

    async fn all(&self) -> Result<Vec<KeptSession>, StoreError> {
        todo!()
    }

    async fn forget_unreadable(&self) -> Result<usize, StoreError> {
        todo!()
    }

    async fn remove(&self, digests: &[SessionDigest]) -> Result<(), StoreError> {
        let _ = digests;
        todo!()
    }

    async fn remove_all(&self) -> Result<(), StoreError> {
        todo!()
    }

    async fn record_use(&self, digest: &SessionDigest, at: Timestamp) -> Result<(), StoreError> {
        let _ = (digest, at);
        todo!()
    }

    async fn length(&self) -> Result<Option<SessionLength>, StoreError> {
        todo!()
    }

    async fn set_length(&self, length: SessionLength) -> Result<(), StoreError> {
        let _ = length;
        todo!()
    }
}
