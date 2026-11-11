//! The `Retention` port: which of a plan's snapshots its policy lets go,
//! evaluated with rustic's own retention rules (technical design §3.2).
//! Ladderback decides *which* snapshots a policy applies to — the partition
//! into managed, orphaned and unmanaged groups is I6, and it lives in the
//! domain — and this port applies the policy to one managed group.

use async_trait::async_trait;
use domain::SnapshotId;
use domain::maintenance::Snapshot;
use domain::planning::RetentionPolicy;

use super::RepositoryAccess;

/// Why a retention evaluation failed.
#[derive(Debug, PartialEq, Eq)]
pub enum RetentionError {
    /// The repository couldn't be opened or read. `detail` is the engine's
    /// own message.
    Engine { detail: String },
    /// The policy isn't one rustic's retention accepts.
    InvalidPolicy,
}

/// Applies a plan's retention policy to the plan's own snapshots, and names
/// the ones the policy lets go. The real adapter is rustic's own
/// `ForgetGroups`, in-process (§3.2); a fake serves the acceptance runner.
/// A calendar rule (`keep-daily` and the rest) counts each snapshot on the
/// calendar of its own snapshot time, the offset the clock that took it
/// recorded, never this machine's zone (§16, decision 56).
#[async_trait]
pub trait Retention: Send + Sync {
    /// Which of `group`'s snapshots `policy` removes. `group` is one
    /// managed group's snapshots, newest first, exactly as the partition
    /// built it.
    async fn removes(
        &self,
        repository: &RepositoryAccess,
        policy: &RetentionPolicy,
        group: &[Snapshot],
    ) -> Result<Vec<SnapshotId>, RetentionError>;
}
