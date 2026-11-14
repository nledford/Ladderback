use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use domain::access::api_token::{ApiToken, ApiTokenId};

use crate::ports::{Added, ApiTokenStore, StoreError};

/// The API tokens, kept in memory, oldest first, for use-case tests and
/// acceptance scenarios. It keeps times to the millisecond, as the real
/// store does.
#[derive(Default)]
pub struct InMemoryApiTokenStore {
    kept: Mutex<Vec<ApiToken>>,
}

impl InMemoryApiTokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Vec<ApiToken>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `at` cut to the millisecond, as the real store keeps it.
fn to_the_millisecond(at: Timestamp) -> Timestamp {
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
}

#[async_trait]
impl ApiTokenStore for InMemoryApiTokenStore {
    async fn add(&self, token: &ApiToken, at_most: usize) -> Result<Added, StoreError> {
        let mut kept = self.kept();
        if kept.len() >= at_most {
            return Ok(Added::Full);
        }
        if kept.iter().any(|other| other.id == token.id) {
            return Err(StoreError::Unavailable {
                detail: format!("API token {} is kept already", token.id.as_str()),
            });
        }
        let mut token = token.clone();
        token.created_at = to_the_millisecond(token.created_at);
        token.last_used_at = token.last_used_at.map(to_the_millisecond);
        kept.push(token);
        Ok(Added::Kept)
    }

    async fn get(&self, id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        Ok(self.kept().iter().find(|token| &token.id == id).cloned())
    }

    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        let mut all = self.kept().clone();
        all.sort_by_key(|token| token.created_at);
        Ok(all)
    }

    async fn revoke(&self, id: &ApiTokenId) -> Result<bool, StoreError> {
        let mut kept = self.kept();
        let before = kept.len();
        kept.retain(|token| &token.id != id);
        Ok(kept.len() < before)
    }

    async fn revoke_all_but(&self, password: PasswordFingerprint) -> Result<(), StoreError> {
        self.kept().retain(|token| token.password == password);
        Ok(())
    }

    async fn record_use(&self, id: &ApiTokenId, at: Timestamp) -> Result<(), StoreError> {
        if let Some(token) = self.kept().iter_mut().find(|token| &token.id == id) {
            token.last_used_at = Some(to_the_millisecond(at));
        }
        Ok(())
    }
}
