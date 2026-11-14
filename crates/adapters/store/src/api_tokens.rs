//! The API tokens (technical design §10.1, §9 `api_tokens`; decision 93):
//! the `ApiTokenStore` port on the store's own file.

use app::ports::{Added, ApiTokenStore, StoreError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use domain::access::api_token::{ApiToken, ApiTokenId, Scope};
use sqlx::Connection;

use crate::{SqliteStore, row, sql, unavailable};

/// A token's scopes as the `scopes` column keeps them: their names, in order,
/// joined by commas.
fn scopes_text(token: &ApiToken) -> String {
        todo!()
    }

#[async_trait]
impl ApiTokenStore for SqliteStore {
    /// One transaction, the write lock taken up front, so the count and the
    /// insert see the same tokens: of adds racing for the last place, one is
    /// kept.
    async fn add(&self, token: &ApiToken, at_most: usize) -> Result<Added, StoreError> {
        todo!()
    }

    async fn get(&self, id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        todo!()
    }

    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        todo!()
    }

    async fn revoke(&self, id: &ApiTokenId) -> Result<bool, StoreError> {
        todo!()
    }

    async fn revoke_all_but(&self, password: PasswordFingerprint) -> Result<(), StoreError> {
        todo!()
    }

    async fn record_use(&self, id: &ApiTokenId, at: Timestamp) -> Result<(), StoreError> {
        todo!()
    }
}
