//! The API tokens (technical design §10.1, §9 `api_tokens`; decision 93):
//! the `ApiTokenStore` port on the store's own file.

use app::ports::{Added, ApiTokenStore, PermissionsSet, StoreError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use domain::access::api_token::{ApiToken, ApiTokenId, TokenPermissions, admits_api_token};
use sqlx::Connection;

use crate::{SqliteStore, row, sql, unavailable};

/// A token's permissions as the `permissions` column keeps them: its rows
/// set to a level, as `row:level`, in order, joined by commas.
fn permissions_text(permissions: TokenPermissions) -> String {
    permissions
        .get()
        .rows()
        .map(|(row, level)| format!("{}:{}", row.as_str(), level.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}

#[async_trait]
impl ApiTokenStore for SqliteStore {
    /// One transaction, the write lock taken up front, so the count and the
    /// insert see the same tokens: of adds racing for the last place, one is
    /// kept.
    async fn add(&self, token: &ApiToken, at_most: usize) -> Result<Added, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let kept: i64 = sqlx::query_scalar(sql::COUNT_API_TOKENS)
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
        if admits_api_token(usize::try_from(kept).unwrap_or(usize::MAX), at_most).is_err() {
            tx.rollback().await.map_err(unavailable)?;
            return Ok(Added::Full);
        }
        sqlx::query(sql::INSERT_API_TOKEN)
            .bind(token.id.as_str())
            .bind(token.label.as_str())
            .bind(token.digest.as_bytes().as_slice())
            .bind(permissions_text(token.permissions))
            .bind(token.password.value().cast_signed())
            .bind(token.created_at.as_millisecond())
            .bind(token.last_used_at.map(Timestamp::as_millisecond))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(Added::Kept)
    }

    async fn get(&self, id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        let _call = self.call().await;
        let found = sqlx::query(sql::GET_API_TOKEN)
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        found.as_ref().map(row::api_token).transpose()
    }

    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        let _call = self.call().await;
        let rows = sqlx::query(sql::ALL_API_TOKENS)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        rows.iter().map(row::api_token).collect()
    }

    async fn revoke(&self, id: &ApiTokenId) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::DELETE_API_TOKEN)
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }

    async fn set_permissions(
        &self,
        id: &ApiTokenId,
        permissions: TokenPermissions,
    ) -> Result<PermissionsSet, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::SET_API_TOKEN_PERMISSIONS)
            .bind(permissions_text(permissions))
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(if done.rows_affected() > 0 {
            PermissionsSet::Changed
        } else {
            PermissionsSet::NotKept
        })
    }

    async fn revoke_all_but(&self, password: PasswordFingerprint) -> Result<(), StoreError> {
        let _call = self.call().await;
        sqlx::query(sql::DELETE_OTHER_PASSWORDS_TOKENS)
            .bind(password.value().cast_signed())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(())
    }

    async fn record_use(&self, id: &ApiTokenId, at: Timestamp) -> Result<(), StoreError> {
        let _call = self.call().await;
        sqlx::query(sql::RECORD_API_TOKEN_USE)
            .bind(at.as_millisecond())
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(())
    }
}
