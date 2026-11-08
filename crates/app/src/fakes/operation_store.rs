use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::OperationId;
use domain::execution::Operation;

use crate::ports::{
    EngineResult, HistoryPage, HistoryQuery, OperationDetail, OperationRecord, OperationStore,
    StoreError,
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

    // The map is only ever changed by whole inserts, so a panic elsewhere
    // can't leave it half-written.
    fn stored(&self) -> MutexGuard<'_, BTreeMap<OperationId, OperationDetail>> {
        self.stored.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl OperationStore for InMemoryOperationStore {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        let detail = OperationDetail {
            record: OperationRecord::of(operation, result),
            warnings: operation.warnings().to_vec(),
        };
        self.stored().insert(operation.id(), detail);
        Ok(())
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        Ok(self.stored().get(&id).cloned())
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        let stored = self.stored();
        let mut matching = stored
            .iter()
            .rev()
            .filter(|(id, _)| query.after.is_none_or(|after| **id < after))
            .map(|(_, detail)| &detail.record)
            .filter(|record| {
                query
                    .plan
                    .as_ref()
                    .is_none_or(|plan| record.target.plan.as_ref() == Some(plan))
            });
        let operations: Vec<OperationRecord> =
            matching.by_ref().take(query.limit.get()).cloned().collect();
        let more_remain = matching.next().is_some();
        let next = operations
            .last()
            .filter(|_| more_remain)
            .map(|last| last.id);
        Ok(HistoryPage { operations, next })
    }
}
