use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use domain::OperationId;
use domain::execution::Operation;

use crate::ports::{
    EngineResult, HistoryPage, HistoryQuery, OperationDetail, OperationStore, StoreError,
};

/// An operation log that lives in memory, for use-case tests and acceptance
/// scenarios. It keeps what the real stores keep, and nothing else.
#[derive(Default)]
pub struct InMemoryOperationStore {
    stored: Mutex<BTreeMap<OperationId, OperationDetail>>,
}

impl InMemoryOperationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl OperationStore for InMemoryOperationStore {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        todo!()
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        todo!()
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        todo!()
    }
}
