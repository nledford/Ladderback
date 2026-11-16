//! Creating, changing and deleting a plan (scheduling-and-config §6.1): each
//! is one change of the configuration, read, changed by the rules the
//! [`AgentConfig`] aggregate keeps, and saved, under the lock every write of
//! the daemon's configuration holds, so two writes can't undo each other.
//! A change saved is told to the scheduler, which reads the configuration
//! again at once, so a new or changed plan's next run shows without waiting.

use std::sync::Arc;

use domain::PlanId;
use domain::planning::{AgentConfig, ChangeRefused, Plan};

use crate::ports::{ConfigError, ConfigStore};
use crate::repositories::ConfigWrites;
use crate::schedule::ConfigChanged;

/// Why a plan was not created, changed or deleted. Nothing was saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// The configuration refused the change.
    Refused(ChangeRefused),
    /// The configuration couldn't be read or written. Boxed: the error is
    /// large and every edit returns it.
    Config(Box<ConfigError>),
}

/// Creates, changes and deletes plans in the configuration `config`,
/// holding `writes`, and tells `changed` of each change saved.
pub struct EditPlans {
    config: Arc<dyn ConfigStore>,
    writes: Arc<ConfigWrites>,
    changed: ConfigChanged,
}

impl EditPlans {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        writes: Arc<ConfigWrites>,
        changed: ConfigChanged,
    ) -> Self {
        Self {
            config,
            writes,
            changed,
        }
    }

    /// These edits, bringing `proposals` up to date after each change
    /// saved, so one the change made stale goes stale at once.
    pub fn with_proposals(self, proposals: Arc<crate::proposals::Proposals>) -> Self {
        let _ = proposals;
        self
    }

    /// Adds `plan` after the configuration's last plan
    /// ([`AgentConfig::add_plan`]).
    pub async fn create(&self, plan: Plan) -> Result<(), EditError> {
        self.edit(|config| config.add_plan(plan)).await
    }

    /// Puts `plan` in place of the plan with its ID
    /// ([`AgentConfig::change_plan`]).
    pub async fn change(&self, plan: Plan) -> Result<(), EditError> {
        self.edit(|config| config.change_plan(plan)).await
    }

    /// Removes the plan `id` ([`AgentConfig::remove_plan`]). Its snapshots
    /// stay in the repository.
    pub async fn delete(&self, id: &PlanId) -> Result<(), EditError> {
        self.edit(|config| config.remove_plan(id)).await
    }

    /// Reads the configuration, changes it by `change` and saves what that
    /// gives, holding the write lock throughout, then tells the scheduler.
    async fn edit(
        &self,
        change: impl FnOnce(&AgentConfig) -> Result<AgentConfig, ChangeRefused>,
    ) -> Result<(), EditError> {
        let _writing = self.writes.lock().await;
        let loaded = self
            .config
            .load()
            .await
            .map_err(|error| EditError::Config(Box::new(error)))?;
        let changed = change(&loaded).map_err(EditError::Refused)?;
        self.config
            .save(&changed)
            .await
            .map_err(|error| EditError::Config(Box::new(error)))?;
        self.changed.tell();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::pin::pin;
    use std::sync::{Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::planning::{
        AgentSettings, Hooks, RepositoryConfig, RepositorySettings, Source, Violation,
    };
    use domain::secrets::SecretRef;
    use domain::{RepositoryAlias, RepositoryId};
    use futures::FutureExt;

    use super::*;
    use crate::fakes::InMemoryConfigStore;
    use crate::ports::RecordError;
    use crate::schedule::{ConfigChanges, config_changes};

    /// The in-memory store, counting its saves, and refusing them when it
    /// is read-only.
    #[derive(Default)]
    struct Counted {
        store: InMemoryConfigStore,
        saves: Mutex<usize>,
        read_only: bool,
    }

    impl Counted {
        fn saves(&self) -> usize {
            *self.saves.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    fn read_only() -> ConfigError {
        ConfigError::Unavailable {
            file: "config.toml".to_owned(),
            detail: "read-only".to_owned(),
        }
    }

    #[async_trait]
    impl ConfigStore for Counted {
        async fn load(&self) -> Result<AgentConfig, ConfigError> {
            self.store.load().await
        }

        async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
            *self.saves.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            if self.read_only {
                return Err(read_only());
            }
            self.store.save(config).await
        }

        async fn record_id(
            &self,
            alias: &RepositoryAlias,
            read: &RepositoryConfig,
            id: &RepositoryId,
        ) -> Result<(), RecordError> {
            self.store.record_id(alias, read, id).await
        }
    }

    fn id(text: &str) -> PlanId {
        PlanId::parse(text).unwrap()
    }

    fn alias() -> RepositoryAlias {
        RepositoryAlias::parse("nas").unwrap()
    }

    fn plan(name: &str) -> Plan {
        Plan::new(
            id(name),
            alias(),
            vec![Source::parse(&format!("/home/demo/{name}")).unwrap()],
        )
    }

    /// A store holding the repository `nas` and the plans `plans` on it.
    async fn holding(plans: &[&str], read_only: bool) -> Arc<Counted> {
        let repository = RepositoryConfig {
            locations: Default::default(),
            id: None,
            settings: RepositorySettings {
                repository: "/srv/demo-repo".to_owned(),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Default::default(),
            group_by: Default::default(),
            hooks: Hooks::default(),
        };
        let config = AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            BTreeMap::from([(alias(), repository)]),
            plans.iter().map(|name| plan(name)).collect(),
        )
        .unwrap();
        let store = Counted::default();
        store.store.save(&config).await.unwrap();
        Arc::new(Counted { read_only, ..store })
    }

    fn edits(store: &Arc<Counted>, writes: &Arc<ConfigWrites>) -> EditPlans {
        edits_telling(store, writes).0
    }

    /// The edits, and the changes they tell the scheduler.
    fn edits_telling(
        store: &Arc<Counted>,
        writes: &Arc<ConfigWrites>,
    ) -> (EditPlans, ConfigChanges) {
        let (changed, changes) = config_changes();
        (
            EditPlans::new(store.clone(), writes.clone(), changed),
            changes,
        )
    }

    /// Whether a change was told and not yet heard.
    fn told(changes: &ConfigChanges) -> bool {
        changes.next().now_or_never().is_some()
    }

    async fn ids(store: &Counted) -> Vec<String> {
        let config = store.load().await.unwrap();
        config
            .plans()
            .iter()
            .map(|plan| plan.id.as_str().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn a_created_plan_is_saved_into_the_configuration() {
        let store = holding(&["home"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        edits(&store, &writes).create(plan("docs")).await.unwrap();
        assert_eq!(ids(&store).await, ["home", "docs"]);
        assert_eq!(store.saves(), 1);
    }

    #[tokio::test]
    async fn a_changed_plan_is_saved_in_place() {
        let store = holding(&["home", "docs"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        let mut changed = plan("home");
        changed.label = Some("Home".to_owned());
        edits(&store, &writes)
            .change(changed.clone())
            .await
            .unwrap();
        let config = store.load().await.unwrap();
        assert_eq!(config.plan(&id("home")), Some(&changed));
        assert_eq!(ids(&store).await, ["home", "docs"]);
    }

    #[tokio::test]
    async fn a_deleted_plan_is_gone_from_the_configuration() {
        let store = holding(&["home", "docs"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        edits(&store, &writes).delete(&id("home")).await.unwrap();
        assert_eq!(ids(&store).await, ["docs"]);
    }

    #[tokio::test]
    async fn a_refused_change_saves_nothing() {
        let store = holding(&["home"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        let edit = edits(&store, &writes);
        assert_eq!(
            edit.create(plan("home")).await,
            Err(EditError::Refused(ChangeRefused::Violation(
                Violation::DuplicatePlan {
                    index: 1,
                    plan: id("home"),
                }
            )))
        );
        assert_eq!(
            edit.delete(&id("docs")).await,
            Err(EditError::Refused(ChangeRefused::UnknownPlan(id("docs"))))
        );
        assert_eq!(store.saves(), 0);
        assert!(writes.try_lock().is_some(), "the lock is let go");
    }

    #[tokio::test]
    async fn a_configuration_that_cannot_be_written_is_the_error() {
        let store = holding(&["home"], true).await;
        let writes = Arc::new(ConfigWrites::new(()));
        assert_eq!(
            edits(&store, &writes).create(plan("docs")).await,
            Err(EditError::Config(Box::new(read_only())))
        );
        assert!(writes.try_lock().is_some(), "the lock is let go");
    }

    #[tokio::test]
    async fn each_change_saved_tells_the_scheduler() {
        let store = holding(&["home"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        let (edit, changes) = edits_telling(&store, &writes);
        assert!(!told(&changes));
        edit.create(plan("docs")).await.unwrap();
        assert!(told(&changes), "a created plan");
        edit.change(plan("docs")).await.unwrap();
        assert!(told(&changes), "a changed plan");
        edit.delete(&id("docs")).await.unwrap();
        assert!(told(&changes), "a deleted plan");
    }

    #[tokio::test]
    async fn a_change_refused_or_not_saved_tells_nobody() {
        let store = holding(&["home"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        let (edit, changes) = edits_telling(&store, &writes);
        edit.create(plan("home")).await.unwrap_err();
        assert!(!told(&changes), "a refused change");
        let store = holding(&["home"], true).await;
        let (edit, changes) = edits_telling(&store, &writes);
        edit.create(plan("docs")).await.unwrap_err();
        assert!(!told(&changes), "a change the configuration couldn't save");
    }

    #[tokio::test]
    async fn a_change_waits_for_the_configuration_write_lock() {
        let store = holding(&["home"], false).await;
        let writes = Arc::new(ConfigWrites::new(()));
        let edit = edits(&store, &writes);
        let held = writes.lock().await;
        let mut creating = pin!(edit.create(plan("docs")));
        assert!(
            creating.as_mut().now_or_never().is_none(),
            "the change waits while another write holds the lock"
        );
        assert_eq!(store.saves(), 0);
        drop(held);
        creating.await.unwrap();
        assert_eq!(ids(&store).await, ["home", "docs"]);
    }
}
