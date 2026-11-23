//! The hub's registry (technical design §7.1, §9 `agents` and
//! `pairing_tokens`; decision 120, "Pairing" and "Pending approval"): the
//! `AgentRegistry` port on the store's own file, built only with the hub
//! (decision 122).

use app::ports::{Added, AgentRegistry, StoreError};
use async_trait::async_trait;
use domain::fleet::{
    AgentRecord, AgentStatus, ApprovalRefused, BindingKind, Fingerprint, HostnameKey, KEY_BYTES,
    MachineCheck, MachineFingerprint, MachineZone, PairRefusal, PairRequest, Pairing, PairingView,
    ProofRefusal, ProofRequest, ProofView, Proved, PublicKey, SecondMachine, TokenId, TokenRecord,
    TokenUse, admits_unused_token, decide_approval, decide_pairing, decide_proof,
    voids_tokens_for_new_key,
};
use domain::{AgentId, Timestamp};
use sqlx::{Connection, SqliteConnection};

use crate::sql::hub as sql;
use crate::{SqliteStore, row, unavailable};

/// `at` cut to the millisecond, as the columns keep it, so a record
/// answered is the record read back.
fn to_the_millisecond(at: Timestamp) -> Timestamp {
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
}

/// A token's state as the `state` column keeps it, and the agent it was
/// spent on.
fn state_columns(state: TokenUse) -> (&'static str, Option<String>, Option<PublicKey>) {
    match state {
        TokenUse::Unused => ("unused", None, None),
        TokenUse::Used {
            agent_id,
            agent_key,
        } => ("used", Some(row::agent_id_text(agent_id)), Some(agent_key)),
        TokenUse::Revoked => ("revoked", None, None),
    }
}

/// An agent's status as the `status` and `status_at` columns keep it.
const fn status_columns(status: AgentStatus) -> (&'static str, Timestamp) {
    match status {
        AgentStatus::Pending { since } => ("pending", since),
        AgentStatus::Approved { at } => ("approved", at),
    }
}

async fn count(tx: &mut SqliteConnection, query: &'static str) -> Result<usize, StoreError> {
    let counted: i64 = sqlx::query_scalar(query)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
    Ok(usize::try_from(counted).unwrap_or(usize::MAX))
}

async fn token(
    tx: &mut SqliteConnection,
    pairing_key: &PublicKey,
) -> Result<Option<TokenRecord>, StoreError> {
    let found = sqlx::query(sql::TOKEN)
        .bind(pairing_key.as_bytes().as_slice())
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?;
    found.as_ref().map(row::pairing_token).transpose()
}

async fn agent(
    tx: &mut SqliteConnection,
    query: &'static str,
    by: String,
) -> Result<Option<AgentRecord>, StoreError> {
    let found = sqlx::query(query)
        .bind(by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?;
    found.as_ref().map(row::agent).transpose()
}

async fn unused_tokens(
    conn: &mut SqliteConnection,
    now: Timestamp,
) -> Result<Vec<TokenRecord>, StoreError> {
    let rows = sqlx::query(sql::UNUSED_TOKENS)
        .bind(now.as_millisecond())
        .fetch_all(&mut *conn)
        .await
        .map_err(unavailable)?;
    rows.iter().map(row::pairing_token).collect()
}

/// Writes `agent` as a new row (`INSERT_AGENT`) or over the one kept under
/// its ID (`UPDATE_AGENT`).
async fn write_agent(
    tx: &mut SqliteConnection,
    agent: &AgentRecord,
    new: bool,
) -> Result<(), StoreError> {
    let (status, status_at) = status_columns(agent.status);
    let id = row::agent_id_text(agent.agent_id);
    let key = agent.key.as_bytes().as_slice();
    let hostname_key = HostnameKey::of(&agent.hostname);
    let query = if new {
        sqlx::query(sql::INSERT_AGENT).bind(id.as_str()).bind(key)
    } else {
        sqlx::query(sql::UPDATE_AGENT).bind(key)
    };
    let query = query
        .bind(agent.hostname.as_str())
        .bind(hostname_key.as_str())
        .bind(status)
        .bind(status_at.as_millisecond())
        .bind(row::counter_text(agent.counter))
        .bind(agent.last_seen.as_millisecond())
        .bind(agent.zone.as_ref().map(MachineZone::as_str));
    let query = if new { query } else { query.bind(id.as_str()) };
    query.execute(&mut *tx).await.map_err(unavailable)?;
    Ok(())
}

async fn spend(tx: &mut SqliteConnection, request: &PairRequest) -> Result<(), StoreError> {
    sqlx::query(sql::SPEND_TOKEN)
        .bind(row::agent_id_text(request.agent_id))
        .bind(request.agent_key.as_bytes().as_slice())
        .bind(request.pairing_key.as_bytes().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
    Ok(())
}

#[async_trait]
impl AgentRegistry for SqliteStore {
    /// One transaction, the write lock taken up front, so the count and the
    /// insert see the same tokens: of adds racing for the last place, one
    /// is kept.
    async fn add_token(
        &self,
        token: &TokenRecord,
        at_most: usize,
        now: Timestamp,
    ) -> Result<Added, StoreError> {
        let (state, agent_id, agent_key) = state_columns(token.state);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let unused: i64 = sqlx::query_scalar(sql::COUNT_UNUSED_TOKENS)
            .bind(now.as_millisecond())
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
        if admits_unused_token(usize::try_from(unused).unwrap_or(usize::MAX), at_most).is_err() {
            tx.rollback().await.map_err(unavailable)?;
            return Ok(Added::Full);
        }
        sqlx::query(sql::INSERT_TOKEN)
            .bind(token.pairing_key.as_bytes().as_slice())
            .bind(token.hub_key.as_bytes().as_slice())
            .bind(i64::from(token.kind.byte()))
            .bind(token.made_at.as_millisecond())
            .bind(token.expires_at.as_millisecond())
            .bind(state)
            .bind(agent_id)
            .bind(agent_key.as_ref().map(|key| key.as_bytes().as_slice()))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(Added::Kept)
    }

    async fn unused_tokens(&self, now: Timestamp) -> Result<Vec<TokenRecord>, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        unused_tokens(&mut conn, now).await
    }

    /// One transaction, the write lock taken up front: a token ID is worked
    /// out from the pairing key, which SQL can't, so the unused tokens are
    /// read and the ones with `id` revoked in the same step.
    async fn revoke_token(&self, id: TokenId, now: Timestamp) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let mut revoked = false;
        for token in unused_tokens(&mut tx, now).await? {
            if token.id() == id {
                sqlx::query(sql::REVOKE_TOKEN)
                    .bind(token.pairing_key.as_bytes().as_slice())
                    .execute(&mut *tx)
                    .await
                    .map_err(unavailable)?;
                revoked = true;
            }
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(revoked)
    }

    /// One transaction, the write lock taken up front, so no pairing
    /// registers an agent between the count and the revoking.
    async fn void_tokens_for_new_key(&self) -> Result<bool, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        if !voids_tokens_for_new_key(count(&mut tx, sql::COUNT_AGENTS).await?) {
            tx.rollback().await.map_err(unavailable)?;
            return Ok(false);
        }
        sqlx::query(sql::REVOKE_UNUSED_TOKENS)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(true)
    }

    async fn tls_key(&self) -> Result<Option<Fingerprint>, StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let recorded: Option<Vec<u8>> = sqlx::query_scalar(sql::TLS_KEY)
            .fetch_optional(&mut *conn)
            .await
            .map_err(unavailable)?;
        recorded
            .map(|bytes| {
                <[u8; KEY_BYTES]>::try_from(bytes)
                    .map(Fingerprint::from_digest)
                    .map_err(|bytes| StoreError::Corrupt {
                        detail: format!(
                            "hub_tls_key.fingerprint holds {} bytes, not {KEY_BYTES}",
                            bytes.len()
                        ),
                    })
            })
            .transpose()
    }

    async fn record_tls_key(&self, fingerprint: Fingerprint) -> Result<(), StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        sqlx::query(sql::RECORD_TLS_KEY)
            .bind(fingerprint.digest().as_slice())
            .execute(&mut *conn)
            .await
            .map_err(unavailable)?;
        Ok(())
    }

    /// One transaction, the write lock taken up front, so the records the
    /// decision reads are the ones its changes are written over: of two
    /// pairings on one token, the second sees the token spent.
    async fn pair(
        &self,
        request: &PairRequest,
        now: Timestamp,
    ) -> Result<Result<Pairing, PairRefusal>, StoreError> {
        let now = to_the_millisecond(now);
        let hostname_key = HostnameKey::of(&request.hostname);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let token = token(&mut tx, &request.pairing_key).await?;
        let by_id = agent(&mut tx, sql::AGENT, row::agent_id_text(request.agent_id)).await?;
        let by_hostname = agent(
            &mut tx,
            sql::AGENT_BY_HOSTNAME,
            hostname_key.as_str().to_owned(),
        )
        .await?;
        let pending = count(&mut tx, sql::COUNT_PENDING_AGENTS).await?;
        let view = PairingView {
            token: token.as_ref(),
            by_id: by_id.as_ref(),
            by_hostname: by_hostname.as_ref(),
            pending,
        };
        let decided = decide_pairing(&view, request, now);
        match &decided {
            Ok(Pairing::New(agent)) => {
                write_agent(&mut tx, agent, true).await?;
                spend(&mut tx, request).await?;
            }
            Ok(Pairing::Again(agent)) => {
                write_agent(&mut tx, agent, false).await?;
                spend(&mut tx, request).await?;
            }
            Ok(Pairing::Repeated(_)) | Err(_) => {
                tx.rollback().await.map_err(unavailable)?;
                return Ok(decided);
            }
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(decided)
    }

    async fn agents(&self) -> Result<Vec<AgentRecord>, StoreError> {
        let _call = self.call().await;
        let rows = sqlx::query(sql::ALL_AGENTS)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        rows.iter().map(row::agent).collect()
    }

    async fn agent(&self, agent_id: AgentId) -> Result<Option<AgentRecord>, StoreError> {
        let _call = self.call().await;
        let found = sqlx::query(sql::AGENT)
            .bind(row::agent_id_text(agent_id))
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        found.as_ref().map(row::agent).transpose()
    }

    /// One transaction, the write lock taken up front, so the record the
    /// decision reads is the one its change is written over: of two proofs
    /// at once, the second sees the first's counter.
    async fn take_proof(
        &self,
        request: &ProofRequest,
        now: Timestamp,
    ) -> Result<Result<Proved, ProofRefusal>, StoreError> {
        let now = to_the_millisecond(now);
        let id = row::agent_id_text(request.agent_id);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let record = agent(&mut tx, sql::AGENT, id.clone()).await?;
        let spent = sqlx::query(sql::SPENT_TOKENS)
            .bind(id)
            .bind(request.agent_key.as_bytes().as_slice())
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?;
        let kinds = spent
            .iter()
            .map(|found| row::pairing_token(found).map(|token| token.kind))
            .collect::<Result<Vec<BindingKind>, StoreError>>()?;
        let view = ProofView {
            record: record.as_ref(),
            kinds: &kinds,
        };
        let decided = decide_proof(&view, request, now);
        match &decided {
            Ok(proved) => {
                write_agent(&mut tx, &proved.agent, false).await?;
                tx.commit().await.map_err(unavailable)?;
            }
            Err(_) => tx.rollback().await.map_err(unavailable)?,
        }
        Ok(decided)
    }

    async fn record_zone(
        &self,
        agent_id: AgentId,
        zone: Option<&MachineZone>,
    ) -> Result<(), StoreError> {
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        sqlx::query(sql::RECORD_ZONE)
            .bind(zone.map(MachineZone::as_str))
            .bind(row::agent_id_text(agent_id))
            .execute(&mut *conn)
            .await
            .map_err(unavailable)?;
        Ok(())
    }

    async fn check_machine(
        &self,
        agent_id: AgentId,
        presented: Option<&MachineFingerprint>,
        now: Timestamp,
    ) -> Result<Option<MachineCheck>, StoreError> {
        let _ = (agent_id, presented, now);
        todo!()
    }

    async fn second_machines(&self) -> Result<Vec<SecondMachine>, StoreError> {
        todo!()
    }

    async fn forget_machine(&self, agent_id: AgentId) -> Result<bool, StoreError> {
        let _ = agent_id;
        todo!()
    }

    /// One transaction, the write lock taken up front, so the record the
    /// decision reads is the one its change is written over: of two
    /// approvals at once, the second sees the agent approved.
    async fn approve(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
        now: Timestamp,
    ) -> Result<Result<AgentRecord, ApprovalRefused>, StoreError> {
        let now = to_the_millisecond(now);
        let _call = self.call().await;
        let mut conn = self.pool.acquire().await.map_err(unavailable)?;
        let mut tx = conn
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let record = agent(&mut tx, sql::AGENT, row::agent_id_text(agent_id)).await?;
        let decided = decide_approval(record.as_ref(), &agent_key, now);
        match &decided {
            Ok(agent) => {
                write_agent(&mut tx, agent, false).await?;
                tx.commit().await.map_err(unavailable)?;
            }
            Err(_) => tx.rollback().await.map_err(unavailable)?,
        }
        Ok(decided)
    }
}
