//! Turning maintenance from this agent on or off for a repository the
//! configuration already holds (technical design §7.3, decision 48): the one
//! setting of its entry that says who it asks to maintain it. Exactly one
//! party maintains a repository (I1), so [`Ownership`] decides, and a
//! maintenance operation of the repository running here keeps it maintained
//! here until it ends.

use std::sync::Arc;

use domain::maintenance::Ownership;
use domain::planning::Maintenance;
use domain::{OperationId, RepositoryAlias};

use crate::ports::{ConfigError, ConfigStore, OperationStore, StoreError};
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

    /// Asks this agent to maintain the repository saved under `alias`
    /// (`maintain_here`), or nobody, and answers what its entry asks for
    /// now. An entry that already asks for it is not written again.
    pub async fn change(
        &self,
        alias: &RepositoryAlias,
        maintain_here: bool,
    ) -> Result<Maintenance, MaintainerError> {
        let _ = (
            &self.config,
            &self.operations,
            &self.writes,
            alias,
            maintain_here,
        );
        let _ = Ownership::owned_here;
        todo!()
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
}
