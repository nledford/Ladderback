//! History use cases: reading the operation log (technical design §3.2).

use std::sync::Arc;

use domain::{OperationId, PlanId};

use crate::ports::{HistoryPage, OperationDetail, OperationStore, StoreError};

/// Lists operations, newest first, a page at a time.
pub struct ListOperations {
    store: Arc<dyn OperationStore>,
}

impl ListOperations {
    pub fn new(store: Arc<dyn OperationStore>) -> Self {
        Self { store }
    }

    /// One page of history. `plan` limits it to one plan's operations;
    /// `after` continues from a page's `next`. The size comes from the
    /// caller, so it is clamped: `None` asks for the default
    /// ([`DEFAULT_PAGE_SIZE`](crate::ports::DEFAULT_PAGE_SIZE)), and no page
    /// holds more than [`MAX_PAGE_SIZE`](crate::ports::MAX_PAGE_SIZE) or
    /// fewer than one.
    pub async fn list(
        &self,
        plan: Option<PlanId>,
        after: Option<OperationId>,
        limit: Option<usize>,
    ) -> Result<HistoryPage, StoreError> {
        todo!()
    }
}

/// Reads one operation: what happened, with its warnings.
pub struct GetOperation {
    store: Arc<dyn OperationStore>,
}

impl GetOperation {
    pub fn new(store: Arc<dyn OperationStore>) -> Self {
        Self { store }
    }

    /// The operation, or `None` when the log has none with this ID.
    pub async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        todo!()
    }
}
