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
    length: Mutex<Option<SessionLength>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Vec<KeptSession>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `at`, cut to the millisecond.
fn to_the_millisecond(at: Timestamp) -> Timestamp {
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn add(&self, session: &KeptSession, forget: &[SessionDigest]) -> Result<(), StoreError> {
        let mut kept = self.kept();
        kept.retain(|other| other.digest != session.digest && !forget.contains(&other.digest));
        kept.push(KeptSession {
            started: to_the_millisecond(session.started),
            last_used: to_the_millisecond(session.last_used),
            ..session.clone()
        });
        Ok(())
    }

    async fn all(&self) -> Result<Vec<KeptSession>, StoreError> {
        Ok(self.kept().clone())
    }

    /// It keeps only what it was given, which it can always read.
    async fn forget_unreadable(&self) -> Result<usize, StoreError> {
        Ok(0)
    }

    async fn remove(&self, digests: &[SessionDigest]) -> Result<(), StoreError> {
        self.kept()
            .retain(|session| !digests.contains(&session.digest));
        Ok(())
    }

    async fn remove_all(&self) -> Result<(), StoreError> {
        self.kept().clear();
        Ok(())
    }

    async fn record_use(&self, digest: &SessionDigest, at: Timestamp) -> Result<(), StoreError> {
        if let Some(session) = self
            .kept()
            .iter_mut()
            .find(|session| session.digest == *digest)
        {
            session.last_used = to_the_millisecond(at);
        }
        Ok(())
    }

    async fn length(&self) -> Result<Option<SessionLength>, StoreError> {
        Ok(*self.length.lock().unwrap_or_else(PoisonError::into_inner))
    }

    async fn set_length(&self, length: SessionLength) -> Result<(), StoreError> {
        *self.length.lock().unwrap_or_else(PoisonError::into_inner) = Some(length);
        Ok(())
    }
}
