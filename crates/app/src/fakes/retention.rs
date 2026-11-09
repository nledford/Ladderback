//! A `Retention` fake for the acceptance runner: it keeps the newest
//! `keep-last` snapshots and lets the rest go, and refuses a policy that
//! uses anything else (the shared contract suite checks it against the real
//! adapter, which is rustic's own retention).

use async_trait::async_trait;
use domain::SnapshotId;
use domain::maintenance::Snapshot;
use domain::planning::RetentionPolicy;

use crate::ports::{RepositoryAccess, Retention, RetentionError};

/// Evaluates `keep-last` and `keep-within` only, newest first, exactly as
/// the port's contract says a group arrives.
#[derive(Default)]
pub struct FakeRetention;

#[async_trait]
impl Retention for FakeRetention {
    async fn removes(
        &self,
        _repository: &RepositoryAccess,
        policy: &RetentionPolicy,
        group: &[Snapshot],
    ) -> Result<Vec<SnapshotId>, RetentionError> {
        let keep_last = policy.keep_last;
        if policy
            != &(RetentionPolicy {
                keep_last,
                ..RetentionPolicy::default()
            })
        {
            return Err(RetentionError::InvalidPolicy);
        }
        let Some(keep_last) = keep_last else {
            return Ok(Vec::new());
        };
        if keep_last.is_all() {
            return Ok(Vec::new());
        }
        let keep = usize::try_from(keep_last.get()).unwrap_or(0);
        // The group is newest first: everything past the first `keep` goes.
        Ok(group
            .iter()
            .skip(keep)
            .map(|snapshot| snapshot.id.clone())
            .collect())
    }
}
