//! The admin's sessions (technical design §10.1, §9 `sessions`; decision 70):
//! the `SessionStore` port on the store's own file.

use app::ports::{SessionStore, StoreError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::access::{KeptSession, SessionDigest, SessionLength};
use sqlx::sqlite::SqliteRow;
use sqlx::{Connection, Row};

use crate::{SqliteStore, row, sql, unavailable};

#[async_trait]
impl SessionStore for SqliteStore {
    /// One transaction, so a crash keeps the new session and forgets the
    /// others, or does neither.
    async fn add(&self, session: &KeptSession, forget: &[SessionDigest]) -> Result<(), StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        for digest in forget {
            sqlx::query(sql::DELETE_SESSION)
                .bind(digest.as_bytes().as_slice())
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        sqlx::query(sql::INSERT_SESSION)
            .bind(session.digest.as_bytes().as_slice())
            .bind(session.password.value().cast_signed())
            .bind(session.started.as_millisecond())
            .bind(session.last_used.as_millisecond())
            .bind(i64::from(session.keep_signed_in))
            .bind(i64::from(session.secure))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)
    }

    async fn all(&self) -> Result<Vec<KeptSession>, StoreError> {
        let _call = self.call().await;
        let rows = sqlx::query(sql::ALL_SESSIONS)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        rows.iter().map(row::kept_session).collect()
    }

    /// Reads and deletes in one transaction, the write lock taken up front,
    /// so a session kept meanwhile is never taken for an unreadable one.
    async fn forget_unreadable(&self) -> Result<usize, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let rows = sqlx::query(sql::ALL_SESSIONS)
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?;
        let unreadable: Vec<i64> = rows
            .iter()
            .filter(|row| row::kept_session(row).is_err())
            .map(rowid)
            .collect::<Result<_, _>>()?;
        for id in &unreadable {
            sqlx::query(sql::DELETE_SESSION_ROW)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(unreadable.len())
    }

    async fn remove(&self, digests: &[SessionDigest]) -> Result<bool, StoreError> {
        if digests.is_empty() {
            return Ok(false);
        }
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn.begin().await.map_err(unavailable)?;
        let mut forgot = false;
        for digest in digests {
            let done = sqlx::query(sql::DELETE_SESSION)
                .bind(digest.as_bytes().as_slice())
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
            forgot |= done.rows_affected() > 0;
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(forgot)
    }

    async fn remove_all(&self) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::DELETE_ALL_SESSIONS)
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }

    async fn record_use(&self, digest: &SessionDigest, at: Timestamp) -> Result<(), StoreError> {
        let _call = self.call().await;
        sqlx::query(sql::RECORD_SESSION_USE)
            .bind(at.as_millisecond())
            .bind(digest.as_bytes().as_slice())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(())
    }

    async fn length(&self) -> Result<Option<SessionLength>, StoreError> {
        let _call = self.call().await;
        let found = sqlx::query(sql::SESSION_LENGTH)
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        found.as_ref().map(row::session_length).transpose()
    }

    async fn set_length(&self, length: SessionLength) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let done = sqlx::query(sql::SET_SESSION_LENGTH)
            .bind(length.as_str())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(done.rows_affected() > 0)
    }
}

/// The `rowid` of a row of `sql::ALL_SESSIONS`, which SQLite always sets.
fn rowid(row: &SqliteRow) -> Result<i64, StoreError> {
    row.try_get::<i64, _>("rowid")
        .map_err(|error| StoreError::Corrupt {
            detail: format!("a session: column rowid: {error}"),
        })
}
