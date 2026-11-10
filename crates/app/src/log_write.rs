//! Writes to the operation log that cannot hold a run (§9).

use std::pin::pin;

use domain::execution::Operation;
use domain::{SignedDuration, Timestamp};
use futures::future::{Either, select};

use crate::ports::{Clock, EngineResult, OperationStore, StoreError};

/// How long a write to the operation log may take before the run goes on
/// without it: past the store's own bound (about 15 s), so only a store
/// that is stalled, not a busy one, is given up on.
pub const SAVE_BOUND: SignedDuration = SignedDuration::from_secs(20);

/// Writes `operation` to `store`, giving up after [`SAVE_BOUND`] on `clock`:
/// a write that stalls is reported as the store being unavailable, like one
/// the store refused, and the caller goes on (backups never depend on the
/// database, §9). The write is dropped where it stands; the next one is the
/// whole operation again.
pub(crate) async fn save_within_bound(
    clock: &dyn Clock,
    store: &dyn OperationStore,
    operation: &Operation,
    result: Option<&EngineResult>,
) -> Result<(), StoreError> {
    let give_up_at = clock
        .now()
        .checked_add(SAVE_BOUND)
        .unwrap_or(Timestamp::MAX);
    let save = pin!(store.save(operation, result));
    let give_up = pin!(clock.sleep_until(give_up_at));
    match select(save, give_up).await {
        Either::Left((saved, _)) => saved,
        Either::Right(((), _)) => Err(StoreError::Unavailable {
            detail: format!(
                "the operation log did not answer within {} s",
                SAVE_BOUND.as_secs()
            ),
        }),
    }
}
