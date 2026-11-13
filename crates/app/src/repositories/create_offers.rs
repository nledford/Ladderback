//! Which saved repositories the Repositories page offers to create (decision
//! 53): the ones a job found missing and nothing on record has reached, read
//! from the log alone. Nothing here opens a repository or resolves a
//! password.

use std::collections::BTreeSet;
use std::sync::Arc;

use domain::RepositoryAlias;
use domain::planning::AgentConfig;

use crate::ports::{HistoryQuery, OperationRecord, OperationStore, PageSize, StoreError};

/// Decides, for each repository the configuration holds, whether its row
/// offers to create it: only when its entry records no repository ID, its
/// latest operation failed because no repository is at the location, and
/// none of its operations ever succeeded. An entry that records an ID names
/// a repository Ladderback has opened, so it was there. A repository with no
/// operation on record is not offered: a folder that isn't there, as on a
/// drive that isn't plugged in, never fails `repository-not-found`, so that
/// failure is the evidence that the location is reachable and holds none. A
/// repository a backup once reached and that is gone now is more likely on a
/// drive that isn't plugged in than never made, and creating one would put a
/// new, empty repository where the old one should be.
///
/// A repository's operations are those of the plans that back up to it, its
/// backups and adoptions; the latest is the newest by ID across those plans.
/// A connection test is not one: it writes no operation.
pub struct CreateOffers {
    operations: Arc<dyn OperationStore>,
}

impl CreateOffers {
    pub fn new(operations: Arc<dyn OperationStore>) -> Self {
        Self { operations }
    }

    /// The aliases of `config`'s repositories whose rows offer to create
    /// them, or the store's error when the log can't be read.
    pub async fn offered(
        &self,
        config: &AgentConfig,
    ) -> Result<BTreeSet<RepositoryAlias>, StoreError> {
        let mut offered = BTreeSet::new();
        for alias in config.repositories().keys() {
            if self.offers(config, alias).await? {
                offered.insert(alias.clone());
            }
        }
        Ok(offered)
    }

    /// Whether the row of the repository `config` holds under `alias` offers
    /// to create it, by the rule above: false for an alias `config` doesn't
    /// hold, or the store's error when the log can't be read. One row read
    /// per plan that backs up to it, none for an entry that records an ID.
    pub async fn offers(
        &self,
        config: &AgentConfig,
        alias: &RepositoryAlias,
    ) -> Result<bool, StoreError> {
        if config
            .repository(alias)
            .is_none_or(|entry| entry.id.is_some())
        {
            return Ok(false);
        }
        // The newest operation of the repository, from the newest of each
        // of its plans.
        let mut latest: Option<OperationRecord> = None;
        for plan in config
            .plans()
            .iter()
            .filter(|plan| &plan.repository == alias)
        {
            let page = self
                .operations
                .list(&HistoryQuery {
                    plan: Some(plan.id.clone()),
                    after: None,
                    limit: PageSize::new(1),
                })
                .await?;
            let Some(newest) = page.operations.into_iter().next() else {
                continue;
            };
            match &latest {
                Some(known) if known.id >= newest.id => {}
                _ => latest = Some(newest),
            }
        }
        match latest {
            Some(operation) if operation.status.found_no_repository() => {
                Ok(!self.ever_succeeded(config, alias).await?)
            }
            _ => Ok(false),
        }
    }

    /// Whether any operation of a plan that backs up to `alias` succeeded,
    /// with warnings or without: one indexed lookup per plan, asked only of
    /// a repository whose latest operation found none there.
    async fn ever_succeeded(
        &self,
        config: &AgentConfig,
        alias: &RepositoryAlias,
    ) -> Result<bool, StoreError> {
        for plan in config
            .plans()
            .iter()
            .filter(|plan| &plan.repository == alias)
        {
            if self.operations.ever_succeeded(&plan.id).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use domain::execution::{
        FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning,
        WarningKind,
    };
    use domain::planning::{
        AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
        RepositorySettings, Source,
    };
    use domain::secrets::SecretRef;
    use domain::{OperationId, PlanId, RepositoryAlias, RepositoryId, Timestamp, Uuid};

    use super::CreateOffers;
    use crate::fakes::InMemoryOperationStore;
    use crate::ports::OperationStore;

    fn alias(name: &str) -> RepositoryAlias {
        RepositoryAlias::parse(name).unwrap_or_else(|e| panic!("{name}: {e:?}"))
    }

    fn plan_id(name: &str) -> PlanId {
        PlanId::parse(name).unwrap_or_else(|e| panic!("{name}: {e:?}"))
    }

    /// The entry of `name`, at a location of its own, recording `id`.
    fn entry(name: &str, id: Option<RepositoryId>) -> RepositoryConfig {
        RepositoryConfig {
            locations: Default::default(),
            settings: RepositorySettings {
                repository: format!("/home/demo/scratch/{name}"),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            id,
            maintenance: Maintenance::None,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        }
    }

    /// Repositories `demo` and `other`, neither recording its ID, with the
    /// plans `home` and `photos` backing up to `demo` and `docs` to `other`.
    fn config() -> AgentConfig {
        config_recording(None)
    }

    /// [`config`], with `demo`'s entry recording `demo_id`.
    fn config_recording(demo_id: Option<RepositoryId>) -> AgentConfig {
        let repositories = [
            (alias("demo"), entry("demo", demo_id)),
            (alias("other"), entry("other", None)),
        ]
        .into_iter()
        .collect();
        let plan = |id: &str, repository: &str| {
            Plan::new(
                plan_id(id),
                alias(repository),
                vec![Source::parse("/home/demo").unwrap_or_else(|e| panic!("{e:?}"))],
            )
        };
        AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            repositories,
            vec![
                plan("home", "demo"),
                plan("photos", "demo"),
                plan("docs", "other"),
            ],
        )
        .unwrap_or_else(|e| panic!("the configuration is whole: {e:?}"))
    }

    fn at() -> Timestamp {
        Timestamp::from_second(1_800_000_000).unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// Records operation `n` (a higher `n` is a newer ID) of the plan, a
    /// backup, as `status` has it.
    async fn record(store: &InMemoryOperationStore, n: u128, plan: &str, status: &OperationStatus) {
        let mut operation = Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(n)),
            OperationKind::Backup,
            Trigger::Manual,
            Target {
                plan: Some(plan_id(plan)),
                repository: None,
            },
            at(),
        );
        let moved = match status {
            OperationStatus::Queued => Ok(()),
            OperationStatus::Running => operation.start(at()),
            OperationStatus::Succeeded => {
                operation.start(at()).and_then(|()| operation.succeed(at()))
            }
            OperationStatus::SucceededWithWarnings => operation
                .start(at())
                .and_then(|()| {
                    operation.record_warning(Warning {
                        kind: WarningKind::SourceUnreadable,
                        item: Some("/home/demo/a".to_owned()),
                        during: "reading".to_owned(),
                        detail: "permission denied".to_owned(),
                    })
                })
                .and_then(|()| operation.succeed(at())),
            OperationStatus::Failed { reason } => operation
                .start(at())
                .and_then(|()| operation.fail(reason.clone(), at())),
            OperationStatus::Cancelled => operation.cancel(at()),
        };
        moved.unwrap_or_else(|e| panic!("{status:?}: {e:?}"));
        assert_eq!(operation.status(), status, "the operation is as asked");
        store
            .save(&operation, None)
            .await
            .unwrap_or_else(|e| panic!("the operation is stored: {e:?}"));
    }

    async fn offered(store: Arc<InMemoryOperationStore>) -> BTreeSet<RepositoryAlias> {
        CreateOffers::new(store)
            .offered(&config())
            .await
            .unwrap_or_else(|e| panic!("the log is read: {e:?}"))
    }

    fn not_found() -> OperationStatus {
        OperationStatus::Failed {
            reason: FailureReason::RepositoryNotFound,
        }
    }

    /// A folder that isn't there, as on a drive that isn't plugged in, never
    /// fails `repository-not-found`, so a repository with nothing on record
    /// may be one that is there, or one on such a drive (decision 53).
    #[tokio::test]
    async fn a_repository_with_no_operation_on_record_is_not_offered() {
        let store = Arc::new(InMemoryOperationStore::new());
        assert_eq!(offered(store).await, BTreeSet::new());
    }

    #[tokio::test]
    async fn a_repository_whose_latest_operation_found_no_repository_is_offered() {
        let store = Arc::new(InMemoryOperationStore::new());
        record(
            &store,
            1,
            "photos",
            &OperationStatus::Failed {
                reason: FailureReason::DaemonStopped,
            },
        )
        .await;
        record(&store, 2, "home", &OperationStatus::Cancelled).await;
        record(&store, 3, "home", &not_found()).await;
        // Another repository's success doesn't count for this one.
        record(&store, 4, "docs", &OperationStatus::Succeeded).await;
        assert_eq!(offered(store).await, BTreeSet::from([alias("demo")]));
    }

    /// An unplugged drive whose mount point is an empty folder: the latest
    /// backup found no repository there, but a backup to it once succeeded.
    /// Creating one would make a new, empty repository where the old one
    /// should be (decision 53).
    #[tokio::test]
    async fn a_repository_one_of_whose_plans_ever_succeeded_is_not_offered() {
        for status in [
            OperationStatus::Succeeded,
            OperationStatus::SucceededWithWarnings,
        ] {
            for plan in ["home", "photos"] {
                let store = Arc::new(InMemoryOperationStore::new());
                record(&store, 1, plan, &status).await;
                record(&store, 2, "home", &not_found()).await;
                assert_eq!(
                    offered(store).await,
                    BTreeSet::new(),
                    "{status:?} of {plan}"
                );
            }
        }
    }

    #[tokio::test]
    async fn any_other_latest_operation_is_not_offered() {
        for status in [
            OperationStatus::Succeeded,
            OperationStatus::Queued,
            OperationStatus::Running,
            OperationStatus::Cancelled,
            OperationStatus::Failed {
                reason: FailureReason::RepositoryExists,
            },
            OperationStatus::Failed {
                reason: FailureReason::DaemonStopped,
            },
        ] {
            let store = Arc::new(InMemoryOperationStore::new());
            record(&store, 1, "home", &not_found()).await;
            record(&store, 2, "home", &status).await;
            assert_eq!(offered(store).await, BTreeSet::new(), "{status:?}");
        }
    }

    #[tokio::test]
    async fn the_latest_operation_is_the_newest_across_the_repositorys_plans() {
        let store = Arc::new(InMemoryOperationStore::new());
        record(&store, 1, "photos", &OperationStatus::Cancelled).await;
        record(&store, 2, "home", &not_found()).await;
        record(&store, 3, "docs", &not_found()).await;
        assert_eq!(
            offered(store.clone()).await,
            BTreeSet::from([alias("demo"), alias("other")])
        );
        record(&store, 4, "photos", &OperationStatus::Running).await;
        assert_eq!(offered(store).await, BTreeSet::from([alias("other")]));
    }

    /// An entry that records its repository's ID names a repository
    /// Ladderback has opened, so it was there whatever the log says, even a
    /// latest operation that found none there, as an unplugged drive's whose
    /// mount point is an empty folder does (decision 53).
    #[tokio::test]
    async fn a_repository_whose_entry_records_its_id_is_not_offered() {
        let recorded = RepositoryId::parse(&"b".repeat(64)).unwrap_or_else(|e| panic!("{e:?}"));
        let store = Arc::new(InMemoryOperationStore::new());
        record(&store, 1, "home", &not_found()).await;
        record(&store, 2, "docs", &not_found()).await;
        let found_none = CreateOffers::new(store)
            .offered(&config_recording(Some(recorded)))
            .await
            .unwrap_or_else(|e| panic!("the log is read: {e:?}"));
        assert_eq!(found_none, BTreeSet::from([alias("other")]));
    }
}
