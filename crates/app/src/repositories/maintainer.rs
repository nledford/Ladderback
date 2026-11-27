//! Turning maintenance from this agent on or off for a repository the
//! configuration already holds (technical design §7.3, decision 48): the one
//! setting of its entry that says who it asks to maintain it. Exactly one
//! party maintains a repository (I1), so [`Ownership`] decides, and a
//! maintenance operation of the repository running here keeps it maintained
//! here until it ends.

use std::sync::Arc;

use domain::maintenance::{Ownership, OwnershipChange};
use domain::planning::Maintenance;
use domain::{OperationId, RepositoryAlias};

use crate::ports::{ConfigError, ConfigStore, OperationStore, Problem, StoreError};
use crate::repositories::ConfigWrites;

/// Changes whether this agent maintains a saved repository, writing only its
/// `maintenance` and leaving the rest of the configuration as it is. It holds
/// the configuration's write lock ([`ConfigWrites`]) across the change, as
/// every read-modify-write of it does and as a maintenance start does while
/// it reads again who maintains the repository and takes its slot (I1).
pub struct ChangeMaintainer {
    config: Arc<dyn ConfigStore>,
    operations: Arc<dyn OperationStore>,
    writes: Arc<ConfigWrites>,
}

/// Why the maintenance of a saved repository wasn't changed. A typed code,
/// never a sentence (§8.4); nothing was written.
#[derive(Debug, PartialEq)]
pub enum MaintainerError {
    /// The configuration holds no repository under the alias.
    Unknown(RepositoryAlias),
    /// A maintenance operation of the repository, `running`, runs here, so
    /// this agent maintains it until that one ends (I1).
    Busy { running: OperationId },
    /// The configuration couldn't be read or written.
    Config(ConfigError),
    /// The operation log, which says what maintenance runs, couldn't be
    /// read.
    Log(StoreError),
}

#[expect(
    clippy::result_large_err,
    reason = "`MaintainerError` is a typed code for the caller to match; none of its variants is large on its own"
)]
impl ChangeMaintainer {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        writes: Arc<ConfigWrites>,
    ) -> Self {
        Self {
            config,
            operations,
            writes,
        }
    }

    /// Notes the pause of a repository this paired machine turns
    /// maintenance on for while it holds no lease on it (#1671).
    pub fn with_leases(self, _leases: Arc<crate::leases::MaintenanceLeases>) -> Self {
        todo!()
    }

    /// Asks this agent to maintain the repository saved under `alias`
    /// (`maintain_here`), or nobody, and answers what its entry asks for
    /// now. An entry that already asks for it is not written again.
    pub async fn change(
        &self,
        alias: &RepositoryAlias,
        maintain_here: bool,
    ) -> Result<Maintenance, MaintainerError> {
        let _writing = self.writes.lock().await;
        let loaded = self.config.load().await.map_err(MaintainerError::Config)?;
        let entry = loaded
            .repository(alias)
            .ok_or_else(|| MaintainerError::Unknown(alias.clone()))?;
        let request = match Ownership::change(entry.maintenance, maintain_here) {
            OwnershipChange::Unchanged => return Ok(entry.maintenance),
            OwnershipChange::TakeOver => Maintenance::Local,
            OwnershipChange::GiveUp => {
                // An entry that records no ID can't tell its maintenance
                // apart from another repository's: any running here keeps it.
                let running = self
                    .operations
                    .running_maintenance(entry.id.as_ref())
                    .await
                    .map_err(MaintainerError::Log)?;
                Ownership::give_up(running).map_err(|running| MaintainerError::Busy { running })?
            }
        };
        let mut repositories = loaded.repositories().clone();
        if let Some(entry) = repositories.get_mut(alias) {
            entry.maintenance = request;
        }
        let changed = loaded
            .replacing(
                loaded.agent().clone(),
                loaded.hooks().clone(),
                repositories,
                loaded.plans().to_vec(),
            )
            .map_err(|violation| {
                MaintainerError::Config(ConfigError::Invalid {
                    file: String::new(),
                    key: None,
                    at: None,
                    problem: Problem::Violation(violation),
                    detail: String::new(),
                })
            })?;
        self.config
            .save(&changed)
            .await
            .map_err(MaintainerError::Config)?;
        Ok(request)
    }
}

#[cfg(test)]
#[cfg(feature = "fakes")]
mod tests {
    use std::collections::BTreeMap;

    use domain::execution::{Operation, OperationKind, Target, Trigger};
    use domain::planning::{AgentConfig, RepositoryConfig, RepositorySettings};
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{RepositoryId, Timestamp, Uuid};
    use futures::FutureExt;

    use super::*;
    use crate::fakes::{InMemoryConfigStore, InMemoryOperationStore};
    use crate::ports::Start;

    fn alias() -> RepositoryAlias {
        RepositoryAlias::parse("demo").expect("an alias")
    }

    fn repository() -> RepositoryId {
        RepositoryId::parse(&"07".repeat(32)).expect("a repository ID")
    }

    fn maintained_here() -> AgentConfig {
        let entry = RepositoryConfig {
            locations: Default::default(),
            id: Some(repository()),
            settings: RepositorySettings {
                repository: "/srv/demo/repo".to_owned(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new("demo-password")),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Maintenance::Local,
            group_by: Default::default(),
            hooks: Default::default(),
        };
        AgentConfig::new(
            Default::default(),
            Default::default(),
            BTreeMap::from([(alias(), entry)]),
            Vec::new(),
        )
        .expect("a valid configuration")
    }

    /// A start holds the lock while it reads again that this agent
    /// maintains the repository and takes its slot, so a turn-off waits for
    /// it and then finds the run it let in (I1).
    #[tokio::test]
    async fn turning_maintenance_off_waits_for_a_start_and_then_finds_its_run() {
        let config = Arc::new(InMemoryConfigStore::new());
        config
            .save(&maintained_here())
            .await
            .expect("the configuration saves");
        let operations = Arc::new(InMemoryOperationStore::new());
        let writes: Arc<ConfigWrites> = Arc::default();
        let use_case = ChangeMaintainer::new(config.clone(), operations.clone(), writes.clone());

        // A maintenance start, between its read of the configuration and its
        // slot.
        let starting = writes.lock().await;
        let demo = alias();
        let mut change = Box::pin(use_case.change(&demo, false));
        assert!(
            (&mut change).now_or_never().is_none(),
            "turning maintenance off waits for the start to take its slot"
        );
        let id = OperationId::from_uuid(Uuid::from_u128(0x0619));
        let at = Timestamp::from_second(1_800_000_000).expect("a time");
        let target = Target {
            plan: None,
            repository: Some(repository()),
        };
        let mut prune = Operation::queue(id, OperationKind::Prune, Trigger::Manual, target, at);
        prune.start(at).expect("a queued operation starts");
        assert_eq!(operations.try_start(&prune).await, Ok(Start::Started));
        drop(starting);

        assert_eq!(change.await, Err(MaintainerError::Busy { running: id }));
        let now = config.load().await.expect("the configuration loads");
        assert_eq!(
            now.repository(&alias()).map(|entry| entry.maintenance),
            Some(Maintenance::Local),
            "the entry still asks this agent to maintain it"
        );
    }

    /// Turning maintenance off keeps what a person set AI agents may do
    /// without a token.
    #[tokio::test]
    async fn changing_the_maintainer_keeps_what_ai_agents_may_do_without_a_token() {
        let no_backup_runs = domain::access::permissions::Permissions::default()
            .with(domain::access::permissions::Permission::BackupRuns, None)
            .expect("a level the row has");
        let config = Arc::new(InMemoryConfigStore::new());
        config
            .save(&maintained_here().with_without_token(no_backup_runs))
            .await
            .expect("the configuration saves");
        let use_case = ChangeMaintainer::new(
            config.clone(),
            Arc::new(InMemoryOperationStore::new()),
            Arc::default(),
        );
        use_case
            .change(&alias(), false)
            .await
            .expect("nothing runs, so maintenance turns off");
        let now = config.load().await.expect("the configuration loads");
        assert_eq!(
            now.repository(&alias()).map(|entry| entry.maintenance),
            Some(Maintenance::None)
        );
        assert_eq!(now.without_token(), no_backup_runs);
    }
}
