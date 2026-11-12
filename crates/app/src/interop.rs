//! Profile interop use cases (technical design §6.2): importing a rustic
//! profile into the configuration, and exporting one plan as a profile the
//! stock `rustic` CLI accepts.
//!
//! The adapter translates rustic's format to and from Ladderback's types;
//! what the translation means for the configuration is decided here: a plan
//! ID the configuration already has is refused before anything is saved, and
//! so is a second alias for a repository it already holds (§7.3); the
//! hostname rule below.
//!
//! **Repository.** A profile whose repository the configuration already
//! holds, at the same location under the alias given, adds its plans to
//! that entry, which keeps its settings, ID and maintenance; a notice names
//! each setting of it the profile sets otherwise. Under another alias it is
//! refused, naming the alias it has; an alias the configuration holds for
//! another location is refused as taken. A location is the whole of it,
//! options included, credentials aside
//! ([`same_location`](domain::planning::RepositorySettings::same_location)).
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
use domain::planning::{AgentConfig, Hostname, Plan, RepositoryConfig, Violation};
use domain::{AgentId, PlanId, RepositoryAlias};

use crate::ports::{
    ConfigError, ConfigStore, ExportRefusal, PlanExport, Problem, ProfileError, ProfileNotice,
    ProfileReader, ProfileWriter,
};

/// Imports a rustic profile: reads it through the [`ProfileReader`], checks
/// it against the configuration, and saves the repository (unless the
/// configuration already holds it under that alias), the plans and the
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
        let profile = self
            .reader
            .read(file, &repository)
            .await
            .map_err(ImportError::Profile)?;
        // A profile knows no repository ID, so its location alone tells
        // whether the configuration already holds its repository. The entry
        // the alias names is looked at first: one location may hold two
        // entries whose IDs differ, and the alias says which is meant.
        let kept = match loaded.repository(&repository) {
            Some(entry) if entry.settings.same_location(&profile.repository.settings) => {
                Some(entry)
            }
            Some(_) => return Err(ImportError::AliasTaken(repository)),
            None => match loaded.alias_of(&profile.repository.settings, None) {
                Some(alias) => return Err(ImportError::AlreadyConfigured(alias.clone())),
                None => None,
            },
        };
        let mut notices = profile.notices;
        if let Some(entry) = kept {
            let differ = settings_kept(entry, &profile.repository);
            // The prune notice says the maintenance became local, which the
            // kept entry's is not. The entry is what is saved, not the
            // profile's repository: no secret the profile writes as text is
            // kept as text.
            if differ.contains(&"maintenance") {
                notices.retain(|notice| *notice != ProfileNotice::Prune);
            }
            notices.retain(|notice| {
                !matches!(
                    notice,
                    ProfileNotice::InlineSecret { .. } | ProfileNotice::LocationSecret { .. }
                )
            });
            notices.extend(differ.into_iter().map(|setting| {
                ProfileNotice::RepositorySettingKept {
                    repository: repository.as_str().to_owned(),
                    setting: setting.to_owned(),
                }
            }));
        }

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
        if kept.is_none() {
            repositories.insert(repository.clone(), profile.repository);
        }

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
            notices,
        })
    }
}

/// The settings of `entry` that `theirs`, a profile's repository at the same
/// location, sets otherwise, as the configuration names them, in the order
/// it writes them. Where it is and its ID are not among them: the location
/// is the same, and a profile knows no ID. An option set can differ only in
/// its credentials, which are not part of the location.
fn settings_kept(entry: &RepositoryConfig, theirs: &RepositoryConfig) -> Vec<&'static str> {
    let (ours, profile) = (&entry.settings, &theirs.settings);
    [
        ("password", ours.password != profile.password),
        ("maintenance", entry.maintenance != theirs.maintenance),
        ("group-by", entry.group_by != theirs.group_by),
        ("options", ours.options != profile.options),
        ("options-hot", ours.options_hot != profile.options_hot),
        ("options-cold", ours.options_cold != profile.options_cold),
        ("hooks", entry.hooks != theirs.hooks),
    ]
    .into_iter()
    .filter_map(|(setting, differs)| differs.then_some(setting))
    .collect()
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
                agent: config.agent().clone(),
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

    use domain::RepositoryId;
    use domain::planning::{
        AgentSettings, GroupBy, HookEntry, Hooks, Maintenance, RepositoryConfig,
        RepositorySettings, Source,
    };
    use domain::secrets::{SecretRef, SecretValue};

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

    /// The rules and channels of a configuration: one channel, and one rule
    /// that sends it every failed backup.
    fn notifications() -> domain::notifications::Notifications {
        use domain::notifications::{
            Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName,
        };
        let ops = ChannelName::parse("ops").expect("a channel name");
        Notifications::new(
            BTreeMap::from([(
                ops.clone(),
                Channel::new(
                    ChannelKind::Ntfy,
                    SecretRef::Env("DEMO_TOPIC_URL".to_owned()),
                    None,
                )
                .expect("a channel"),
            )]),
            BTreeMap::from([(
                RuleName::parse("failures").expect("a rule name"),
                Rule::new(
                    [EventKind::BackupFailed].into(),
                    ops,
                    Default::default(),
                    None,
                )
                .expect("a rule"),
            )]),
        )
        .expect("rules that name their channel")
    }

    #[tokio::test]
    async fn an_import_keeps_the_notification_rules_and_channels() {
        let config = Arc::new(InMemoryConfigStore::new());
        config
            .save(
                &AgentConfig::default()
                    .with_notifications(notifications())
                    .expect("rules for no plan in particular"),
            )
            .await
            .expect("saved");
        import(&config, profile(vec![plan("home", None)], Hooks::default()))
            .await
            .expect("imported");
        assert_eq!(saved(&config).await.notifications(), &notifications());
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

    /// The alias holds the profile's repository (the same location), so the
    /// import adds its plans to that entry instead of a second one (§7.3).
    #[tokio::test]
    async fn a_profile_for_the_repository_its_alias_holds_adds_its_plans_to_it() {
        let config = Arc::new(InMemoryConfigStore::new());
        import(&config, profile(vec![plan("home", None)], Hooks::default()))
            .await
            .expect("imported");
        let before = saved(&config).await.repositories().clone();
        let reader = Arc::new(FakeProfileReader::new(Ok(profile(
            vec![plan("docs", None)],
            Hooks::default(),
        ))));
        let imported = ImportProfile::new(reader, config.clone())
            .import(Path::new("nas.toml"), alias("nas"))
            .await
            .expect("the plans are added");
        assert_eq!(imported.plans, vec![PlanId::parse("docs").unwrap()]);
        let after = saved(&config).await;
        assert_eq!(after.repositories(), &before, "the entry is kept as it was");
        assert_eq!(after.plans().len(), 2);
    }

    fn in_bucket(bucket: &str) -> RepositoryConfig {
        let mut repository = repository();
        repository.settings.repository = "opendal:b2".to_owned();
        repository
            .settings
            .options
            .insert("bucket".to_owned(), bucket.to_owned());
        repository
    }

    async fn holding(entries: Vec<(&str, RepositoryConfig)>) -> Arc<InMemoryConfigStore> {
        let config = Arc::new(InMemoryConfigStore::new());
        let entries = entries
            .into_iter()
            .map(|(name, entry)| (alias(name), entry))
            .collect();
        config
            .save(
                &AgentConfig::new(
                    AgentSettings::default(),
                    Hooks::default(),
                    entries,
                    Vec::new(),
                )
                .expect("a configuration"),
            )
            .await
            .expect("saved");
        config
    }

    #[allow(clippy::result_large_err)]
    async fn import_as(
        config: &Arc<InMemoryConfigStore>,
        repository: RepositoryConfig,
        name: &str,
    ) -> Result<Imported, ImportError> {
        let mut profile = profile(vec![plan("home", None)], Hooks::default());
        profile.repository = repository;
        profile.plans[0].repository = alias(name);
        ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(profile))),
            config.clone(),
        )
        .import(Path::new("nas.toml"), alias(name))
        .await
    }

    /// An `opendal:` repository's bucket is part of where it is: the alias
    /// holds another repository than the profile's.
    #[tokio::test]
    async fn a_profile_for_another_bucket_under_a_taken_alias_is_refused_and_nothing_is_saved() {
        let config = holding(vec![("nas", in_bucket("photos"))]).await;
        let refused = import_as(&config, in_bucket("docs"), "nas").await;
        assert_eq!(refused, Err(ImportError::AliasTaken(alias("nas"))));
        let saved = saved(&config).await;
        assert_eq!(saved.repository(&alias("nas")), Some(&in_bucket("photos")));
        assert!(saved.plans().is_empty());
    }

    #[tokio::test]
    async fn a_profile_for_another_bucket_under_a_new_alias_is_added() {
        let config = holding(vec![("photos", in_bucket("photos"))]).await;
        import_as(&config, in_bucket("docs"), "docs")
            .await
            .expect("another repository");
        let saved = saved(&config).await;
        assert_eq!(saved.repository(&alias("docs")), Some(&in_bucket("docs")));
        assert_eq!(saved.plans().len(), 1);
    }

    /// The profile knows no ID, so the entry that knows one is found by its
    /// location, and keeps its ID.
    #[tokio::test]
    async fn a_profile_for_an_entry_with_an_id_adds_its_plans_to_it() {
        let mut entry = repository();
        entry.id = Some(RepositoryId::parse(&"07".repeat(32)).expect("an ID"));
        let config = holding(vec![("nas", entry.clone())]).await;
        import_as(&config, repository(), "nas")
            .await
            .expect("the plans are added");
        let saved = saved(&config).await;
        assert_eq!(saved.repository(&alias("nas")), Some(&entry));
        assert_eq!(saved.plans().len(), 1);
    }

    /// One location may hold two entries whose IDs differ: the repository
    /// there was replaced and connected again under another alias. The
    /// alias the profile is imported as is looked at first, so the import
    /// adds its plans to the entry it names.
    #[tokio::test]
    async fn a_profile_imported_as_the_second_of_two_entries_at_its_location_adds_its_plans_to_it()
    {
        let with_id = |id: &str| {
            let mut entry = repository();
            entry.id = Some(RepositoryId::parse(&id.repeat(32)).expect("an ID"));
            entry
        };
        let config = holding(vec![("nas", with_id("07")), ("nas2", with_id("08"))]).await;
        import_as(&config, repository(), "nas2")
            .await
            .expect("the plans are added");
        let saved = saved(&config).await;
        assert_eq!(saved.repository(&alias("nas2")), Some(&with_id("08")));
        assert_eq!(saved.repository(&alias("nas")), Some(&with_id("07")));
        assert_eq!(saved.plans().len(), 1);
    }

    fn kept(setting: &str) -> ProfileNotice {
        ProfileNotice::RepositorySettingKept {
            repository: "nas".to_owned(),
            setting: setting.to_owned(),
        }
    }

    #[allow(clippy::result_large_err)]
    async fn import_over(
        entry: &RepositoryConfig,
        theirs: RepositoryConfig,
        notices: Vec<ProfileNotice>,
    ) -> (Imported, AgentConfig) {
        let config = holding(vec![("nas", entry.clone())]).await;
        let mut profile = profile(vec![plan("home", None)], Hooks::default());
        profile.repository = theirs;
        profile.notices = notices;
        let imported = ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(profile))),
            config.clone(),
        )
        .import(Path::new("nas.toml"), alias("nas"))
        .await
        .expect("the plans are added");
        (imported, saved(&config).await)
    }

    /// The entry is kept whole, and each of its settings the profile sets
    /// otherwise is named. The profile's prune notice said the
    /// maintenance became local, which the kept entry's is not: it goes.
    #[tokio::test]
    async fn a_profile_that_sets_the_repository_up_otherwise_keeps_the_entry_and_names_each_setting()
     {
        let mut entry = repository();
        entry.maintenance = Maintenance::Hub;
        let mut theirs = repository();
        theirs.settings.password = SecretRef::Env("OTHER_PASSWORD".to_owned());
        theirs.maintenance = Maintenance::Local;
        theirs.group_by = GroupBy::parse("host").expect("a grouping");
        theirs.hooks.run_before = vec![hook("their-before")];
        for set in [
            &mut theirs.settings.options,
            &mut theirs.settings.options_hot,
            &mut theirs.settings.options_cold,
        ] {
            set.insert("sas_token".to_owned(), "not-a-real-token".to_owned());
        }
        let (imported, saved) = import_over(
            &entry,
            theirs,
            vec![ProfileNotice::Prune, ProfileNotice::EnvIgnored],
        )
        .await;
        assert_eq!(
            imported.notices,
            [
                ProfileNotice::EnvIgnored,
                kept("password"),
                kept("maintenance"),
                kept("group-by"),
                kept("options"),
                kept("options-hot"),
                kept("options-cold"),
                kept("hooks"),
            ]
        );
        assert_eq!(saved.repository(&alias("nas")), Some(&entry));
        assert_eq!(saved.plans().len(), 1);
    }

    /// The same maintenance: the prune notice is still true, and stays.
    #[tokio::test]
    async fn a_profile_that_sets_the_repository_up_alike_names_nothing_kept() {
        let mut entry = repository();
        entry.maintenance = Maintenance::Local;
        let (imported, _) = import_over(&entry, entry.clone(), vec![ProfileNotice::Prune]).await;
        assert_eq!(imported.notices, [ProfileNotice::Prune]);
    }

    /// The kept entry is the one saved, not the profile's repository:
    /// nothing the profile holds as text is kept as text, so none of its
    /// `inline-secret` notices is said, and only the password kept is named.
    /// The entry's own secrets written as text are said when the
    /// configuration loads (`config-inline-secret`).
    #[tokio::test]
    async fn a_profile_secret_written_as_text_is_not_said_kept_as_text_when_the_entry_is_kept() {
        let mut entry = repository();
        entry
            .settings
            .options
            .insert("secret_access_key".to_owned(), "not-a-real-key".to_owned());
        let mut theirs = entry.clone();
        theirs.settings.password = SecretRef::Inline(SecretValue::new("not-a-real-password"));
        let (imported, saved) = import_over(
            &entry,
            theirs,
            vec![
                ProfileNotice::InlineSecret {
                    field: "repository.password".to_owned(),
                },
                ProfileNotice::InlineSecret {
                    field: "repository.options.secret_access_key".to_owned(),
                },
            ],
        )
        .await;
        assert_eq!(imported.notices, [kept("password")]);
        assert_eq!(saved.repository(&alias("nas")), Some(&entry));
    }

    /// A kept entry is at the profile's location, so a password in that
    /// location is the entry's own, said when the configuration loads; the
    /// import says nothing of it.
    #[tokio::test]
    async fn a_password_in_the_location_is_not_said_kept_as_text_when_the_entry_is_kept() {
        let mut entry = repository();
        entry.settings.repository = "rest:https://demo:not-real@backup.example.com/nas".to_owned();
        let (imported, saved) = import_over(
            &entry,
            entry.clone(),
            vec![ProfileNotice::LocationSecret {
                field: "repository.repository".to_owned(),
                rclone: false,
            }],
        )
        .await;
        assert_eq!(imported.notices, []);
        assert_eq!(saved.repository(&alias("nas")), Some(&entry));
    }

    /// An entry that reads its credential from the environment, as the
    /// `inline-secret` warning advises, beside a profile that writes it as
    /// text, or a profile with a credential the entry lacks.
    fn with_credential(value: Option<&str>) -> RepositoryConfig {
        let mut repository = in_bucket("photos");
        if let Some(value) = value {
            repository
                .settings
                .options
                .insert("secret_access_key".to_owned(), value.to_owned());
        }
        repository
    }

    /// A credential is how to open the repository, not where it is: the
    /// entry is still at the profile's location, so the plans are added to
    /// it and it keeps its own options.
    #[tokio::test]
    async fn a_profile_whose_credential_the_entry_reads_otherwise_adds_its_plans_to_it() {
        for (entry, theirs) in [
            (
                with_credential(Some("${AWS_SECRET_ACCESS_KEY}")),
                with_credential(Some("not-a-real-key")),
            ),
            (
                with_credential(None),
                with_credential(Some("not-a-real-key")),
            ),
        ] {
            let (imported, saved) = import_over(
                &entry,
                theirs,
                vec![ProfileNotice::InlineSecret {
                    field: "repository.options.secret_access_key".to_owned(),
                }],
            )
            .await;
            assert_eq!(imported.notices, [kept("options")]);
            assert_eq!(saved.repository(&alias("nas")), Some(&entry));
            assert_eq!(saved.plans().len(), 1);
        }
    }

    /// The credential changed and nothing else: the same repository, so a
    /// new alias for it is a second name.
    #[tokio::test]
    async fn a_profile_whose_credential_alone_differs_under_a_new_alias_is_refused() {
        let entry = with_credential(Some("${AWS_SECRET_ACCESS_KEY}"));
        let config = holding(vec![("nas", entry.clone())]).await;
        let refused = import_as(&config, with_credential(Some("not-a-real-key")), "backup").await;
        assert_eq!(refused, Err(ImportError::AlreadyConfigured(alias("nas"))));
        let saved = saved(&config).await;
        assert_eq!(saved.repositories().len(), 1);
        assert_eq!(saved.repository(&alias("nas")), Some(&entry));
        assert!(saved.plans().is_empty());
    }

    /// A Dropbox account at `root`: on a drive only the credential names
    /// the account, so it is part of the location.
    fn on_dropbox(token: &str) -> RepositoryConfig {
        let mut repository = repository();
        repository.settings.repository = "opendal:dropbox".to_owned();
        let options = &mut repository.settings.options;
        options.insert("root".to_owned(), "/ladderback".to_owned());
        options.insert("access_token".to_owned(), token.to_owned());
        repository
    }

    /// Another account's drive at the same `root` is another repository:
    /// under the alias of the first it is refused as taken, never added to
    /// it, and under a new alias it is added beside it.
    #[tokio::test]
    async fn a_profile_for_another_account_on_a_drive_is_another_repository() {
        let home = on_dropbox("${DROPBOX_TOKEN}");
        let config = holding(vec![("nas", home.clone())]).await;
        let refused = import_as(&config, on_dropbox("not-a-real-token"), "nas").await;
        assert_eq!(refused, Err(ImportError::AliasTaken(alias("nas"))));
        let unchanged = saved(&config).await;
        assert_eq!(unchanged.repository(&alias("nas")), Some(&home));
        assert!(unchanged.plans().is_empty());

        import_as(&config, on_dropbox("not-a-real-token"), "work")
            .await
            .expect("another repository");
        let saved = saved(&config).await;
        assert_eq!(saved.repository(&alias("nas")), Some(&home));
        assert_eq!(
            saved.repository(&alias("work")),
            Some(&on_dropbox("not-a-real-token"))
        );
        assert_eq!(saved.plans().len(), 1);
    }

    /// The same password, written as text in both: the profile's is still
    /// not the one saved, and nothing is said of it. The configuration's
    /// own is said when it loads (`config-inline-secret`).
    #[tokio::test]
    async fn the_same_password_written_as_text_in_both_is_not_said_kept_as_text() {
        let mut entry = repository();
        entry.settings.password = SecretRef::Inline(SecretValue::new("not-a-real-password"));
        let (imported, _) = import_over(
            &entry,
            entry.clone(),
            vec![ProfileNotice::InlineSecret {
                field: "repository.password".to_owned(),
            }],
        )
        .await;
        assert_eq!(imported.notices, []);
    }

    /// The ID is what Ladderback read when it opened the repository, which
    /// a profile can't know: it is not a setting the profile sets otherwise.
    #[tokio::test]
    async fn the_entrys_id_is_not_a_setting_kept() {
        let mut entry = repository();
        entry.id = Some(RepositoryId::parse(&"07".repeat(32)).expect("an ID"));
        let (imported, _) = import_over(&entry, repository(), Vec::new()).await;
        assert_eq!(imported.notices, []);
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
