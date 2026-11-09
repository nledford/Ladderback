//! An agent's whole configuration (technical design §3.1, §6.1): its own
//! settings, the repositories it knows, its plans, and the hooks that wrap
//! everything. The aggregate of the Planning context.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use crate::{PlanId, RepositoryAlias};

use super::{Hooks, Hostname, InvalidValue, Locale, Plan, RepositoryConfig, ValueKind};

/// How many operations an agent runs at once: from 1 to
/// [`MAX`](Self::MAX). The default is 1, so that a laptop does not run two
/// backups over each other. The ceiling is not rustic's: it keeps a number
/// from a file from sizing a pool of threads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Concurrency(u8);

impl Concurrency {
    pub const MAX: u8 = 64;

    pub fn new(value: i64) -> Result<Self, InvalidValue> {
        u8::try_from(value)
            .ok()
            .filter(|count| (1..=Self::MAX).contains(count))
            .map(Self)
            .ok_or_else(|| InvalidValue::new(ValueKind::Concurrency, value.to_string()))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl Default for Concurrency {
    fn default() -> Self {
        Self(1)
    }
}

/// How nice the agent's work is to the rest of the machine, as `nice(1)`
/// counts it: from 0, the process's own priority, to [`MAX`](Self::MAX), the
/// politest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nice(u8);

impl Nice {
    pub const MAX: u8 = 19;

    pub fn new(value: i64) -> Result<Self, InvalidValue> {
        u8::try_from(value)
            .ok()
            .filter(|nice| *nice <= Self::MAX)
            .map(Self)
            .ok_or_else(|| InvalidValue::new(ValueKind::Nice, value.to_string()))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

/// The settings of the agent itself: the `[agent]` table of §6.1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentSettings {
    /// The host name its snapshots carry, when it isn't the system's.
    pub hostname: Option<Hostname>,
    pub concurrency: Concurrency,
    pub nice: Option<Nice>,
    /// Whether the hub may restore files onto this machine (§10.4).
    pub allow_hub_restore: bool,
    /// The language of its notifications, when it isn't the system's (§8.4).
    pub locale: Option<Locale>,
}

/// A rule that only the whole configuration can break. `index` is the
/// position of the plan in the list, from 0, so that a message can say
/// which of a long list it means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    /// A plan's ID was already taken by an earlier plan.
    DuplicatePlan { index: usize, plan: PlanId },
    /// A plan names a repository that the configuration doesn't have.
    UnknownRepository {
        index: usize,
        plan: PlanId,
        repository: RepositoryAlias,
    },
    /// A plan backs up nothing.
    NoSources { index: usize, plan: PlanId },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl Error for Violation {}

/// What an agent is configured to do. It is whole or it isn't there: the only
/// way to build one is [`AgentConfig::new`], which checks the rules that
/// reach across its parts, and the default, which is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentConfig {
    agent: AgentSettings,
    hooks: Hooks,
    repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
    plans: Vec<Plan>,
}

impl AgentConfig {
    /// Checks the plans one by one, in order; the first one to break a rule is
    /// reported. For each plan the rules are checked in this order: its ID is
    /// not one an earlier plan has, its repository is one of
    /// `repositories`, and it has at least one source.
    pub fn new(
        agent: AgentSettings,
        hooks: Hooks,
        repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
        plans: Vec<Plan>,
    ) -> Result<Self, Violation> {
        let mut taken = BTreeSet::new();
        for (index, plan) in plans.iter().enumerate() {
            if !taken.insert(&plan.id) {
                return Err(Violation::DuplicatePlan {
                    index,
                    plan: plan.id.clone(),
                });
            }
            if !repositories.contains_key(&plan.repository) {
                return Err(Violation::UnknownRepository {
                    index,
                    plan: plan.id.clone(),
                    repository: plan.repository.clone(),
                });
            }
            if plan.sources.is_empty() {
                return Err(Violation::NoSources {
                    index,
                    plan: plan.id.clone(),
                });
            }
        }
        Ok(Self {
            agent,
            hooks,
            repositories,
            plans,
        })
    }

    pub fn agent(&self) -> &AgentSettings {
        &self.agent
    }

    /// The hooks that wrap every operation of the agent.
    pub fn hooks(&self) -> &Hooks {
        &self.hooks
    }

    /// The repositories, in the order of their aliases.
    pub fn repositories(&self) -> &BTreeMap<RepositoryAlias, RepositoryConfig> {
        &self.repositories
    }

    pub fn repository(&self, alias: &RepositoryAlias) -> Option<&RepositoryConfig> {
        self.repositories.get(alias)
    }

    /// The plans, in the order they were written.
    pub fn plans(&self) -> &[Plan] {
        &self.plans
    }

    pub fn plan(&self, id: &PlanId) -> Option<&Plan> {
        self.plans.iter().find(|plan| &plan.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{GroupBy, Maintenance, RepositorySettings, Source};
    use crate::secrets::SecretRef;

    fn alias(text: &str) -> RepositoryAlias {
        RepositoryAlias::parse(text).unwrap()
    }

    fn id(text: &str) -> PlanId {
        PlanId::parse(text).unwrap()
    }

    fn repository(maintenance: Maintenance) -> RepositoryConfig {
        RepositoryConfig {
            settings: RepositorySettings {
                repository: "/srv/demo-repo".to_owned(),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        }
    }

    fn repositories(aliases: &[&str]) -> BTreeMap<RepositoryAlias, RepositoryConfig> {
        aliases
            .iter()
            .map(|name| (alias(name), repository(Maintenance::None)))
            .collect()
    }

    fn plan(plan: &str, repository: &str) -> Plan {
        Plan::new(
            id(plan),
            alias(repository),
            vec![Source::parse("/home/demo").unwrap()],
        )
    }

    fn config(
        repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
        plans: Vec<Plan>,
    ) -> Result<AgentConfig, Violation> {
        AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            repositories,
            plans,
        )
    }

    #[test]
    fn concurrency_is_one_up_to_sixty_four() {
        assert_eq!(Concurrency::MAX, 64);
        for value in [1, 2, 8, 63, 64] {
            assert_eq!(i64::from(Concurrency::new(value).unwrap().get()), value);
        }
        for value in [0, -1, 65, 256, i64::MIN, i64::MAX] {
            assert_eq!(
                Concurrency::new(value),
                Err(InvalidValue::new(ValueKind::Concurrency, value.to_string())),
                "{value}"
            );
        }
    }

    #[test]
    fn an_agent_runs_one_operation_at_a_time_unless_told_otherwise() {
        assert_eq!(Concurrency::default().get(), 1);
    }

    #[test]
    fn nice_is_zero_up_to_nineteen() {
        assert_eq!(Nice::MAX, 19);
        for value in [0, 1, 10, 18, 19] {
            assert_eq!(i64::from(Nice::new(value).unwrap().get()), value);
        }
        for value in [-1, 20, 256, i64::MIN, i64::MAX] {
            assert_eq!(
                Nice::new(value),
                Err(InvalidValue::new(ValueKind::Nice, value.to_string())),
                "{value}"
            );
        }
    }

    #[test]
    fn an_agent_that_was_told_nothing_has_only_defaults() {
        let settings = AgentSettings::default();
        assert_eq!(settings.hostname, None);
        assert_eq!(settings.concurrency.get(), 1);
        assert_eq!(settings.nice, None);
        assert!(!settings.allow_hub_restore);
        assert_eq!(settings.locale, None);
    }

    #[test]
    fn an_empty_configuration_is_valid_and_is_the_default() {
        let empty = config(BTreeMap::new(), Vec::new()).unwrap();
        assert_eq!(empty, AgentConfig::default());
        assert!(empty.repositories().is_empty());
        assert!(empty.plans().is_empty());
        assert!(empty.hooks().is_empty());
        assert_eq!(empty.agent(), &AgentSettings::default());
    }

    #[test]
    fn a_configuration_keeps_what_it_was_given() {
        let settings = AgentSettings {
            hostname: Some(Hostname::parse("demo-host").unwrap()),
            concurrency: Concurrency::new(2).unwrap(),
            nice: Some(Nice::new(10).unwrap()),
            allow_hub_restore: true,
            locale: Some(Locale::parse("en-US").unwrap()),
        };
        let hooks = Hooks {
            run_before: vec![crate::planning::HookEntry::parse("true").unwrap()],
            ..Hooks::default()
        };
        let mut repositories = repositories(&["nas"]);
        repositories.insert(alias("cloud"), repository(Maintenance::Local));
        let plans = vec![plan("home", "nas"), plan("work", "cloud")];

        let config = AgentConfig::new(
            settings.clone(),
            hooks.clone(),
            repositories.clone(),
            plans.clone(),
        )
        .unwrap();

        assert_eq!(config.agent(), &settings);
        assert_eq!(config.hooks(), &hooks);
        assert_eq!(config.repositories(), &repositories);
        assert_eq!(config.plans(), plans.as_slice());
        assert_eq!(
            config.repository(&alias("cloud")),
            Some(&repository(Maintenance::Local))
        );
        assert_eq!(config.repository(&alias("elsewhere")), None);
        assert_eq!(config.plan(&id("work")), plans.get(1));
        assert_eq!(config.plan(&id("home")), plans.first());
        assert_eq!(config.plan(&id("missing")), None);
        assert_ne!(config, AgentConfig::default());
    }

    #[test]
    fn repositories_are_listed_in_the_order_of_their_aliases() {
        let config = config(repositories(&["zeta", "alpha", "mid"]), Vec::new()).unwrap();
        let aliases: Vec<_> = config.repositories().keys().map(|a| a.as_str()).collect();
        assert_eq!(aliases, ["alpha", "mid", "zeta"]);
    }

    #[test]
    fn plans_are_listed_in_the_order_they_were_written() {
        let config = config(
            repositories(&["nas"]),
            vec![plan("zeta", "nas"), plan("alpha", "nas")],
        )
        .unwrap();
        let ids: Vec<_> = config.plans().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["zeta", "alpha"]);
    }

    #[test]
    fn a_plan_must_name_a_repository_the_configuration_has() {
        let result = config(
            repositories(&["nas"]),
            vec![plan("home", "nas"), plan("work", "cloud")],
        );
        assert_eq!(
            result,
            Err(Violation::UnknownRepository {
                index: 1,
                plan: id("work"),
                repository: alias("cloud"),
            })
        );
    }

    #[test]
    fn a_configuration_without_repositories_has_no_plans() {
        assert_eq!(
            config(BTreeMap::new(), vec![plan("home", "nas")]),
            Err(Violation::UnknownRepository {
                index: 0,
                plan: id("home"),
                repository: alias("nas"),
            })
        );
    }

    #[test]
    fn plan_ids_are_unique() {
        let result = config(
            repositories(&["nas"]),
            vec![
                plan("home", "nas"),
                plan("work", "nas"),
                plan("home", "nas"),
            ],
        );
        assert_eq!(
            result,
            Err(Violation::DuplicatePlan {
                index: 2,
                plan: id("home"),
            })
        );
    }

    #[test]
    fn a_plan_must_back_up_something() {
        let mut empty = plan("work", "nas");
        empty.sources.clear();
        let result = config(repositories(&["nas"]), vec![plan("home", "nas"), empty]);
        assert_eq!(
            result,
            Err(Violation::NoSources {
                index: 1,
                plan: id("work"),
            })
        );
    }

    #[test]
    fn a_plan_is_checked_for_a_duplicate_then_a_repository_then_its_sources() {
        let mut no_sources = plan("home", "cloud");
        no_sources.sources.clear();

        // Duplicate, unknown repository and no sources: the duplicate.
        assert_eq!(
            config(
                repositories(&["nas"]),
                vec![plan("home", "nas"), no_sources.clone()]
            ),
            Err(Violation::DuplicatePlan {
                index: 1,
                plan: id("home")
            })
        );

        // Unknown repository and no sources: the repository.
        assert_eq!(
            config(repositories(&["nas"]), vec![no_sources]),
            Err(Violation::UnknownRepository {
                index: 0,
                plan: id("home"),
                repository: alias("cloud"),
            })
        );
    }

    #[test]
    fn the_first_plan_to_break_a_rule_is_the_one_reported() {
        let mut no_sources = plan("first", "nas");
        no_sources.sources.clear();
        assert_eq!(
            config(
                repositories(&["nas"]),
                vec![no_sources, plan("second", "cloud"), plan("first", "nas"),]
            ),
            Err(Violation::NoSources {
                index: 0,
                plan: id("first")
            })
        );
    }
}
