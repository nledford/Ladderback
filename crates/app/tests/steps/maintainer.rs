//! Steps for `features/maintenance/change_owner.feature`: maintenance from
//! this agent turned on or off for a saved repository, against the in-memory
//! configuration of the connect steps and the agent's in-memory operation
//! log, where a scenario starts the maintenance that runs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::planning::{AgentConfig, Maintenance, RepositoryConfig};
use domain::{OperationId, RepositoryAlias, RepositoryId, Timestamp, Uuid};

use super::Agent;
use app::fakes::InMemoryConfigStore;
use app::ports::{ConfigError, ConfigStore, OperationStore, RecordError, Start};
use app::repositories::{ChangeMaintainer, MaintainerError};

/// What a scenario keeps between its steps.
#[derive(Debug, Default)]
pub struct MaintainerWorld {
    /// The entry as the scenario saved it.
    saved: Option<RepositoryConfig>,
    /// The maintenance operation the scenario started.
    running: Option<OperationId>,
    /// What turning maintenance on or off answered.
    answered: Option<Result<Maintenance, MaintainerError>>,
    /// How many times the use case wrote the configuration.
    writes: usize,
}

/// The configuration of the connect steps, counting the writes made to it.
struct Counted {
    inner: Arc<InMemoryConfigStore>,
    writes: AtomicUsize,
}

#[async_trait]
impl ConfigStore for Counted {
    async fn load(&self) -> Result<AgentConfig, ConfigError> {
        self.inner.load().await
    }

    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.save(config).await
    }

    async fn record_id(
        &self,
        alias: &RepositoryAlias,
        read: &RepositoryConfig,
        id: &RepositoryId,
    ) -> Result<(), RecordError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.record_id(alias, read, id).await
    }
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("a repository alias")
}

/// The ID the saved repository records.
fn recorded() -> RepositoryId {
    RepositoryId::parse(&"c".repeat(64)).expect("a repository ID")
}

fn elsewhere() -> RepositoryId {
    RepositoryId::parse(&"d".repeat(64)).expect("a repository ID")
}

async fn save_entry(agent: &mut Agent, name: &str, maintenance: &str, id: Option<RepositoryId>) {
    let entry = RepositoryConfig {
        locations: Default::default(),
        id,
        settings: agent.repositories.settings().clone(),
        maintenance: Maintenance::parse(maintenance).expect("hub, local or none"),
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let config = AgentConfig::new(
        Default::default(),
        Default::default(),
        BTreeMap::from([(alias(name), entry.clone())]),
        Vec::new(),
    )
    .expect("a valid configuration");
    agent
        .repositories
        .config()
        .save(&config)
        .await
        .expect("the configuration saves");
    agent.maintainer.saved = Some(entry);
}

#[given(expr = "the repository {string} is saved asking for {string} maintenance")]
async fn saved_asking(agent: &mut Agent, name: String, maintenance: String) {
    save_entry(agent, &name, &maintenance, Some(recorded())).await;
}

#[given(
    expr = "the repository {string} is saved asking for {string} maintenance, with no ID recorded"
)]
async fn saved_without_id(agent: &mut Agent, name: String, maintenance: String) {
    save_entry(agent, &name, &maintenance, None).await;
}

/// Starts a running operation of `kind` on `repository` in the agent's log.
async fn start_running(agent: &mut Agent, kind: OperationKind, repository: RepositoryId) {
    let id = OperationId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0619));
    let at = Timestamp::from_second(1_800_000_000).expect("a time");
    let target = Target {
        plan: None,
        repository: Some(repository),
    };
    let mut operation = Operation::queue(id, kind, Trigger::Manual, target, at);
    operation.start(at).expect("a queued operation starts");
    assert_eq!(
        agent.store.try_start(&operation).await,
        Ok(Start::Started),
        "the log starts the operation"
    );
    if kind.is_maintenance() {
        agent.maintainer.running = Some(id);
    }
}

#[given(
    regex = r#"^(a forget|a prune|a check|an adoption) of the repository "[^"]+" is running here$"#
)]
async fn maintenance_running(agent: &mut Agent, what: String) {
    let kind = match what.as_str() {
        "a forget" => OperationKind::Forget,
        "a prune" => OperationKind::Prune,
        "a check" => OperationKind::Check,
        "an adoption" => OperationKind::Adopt,
        other => panic!("no step starts {other}"),
    };
    start_running(agent, kind, recorded()).await;
}

#[given("a prune of another repository is running")]
async fn prune_elsewhere(agent: &mut Agent) {
    start_running(agent, OperationKind::Prune, elsewhere()).await;
}

#[given(expr = "a backup to the repository {string} is running")]
async fn backup_running(agent: &mut Agent, _name: String) {
    start_running(agent, OperationKind::Backup, recorded()).await;
}

#[when(regex = r#"^maintenance of "([^"]+)" from this agent is turned (on|off)$"#)]
async fn turned(agent: &mut Agent, name: String, switch: String) {
    let config = Arc::new(Counted {
        inner: agent.repositories.config(),
        writes: AtomicUsize::new(0),
    });
    let use_case = ChangeMaintainer::new(config.clone(), agent.store.clone(), Arc::default());
    let answered = use_case.change(&alias(&name), switch == "on").await;
    agent.maintainer.answered = Some(answered);
    agent.maintainer.writes = config.writes.load(Ordering::SeqCst);
}

/// The entry saved under `name` now.
async fn entry(agent: &Agent, name: &str) -> RepositoryConfig {
    agent
        .repositories
        .config()
        .load()
        .await
        .expect("the configuration loads")
        .repository(&alias(name))
        .unwrap_or_else(|| panic!("no repository {name} is saved"))
        .clone()
}

#[then(expr = "the repository {string} asks for {string} maintenance")]
async fn asks_for(agent: &mut Agent, name: String, maintenance: String) {
    assert_eq!(
        entry(agent, &name).await.maintenance,
        Maintenance::parse(&maintenance).expect("hub, local or none")
    );
}

#[then(expr = "the rest of the entry {string} is as it was saved")]
async fn rest_as_saved(agent: &mut Agent, name: String) {
    let now = entry(agent, &name).await;
    let mut expected = agent
        .maintainer
        .saved
        .clone()
        .expect("the scenario saved it");
    expected.maintenance = now.maintenance;
    assert_eq!(now, expected, "only its maintenance changed");
}

#[then("the configuration was not written again")]
fn not_written(agent: &mut Agent) {
    assert!(
        agent.maintainer.answered.is_some(),
        "maintenance was turned on or off"
    );
    assert_eq!(agent.maintainer.writes, 0, "the configuration was written");
}

#[then("it is refused because that maintenance operation is running")]
fn refused_busy(agent: &mut Agent) {
    let running = agent
        .maintainer
        .running
        .expect("a maintenance operation runs");
    assert_eq!(
        agent.maintainer.answered.take(),
        Some(Err(MaintainerError::Busy { running }))
    );
    assert_eq!(agent.maintainer.writes, 0, "nothing was written");
}

#[then(expr = "it is refused because the configuration holds no repository {string}")]
fn refused_unknown(agent: &mut Agent, name: String) {
    assert_eq!(
        agent.maintainer.answered.as_ref(),
        Some(&Err(MaintainerError::Unknown(alias(&name))))
    );
}
