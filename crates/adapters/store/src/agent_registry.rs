//! The hub's registry (technical design §7.1, §9 `agents` and
//! `pairing_tokens`; decision 120, "Pairing" and "Pending approval"): the
//! `AgentRegistry` port on the store's own file, built only with the hub
//! (decision 122).

use app::ports::{Added, AgentRegistry, StoreError};
use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{AgentRecord, PairRefusal, PairRequest, Pairing, TokenId, TokenRecord};

use crate::SqliteStore;

#[async_trait]
impl AgentRegistry for SqliteStore {
    async fn add_token(
        &self,
        _token: &TokenRecord,
        _at_most: usize,
        _now: Timestamp,
    ) -> Result<Added, StoreError> {
        todo!()
    }

    async fn unused_tokens(&self, _now: Timestamp) -> Result<Vec<TokenRecord>, StoreError> {
        todo!()
    }

    async fn revoke_token(&self, _id: TokenId, _now: Timestamp) -> Result<bool, StoreError> {
        todo!()
    }

    async fn void_tokens_for_new_key(&self) -> Result<bool, StoreError> {
        todo!()
    }

    async fn pair(
        &self,
        _request: &PairRequest,
        _now: Timestamp,
    ) -> Result<Result<Pairing, PairRefusal>, StoreError> {
        todo!()
    }

    async fn agents(&self) -> Result<Vec<AgentRecord>, StoreError> {
        todo!()
    }
}
