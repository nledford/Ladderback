//! The backups this machine runs now, as its heartbeat tells its hub
//! (technical design §7.2, §7.3 layer 2; §16, decision 166): a run takes
//! its place here when its engine is asked to start and leaves it however
//! it ends, so the hub can hold another machine's prune of the repository
//! meanwhile. Kept in memory only: a daemon that stops runs nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use domain::maintenance::{MAX_RUNNING_BACKUPS, RunningBackup};
use domain::{OperationId, RepositoryId};

/// The backups the machine runs, each with the repository it backs up to
/// when its ID is known.
#[derive(Debug, Default)]
pub struct RunningBackups {
    kept: Mutex<BTreeMap<OperationId, Option<RepositoryId>>>,
}

/// A backup's place among the running ones, which it leaves when this is
/// dropped.
#[must_use = "a backup's place dropped at once leaves the running backups"]
#[derive(Debug)]
pub struct BackupRunning {
    op_id: OperationId,
    backups: Arc<RunningBackups>,
}

impl RunningBackups {
    pub fn new() -> Self {
        Self::default()
    }

    /// The backup `op_id` runs, backing up to `repository`, `None` while
    /// its ID isn't known, until the place returned is dropped.
    pub fn begin(
        self: &Arc<Self>,
        op_id: OperationId,
        repository: Option<RepositoryId>,
    ) -> BackupRunning {
        self.kept().insert(op_id, repository);
        BackupRunning {
            op_id,
            backups: Arc::clone(self),
        }
    }

    /// The backups running now, in the order of their IDs; `None` when
    /// there are more than a heartbeat names ([`MAX_RUNNING_BACKUPS`]),
    /// which the hub reads as "can't tell".
    pub fn listed(&self) -> Option<Vec<RunningBackup>> {
        let kept = self.kept();
        if kept.len() > MAX_RUNNING_BACKUPS {
            return None;
        }
        Some(
            kept.iter()
                .map(|(op_id, repository)| RunningBackup {
                    op_id: *op_id,
                    repository: repository.clone(),
                })
                .collect(),
        )
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, BTreeMap<OperationId, Option<RepositoryId>>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl BackupRunning {
    /// The engine opened `repository`: the backup is named with it from
    /// now on.
    pub fn opened(&self, repository: RepositoryId) {
        if let Some(kept) = self.backups.kept().get_mut(&self.op_id) {
            *kept = Some(repository);
        }
    }
}

impl Drop for BackupRunning {
    fn drop(&mut self) {
        self.backups.kept().remove(&self.op_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Uuid;

    fn op(n: u128) -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(n))
    }

    fn repository(byte: u8) -> RepositoryId {
        RepositoryId::parse(&format!("{byte:02x}").repeat(32)).unwrap()
    }

    #[test]
    fn a_backup_is_listed_from_its_start_until_its_place_is_dropped() {
        let backups = Arc::new(RunningBackups::new());
        assert_eq!(backups.listed(), Some(Vec::new()));
        let first = backups.begin(op(1), None);
        let second = backups.begin(op(2), Some(repository(2)));
        assert_eq!(
            backups.listed(),
            Some(vec![
                RunningBackup {
                    op_id: op(1),
                    repository: None
                },
                RunningBackup {
                    op_id: op(2),
                    repository: Some(repository(2))
                },
            ])
        );
        first.opened(repository(1));
        drop(second);
        assert_eq!(
            backups.listed(),
            Some(vec![RunningBackup {
                op_id: op(1),
                repository: Some(repository(1))
            }])
        );
        drop(first);
        assert_eq!(backups.listed(), Some(Vec::new()));
    }

    #[test]
    fn as_many_backups_as_a_heartbeat_names_are_listed_and_one_more_cant_be_told() {
        let backups = Arc::new(RunningBackups::new());
        let mut places: Vec<_> = (0..MAX_RUNNING_BACKUPS)
            .map(|n| backups.begin(op(n as u128), Some(repository(1))))
            .collect();
        assert_eq!(
            backups.listed().map(|listed| listed.len()),
            Some(MAX_RUNNING_BACKUPS)
        );
        places.push(backups.begin(op(MAX_RUNNING_BACKUPS as u128), Some(repository(1))));
        assert_eq!(backups.listed(), None);
        drop(places);
        assert_eq!(backups.listed(), Some(Vec::new()));
    }
}
