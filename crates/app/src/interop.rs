//! Profile interop use cases (technical design §6.2): importing a rustic
//! profile into the configuration, and exporting one plan as a profile the
//! stock `rustic` CLI accepts.
//!
//! The adapter translates rustic's format to and from Ladderback's types;
//! what the translation means for the configuration is decided here: a plan
//! ID or repository alias the configuration already has is refused before
//! anything is saved, and the hostname rule below.
//!
//! **Hostname.** When every imported plan names the same host `H`, the
//! configuration has no plan yet and the agent has no hostname of its own,
//! the agent takes `H` and the plans name none. A plan whose host is the
//! agent's hostname names none either: it would carry that host anyway. In
//! every other case the plans keep the hosts they came with: moving the
//! agent's hostname would rename the snapshots of the plans it already has.

use std::path::Path;
use std::sync::Arc;

use domain::execution::plan_tags::PlanTags;
use domain::planning::{AgentConfig, Hostname, Plan, Violation};
use domain::{AgentId, PlanId, RepositoryAlias};

use crate::ports::{
    ConfigError, ConfigStore, ExportRefusal, PlanExport, Problem, ProfileError, ProfileNotice,
    ProfileReader, ProfileWriter,
};

/// Imports a rustic profile: reads it through the [`ProfileReader`], checks
/// it against the configuration, and saves the repository, the plans and the
/// profile's global hooks in one save, or saves nothing.
pub struct ImportProfile {
    reader: Arc<dyn ProfileReader>,
    config: Arc<dyn ConfigStore>,
}

/// What an import added to the configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub repository: RepositoryAlias,
    /// The IDs of the plans added, in the order of the profile.
    pub plans: Vec<PlanId>,
    /// What the profile could not carry over, and what changed on the way.
    pub notices: Vec<ProfileNotice>,
}

/// Why an import saved nothing. A typed code, never a sentence (§8.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The profile could not be read, or says what Ladderback cannot honour.
    Profile(ProfileError),
    /// The configuration already holds a repository under the alias, at
    /// another location than the profile's.
    AliasTaken(RepositoryAlias),
    /// The configuration already holds the profile's repository, under the
    /// alias given: a repository has one alias (§7.3).
    AlreadyConfigured(RepositoryAlias),
    /// The configuration, or the profile itself, already has a plan with
    /// this ID.
    PlanTaken(PlanId),
    /// The configuration could not be read or written, or the result is not
    /// a configuration it accepts.
    Config(ConfigError),
}

/// Exports one plan as a profile: finds the plan and its repository in the
/// configuration and hands them, with the agent's own hooks, hostname and
/// ID, to the [`ProfileWriter`].
pub struct ExportProfile {
    writer: Arc<dyn ProfileWriter>,
    config: Arc<dyn ConfigStore>,
    agent: AgentId,
}

/// A plan as a profile: the text, and what the writer left out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedProfile {
    pub text: String,
    pub notices: Vec<ProfileNotice>,
}

/// Why a plan was not exported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// The configuration has no plan with this ID.
    PlanUnknown(PlanId),
    /// The plan can't be written as a profile.
    Render(ExportRefusal),
    /// The configuration could not be read.
    Config(ConfigError),
}

#[allow(clippy::result_large_err)]
impl ImportProfile {
    pub fn new(reader: Arc<dyn ProfileReader>, config: Arc<dyn ConfigStore>) -> Self {
        Self { reader, config }
    }

    /// Imports the profile in `file` under the repository alias `repository`.
    pub async fn import(
        &self,
        file: &Path,
        repository: RepositoryAlias,
    ) -> Result<Imported, ImportError> {
        let loaded = self.config.load().await.map_err(ImportError::Config)?;
        if loaded.repository(&repository).is_some() {
            return Err(ImportError::AliasTaken(repository));
        }
        let profile = self
            .reader
            .read(file, &repository)
            .await
            .map_err(ImportError::Profile)?;

        let mut agent = loaded.agent().clone();
        let mut plans = loaded.plans().to_vec();
        let first_imported = plans.len();
        let ids: Vec<PlanId> = profile.plans.iter().map(|plan| plan.id.clone()).collect();
        plans.extend(profile.plans);
        apply_hostname_rule(&mut agent.hostname, &mut plans, first_imported);

        let mut hooks = loaded.hooks().clone();
        hooks.run_before.extend(profile.hooks.run_before);
        hooks.run_after.extend(profile.hooks.run_after);
        hooks.run_failed.extend(profile.hooks.run_failed);
        hooks.run_finally.extend(profile.hooks.run_finally);

        let mut repositories = loaded.repositories().clone();
        repositories.insert(repository.clone(), profile.repository);

        let saved =
            AgentConfig::new(agent, hooks, repositories, plans).map_err(
                |violation| match violation {
                    Violation::DuplicatePlan { plan, .. } => ImportError::PlanTaken(plan),
                    violation => ImportError::Config(ConfigError::Invalid {
                        file: String::new(),
                        key: None,
                        at: None,
                        problem: Problem::Violation(violation),
                        detail: String::new(),
                    }),
                },
            )?;
        self.config
            .save(&saved)
            .await
            .map_err(ImportError::Config)?;
        Ok(Imported {
            repository,
            plans: ids,
            notices: profile.notices,
        })
    }
}

/// Applies the hostname rule of the module to the imported plans, which are
/// `plans[first_imported..]`.
fn apply_hostname_rule(agent: &mut Option<Hostname>, plans: &mut [Plan], first_imported: usize) {
    let (existing, imported) = plans.split_at_mut(first_imported);
    if agent.is_none() && existing.is_empty() {
        let common = imported.first().and_then(|plan| plan.host.clone());
        if common.is_some() && imported.iter().all(|plan| plan.host == common) {
            *agent = common;
        }
    }
    for plan in imported {
        if plan.host.is_some() && plan.host == *agent {
            plan.host = None;
        }
    }
}

#[allow(clippy::result_large_err)]
impl ExportProfile {
    pub fn new(
        writer: Arc<dyn ProfileWriter>,
        config: Arc<dyn ConfigStore>,
        agent: AgentId,
    ) -> Self {
        Self {
            writer,
            config,
            agent,
        }
    }

    /// The profile of the plan `plan`.
    pub async fn export(&self, plan: &PlanId) -> Result<ExportedProfile, ExportError> {
        let config = self.config.load().await.map_err(ExportError::Config)?;
        let found = config
            .plan(plan)
            .ok_or_else(|| ExportError::PlanUnknown(plan.clone()))?;
        // A loaded configuration has the repository of every plan; a store
        // that gave one without is reported as the configuration it is not.
        let repository = config.repository(&found.repository).ok_or_else(|| {
            ExportError::Config(ConfigError::Invalid {
                file: String::new(),
                key: None,
                at: None,
                problem: Problem::Violation(Violation::UnknownRepository {
                    index: 0,
                    plan: found.id.clone(),
                    repository: found.repository.clone(),
                }),
                detail: String::new(),
            })
        })?;
        let rendered = self
            .writer
            .render(&PlanExport {
                plan: found.clone(),
                repository: repository.clone(),
                agent_hooks: config.hooks().clone(),
                agent_hostname: config.agent().hostname.clone(),
                tags: PlanTags::new(found.id.clone(), self.agent),
            })
            .map_err(ExportError::Render)?;
        Ok(ExportedProfile {
            text: rendered.text,
            notices: rendered.notices,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, reason = "test code")]
mod tests {
    use std::collections::BTreeMap;

    use domain::planning::{
        AgentSettings, HookEntry, Hooks, RepositoryConfig, RepositorySettings, Source,
    };
    use domain::secrets::SecretRef;

    use super::*;
    use crate::fakes::{FakeProfileReader, InMemoryConfigStore};
    use crate::ports::ImportedProfile;

    fn alias(text: &str) -> RepositoryAlias {
        RepositoryAlias::parse(text).expect("an alias")
    }

    fn host(text: &str) -> Hostname {
        Hostname::parse(text).expect("a hostname")
    }

    fn repository() -> RepositoryConfig {
        RepositoryConfig {
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
        }
    }

    fn plan(id: &str, on: Option<&str>) -> Plan {
        let mut plan = Plan::new(
            PlanId::parse(id).expect("an ID"),
            alias("nas"),
            vec![Source::parse(&format!("/home/demo/{id}")).expect("a source")],
        );
        plan.host = on.map(host);
        plan
    }

    fn hook(text: &str) -> HookEntry {
        HookEntry::parse(text).expect("a hook")
    }

    fn profile(plans: Vec<Plan>, hooks: Hooks) -> ImportedProfile {
        ImportedProfile {
            repository: repository(),
            plans,
            hooks,
            notices: Vec::new(),
        }
    }

    #[allow(clippy::result_large_err)]
    async fn import(
        config: &Arc<InMemoryConfigStore>,
        profile: ImportedProfile,
    ) -> Result<Imported, ImportError> {
        ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(profile))),
            config.clone(),
        )
        .import(Path::new("nas.toml"), alias("nas"))
        .await
    }

    async fn saved(config: &InMemoryConfigStore) -> AgentConfig {
        config.load().await.expect("the configuration loads")
    }

    #[tokio::test]
    async fn every_kind_of_global_hook_is_appended_after_the_agents() {
        let config = Arc::new(InMemoryConfigStore::new());
        let own = Hooks {
            run_before: vec![hook("own-before")],
            run_after: vec![hook("own-after")],
            run_failed: vec![hook("own-failed")],
            run_finally: vec![hook("own-finally")],
        };
        config
            .save(
                &AgentConfig::new(AgentSettings::default(), own, BTreeMap::new(), Vec::new())
                    .expect("a configuration"),
            )
            .await
            .expect("saved");
        let theirs = Hooks {
            run_before: vec![hook("their-before")],
            run_after: vec![hook("their-after")],
            run_failed: vec![hook("their-failed")],
            run_finally: vec![hook("their-finally")],
        };
        import(&config, profile(vec![plan("home", None)], theirs))
            .await
            .expect("imported");
        let hooks = saved(&config).await.hooks().clone();
        let programs = |entries: &[HookEntry]| -> Vec<String> {
            entries.iter().map(|e| format!("{:?}", e.command)).collect()
        };
        assert_eq!(programs(&hooks.run_before).len(), 2);
        assert_eq!(programs(&hooks.run_after).len(), 2);
        assert_eq!(programs(&hooks.run_failed).len(), 2);
        assert_eq!(programs(&hooks.run_finally).len(), 2);
        assert!(programs(&hooks.run_before)[0].contains("own-before"));
        assert!(programs(&hooks.run_after)[1].contains("their-after"));
        assert!(programs(&hooks.run_failed)[0].contains("own-failed"));
        assert!(programs(&hooks.run_finally)[1].contains("their-finally"));
    }

    #[tokio::test]
    async fn a_plan_without_a_host_stops_the_agent_from_taking_the_others() {
        let config = Arc::new(InMemoryConfigStore::new());
        import(
            &config,
            profile(
                vec![plan("home", Some("demo-laptop")), plan("docs", None)],
                Hooks::default(),
            ),
        )
        .await
        .expect("imported");
        let saved = saved(&config).await;
        assert_eq!(saved.agent().hostname, None);
        assert_eq!(
            saved.plans()[0].host.as_ref().map(Hostname::as_str),
            Some("demo-laptop")
        );
    }

    #[tokio::test]
    async fn only_the_plans_on_the_agents_host_lose_theirs() {
        let config = Arc::new(InMemoryConfigStore::new());
        config
            .save(
                &AgentConfig::new(
                    AgentSettings {
                        hostname: Some(host("demo-laptop")),
                        ..AgentSettings::default()
                    },
                    Hooks::default(),
                    BTreeMap::new(),
                    Vec::new(),
                )
                .expect("a configuration"),
            )
            .await
            .expect("saved");
        import(
            &config,
            profile(
                vec![
                    plan("home", Some("demo-laptop")),
                    plan("docs", Some("demo-desktop")),
                ],
                Hooks::default(),
            ),
        )
        .await
        .expect("imported");
        let saved = saved(&config).await;
        assert_eq!(saved.plans()[0].host, None);
        assert_eq!(
            saved.plans()[1].host.as_ref().map(Hostname::as_str),
            Some("demo-desktop")
        );
    }

    #[tokio::test]
    async fn a_plan_for_another_repository_is_refused_and_nothing_is_saved() {
        let config = Arc::new(InMemoryConfigStore::new());
        let mut stray = plan("home", None);
        stray.repository = alias("elsewhere");
        let refused = import(&config, profile(vec![stray], Hooks::default())).await;
        assert!(matches!(
            refused,
            Err(ImportError::Config(ConfigError::Invalid {
                problem: Problem::Violation(Violation::UnknownRepository { .. }),
                ..
            }))
        ));
        assert!(saved(&config).await.repositories().is_empty());
    }

    #[tokio::test]
    async fn a_taken_alias_is_refused_before_the_profile_is_read() {
        let config = Arc::new(InMemoryConfigStore::new());
        import(&config, profile(vec![plan("home", None)], Hooks::default()))
            .await
            .expect("imported");
        let reader = Arc::new(FakeProfileReader::new(Ok(profile(
            vec![plan("docs", None)],
            Hooks::default(),
        ))));
        let refused = ImportProfile::new(reader.clone(), config.clone())
            .import(Path::new("nas.toml"), alias("nas"))
            .await;
        assert_eq!(refused, Err(ImportError::AliasTaken(alias("nas"))));
        assert!(reader.asked().is_empty());
    }

    #[tokio::test]
    async fn the_profile_is_read_once_for_the_alias_it_is_imported_as() {
        let config = Arc::new(InMemoryConfigStore::new());
        let reader = Arc::new(FakeProfileReader::new(Ok(profile(
            vec![plan("home", None)],
            Hooks::default(),
        ))));
        ImportProfile::new(reader.clone(), config.clone())
            .import(Path::new("nas.toml"), alias("nas"))
            .await
            .expect("imported");
        assert_eq!(
            reader.asked(),
            vec![(Path::new("nas.toml").to_path_buf(), alias("nas"))]
        );
    }
}
