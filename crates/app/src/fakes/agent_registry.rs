use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{AgentRecord, PairRefusal, PairRequest, Pairing, TokenId, TokenRecord};

use crate::ports::{Added, AgentRegistry, StoreError};

/// The hub's registry in memory, for use-case tests and acceptance
/// scenarios. It keeps times to the millisecond, as the real store does.
#[derive(Debug, Default)]
pub struct InMemoryAgentRegistry;

impl InMemoryAgentRegistry {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentRegistry for InMemoryAgentRegistry {
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
