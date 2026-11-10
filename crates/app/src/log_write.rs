//! Writes to the operation log that cannot hold a run (§9).

use domain::SignedDuration;

/// How long a write to the operation log may take before the run goes on
/// without it: past the store's own bound (about 15 s), so only a store
/// that is stalled, not a busy one, is given up on.
pub const SAVE_BOUND: SignedDuration = SignedDuration::from_secs(20);
