//! Startup recovery: ending what an earlier run left running, and running
//! again what it left queued (technical design §5, §9; decisions 40 and 73).

use std::sync::Arc;

use domain::Timestamp;
use domain::execution::{FailureReason, OperationKind, Target};
use domain::planning::AgentConfig;

use crate::ports::{OperationRecord, OperationStore, Recovered, StoreError};
use crate::schedule::{PlanRuns, ScheduleReport};

/// Why the persistent queue can't run a queued operation again at the start,
/// so that it ends as failed with [`FailureReason::DaemonStopped`]
/// (decision 73).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuedEnd {
    /// The configuration couldn't be read, so no backup's job can be built.
    ConfigUnreadable,
    /// The backup's plan is no longer configured.
    PlanGone,
    /// The backup's plan now backs up to another repository than the one the
    /// operation was queued for.
    RepositoryChanged,
    /// Not a backup: the log doesn't keep the job of any other kind.
    JobNotKept,
}

/// Ends every operation the log still shows as running, as failed with
/// [`FailureReason::DaemonStopped`], and hands back the queued ones. The
/// daemon runs it once at start, before it accepts a request: nothing is
/// running yet, so whatever the log calls running was left by a process
/// that stopped, and a running maintenance operation among it must stop
/// holding its repository (I7).
///
/// It ends running operations as failed, never as succeeded and never by
/// running them again: what an operation did before the process stopped is
/// unknown. It runs no hooks and sends no events, because nobody is
/// listening and the hooks belonged to the process that is gone. A queued
/// operation never reached the engine, so it is the persistent queue's (§5):
/// [`resume`](Self::resume) runs it again.
pub struct RecoverOperations {
    store: Arc<dyn OperationStore>,
}

impl RecoverOperations {
    pub fn new(store: Arc<dyn OperationStore>) -> Self {
        Self { store }
    }

    /// Ends the running operations, and returns how many there were and the
    /// queued ones, oldest first. An error means the log could not be
    /// changed and nothing was: the daemon refuses to start rather than
    /// serve with a repository held for good.
    pub async fn recover(&self) -> Result<Recovered, StoreError> {
        self.store.recover(&FailureReason::DaemonStopped).await
    }

    /// Runs again, oldest first, each queued backup of a plan `config` still
    /// has, on the repository it was queued for, under its own ID and
    /// trigger (decision 73). `config` is `None` when the configuration
    /// can't be read. A queued operation the queue can't run again ends as
    /// failed with [`FailureReason::DaemonStopped`] at `now`, as decision 40
    /// ended every queued one, and `runs` is told why ([`QueuedEnd`]). A
    /// write the log refuses is reported through `runs` and leaves that
    /// operation queued.
    pub async fn resume(
        &self,
        queued: Vec<OperationRecord>,
        config: Option<&AgentConfig>,
        runs: &dyn PlanRuns,
        now: Timestamp,
    ) {
        for record in queued {
            // Recovery hands back queued rows only; one that isn't is the
            // log's fault, said and left as it is rather than run or ended.
            let Some(mut operation) = record.queued_operation() else {
                runs.report(ScheduleReport::Store(StoreError::Corrupt {
                    detail: format!("operation {:?} came back as queued and isn't", record.id),
                }));
                continue;
            };
            let why = match (record.kind, config) {
                (OperationKind::Backup, Some(config)) => match ended_why(&record.target, config) {
                    None => {
                        runs.resume(operation).await;
                        continue;
                    }
                    Some(why) => why,
                },
                (OperationKind::Backup, None) => QueuedEnd::ConfigUnreadable,
                _ => QueuedEnd::JobNotKept,
            };
            runs.report(ScheduleReport::QueuedEnded {
                operation: record.id,
                kind: record.kind,
                why,
            });
            // A queued operation never ends otherwise: fail is allowed.
            if operation.fail(FailureReason::DaemonStopped, now).is_ok()
                && let Err(error) = self.store.save(&operation, None).await
            {
                runs.report(ScheduleReport::Store(error));
            }
        }
    }
}

/// Why a queued backup on `target` can't run again under `config`, or
/// `None` when it can: its plan is configured, and backs up to the
/// repository the backup was queued for.
fn ended_why(target: &Target, config: &AgentConfig) -> Option<QueuedEnd> {
    let Some(plan) = target.plan.as_ref().and_then(|plan| config.plan(plan)) else {
        return Some(QueuedEnd::PlanGone);
    };
    let repository = config
        .repository(&plan.repository)
        .and_then(|repository| repository.id.as_ref());
    (!target.is_on(repository)).then_some(QueuedEnd::RepositoryChanged)
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::{
        FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger,
    };
    use domain::planning::AgentConfig;
    use domain::{OperationId, PlanId, Timestamp, Uuid};

    use super::{QueuedEnd, RecoverOperations};
    use crate::fakes::InMemoryOperationStore;
    use crate::ports::{OperationRecord, OperationStore, StoreError};
    use crate::schedule::{PlanRuns, ScheduleReport};

    #[derive(Default)]
    struct Recording {
        resumed: Mutex<Vec<Operation>>,
        reports: Mutex<Vec<ScheduleReport>>,
    }

    #[async_trait]
    impl PlanRuns for Recording {
        async fn start(&self, _: &PlanId, _: Trigger, _: crate::queue::Place) {}

        async fn resume(&self, operation: Operation) {
            self.resumed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(operation);
        }

        fn report(&self, report: ScheduleReport) {
            self.reports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(report);
        }
    }

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + second).unwrap()
    }

    #[tokio::test]
    async fn a_record_that_is_not_queued_is_reported_and_left_as_it_is() {
        let id = OperationId::from_uuid(Uuid::from_u128(0x274));
        let mut running = Operation::queue(
            id,
            OperationKind::Prune,
            Trigger::Manual,
            Target {
                plan: None,
                repository: None,
            },
            at(0),
        );
        running.start(at(1)).unwrap();
        let store = Arc::new(InMemoryOperationStore::new());
        store.save(&running, None).await.unwrap();
        let runs = Recording::default();

        RecoverOperations::new(store.clone())
            .resume(
                vec![OperationRecord::of(&running, None)],
                Some(&AgentConfig::default()),
                &runs,
                at(10),
            )
            .await;

        let reports = runs
            .reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert!(
            matches!(
                reports.as_slice(),
                [ScheduleReport::Store(StoreError::Corrupt { .. })]
            ),
            "{reports:?}"
        );
        assert!(
            runs.resumed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        );
        let kept = store.get(id).await.unwrap().map(|detail| detail.record);
        assert_eq!(kept, Some(OperationRecord::of(&running, None)));
    }

    /// A queued operation of `kind` on `plan`, saved in `store`.
    async fn queued(
        store: &InMemoryOperationStore,
        n: u128,
        kind: OperationKind,
        plan: Option<&str>,
    ) -> Operation {
        let operation = Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(0x2740 + n)),
            kind,
            Trigger::Schedule,
            Target {
                plan: plan.map(|plan| PlanId::parse(plan).unwrap()),
                repository: None,
            },
            at(0),
        );
        store.save(&operation, None).await.unwrap();
        operation
    }

    /// What `runs` was told of the queued operations that ended.
    fn ended(runs: &Recording) -> Vec<ScheduleReport> {
        runs.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|report| matches!(report, ScheduleReport::QueuedEnded { .. }))
            .cloned()
            .collect()
    }

    async fn is_failed(store: &InMemoryOperationStore, operation: &Operation) -> bool {
        let status = store
            .get(operation.id())
            .await
            .unwrap()
            .unwrap()
            .record
            .status;
        status
            == OperationStatus::Failed {
                reason: FailureReason::DaemonStopped,
            }
    }

    #[tokio::test]
    async fn every_queued_operation_that_ends_says_why() {
        let store = Arc::new(InMemoryOperationStore::new());
        let backup = queued(&store, 1, OperationKind::Backup, Some("home")).await;
        let check = queued(&store, 2, OperationKind::Check, None).await;
        let runs = Recording::default();

        RecoverOperations::new(store.clone())
            .resume(
                vec![
                    OperationRecord::of(&backup, None),
                    OperationRecord::of(&check, None),
                ],
                Some(&AgentConfig::default()),
                &runs,
                at(10),
            )
            .await;

        assert_eq!(
            ended(&runs),
            vec![
                ScheduleReport::QueuedEnded {
                    operation: backup.id(),
                    kind: OperationKind::Backup,
                    why: QueuedEnd::PlanGone,
                },
                ScheduleReport::QueuedEnded {
                    operation: check.id(),
                    kind: OperationKind::Check,
                    why: QueuedEnd::JobNotKept,
                },
            ]
        );
        assert!(is_failed(&store, &backup).await);
        assert!(is_failed(&store, &check).await);
    }

    #[tokio::test]
    async fn a_configuration_that_cannot_be_read_ends_every_queued_operation() {
        let store = Arc::new(InMemoryOperationStore::new());
        let backup = queued(&store, 3, OperationKind::Backup, Some("home")).await;
        let prune = queued(&store, 4, OperationKind::Prune, None).await;
        let runs = Recording::default();

        RecoverOperations::new(store.clone())
            .resume(
                vec![
                    OperationRecord::of(&backup, None),
                    OperationRecord::of(&prune, None),
                ],
                None,
                &runs,
                at(10),
            )
            .await;

        assert_eq!(
            ended(&runs),
            vec![
                ScheduleReport::QueuedEnded {
                    operation: backup.id(),
                    kind: OperationKind::Backup,
                    why: QueuedEnd::ConfigUnreadable,
                },
                ScheduleReport::QueuedEnded {
                    operation: prune.id(),
                    kind: OperationKind::Prune,
                    why: QueuedEnd::JobNotKept,
                },
            ]
        );
        assert!(
            runs.resumed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
        );
        assert!(is_failed(&store, &backup).await);
        assert!(is_failed(&store, &prune).await);
    }
}
