//! Startup recovery: ending what an earlier run left unfinished (technical
//! design §9).

use std::sync::Arc;

use domain::execution::FailureReason;

use crate::ports::{OperationStore, StoreError};

/// Ends every operation the log still shows as queued or running, as failed
/// with [`FailureReason::DaemonStopped`]. The daemon runs it once at start,
/// before it accepts a request: nothing is running yet, so whatever the log
/// calls unfinished was left by a process that stopped, and a running
/// maintenance operation among it must stop holding its repository (I7).
///
/// It ends operations as failed, never as succeeded and never by running them
/// again: what an operation did before the process stopped is unknown. It runs
/// no hooks and sends no events, because nobody is listening and the hooks
/// belonged to the process that is gone.
pub struct RecoverOperations {
    store: Arc<dyn OperationStore>,
}

impl RecoverOperations {
    pub fn new(store: Arc<dyn OperationStore>) -> Self {
        Self { store }
    }

    /// Ends the unfinished operations and returns how many there were. An
    /// error means the log could not be changed and nothing was: the daemon
    /// refuses to start rather than serve with a repository held for good.
    pub async fn recover(&self) -> Result<u64, StoreError> {
        let _ = (&self.store, FailureReason::DaemonStopped);
        todo!("RecoverOperations::recover")
    }
}
