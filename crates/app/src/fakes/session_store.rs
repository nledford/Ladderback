use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;
use domain::access::{KeptSession, SessionDigest, SessionLength};

use crate::ports::{SessionStore, StoreError};

/// The admin's sessions, kept in memory, for use-case tests and acceptance
/// scenarios. It keeps times to the millisecond, as the real store does.
#[derive(Default)]
pub struct InMemorySessionStore {
    kept: Mutex<Vec<KeptSession>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Vec<KeptSession>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn add(
        &self,
        session: &KeptSession,
        forget: &[SessionDigest],
    ) -> Result<(), StoreError> {
        let _ = (session, forget, self.kept());
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
