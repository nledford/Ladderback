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
use domain::planning::{
    AgentConfig, Hooks, Hostname, LocationProgram, Plan, RepositoryConfig, RepositoryField,
    Violation,
};
use domain::secrets::{CommandLine, SecretRef, is_command_option};
use domain::{AgentId, PlanId, RepositoryAlias};

use crate::globs::never_matching_globs;
use crate::ports::{
    ConfigError, ConfigStore, ExportCommands, ExportRefusal, ImportedProfile, NormalizeSources,
    PlanExport, Problem, ProfileError, ProfileNotice, ProfileReader, ProfileWriter,
};

/// Imports a rustic profile: reads it through the [`ProfileReader`], checks
/// it against the configuration, and saves the repository (unless the
/// configuration already holds it under that alias), the plans and the
/// profile's global hooks in one save, or saves nothing. The plans' sources
/// are resolved through [`NormalizeSources`] only to read their globs as
/// the backup will (#510); they are saved as written.
pub struct ImportProfile {
    reader: Arc<dyn ProfileReader>,
    config: Arc<dyn ConfigStore>,
    sources: Arc<dyn NormalizeSources>,
}

/// What an import added to the configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub repository: RepositoryAlias,
    /// The IDs of the plans added, in the order of the profile.
    pub plans: Vec<PlanId>,
    /// The commands the import made runnable, in the order the configuration
    /// keeps them: the agent's hooks, the repository's, then each plan's.
    pub commands: Vec<ImportedCommand>,
    /// What the profile could not carry over, and what changed on the way.
    pub notices: Vec<ProfileNotice>,
}

/// A command an import makes runnable, and where the configuration keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedCommand {
    /// Its key in the configuration: `hooks.run-before`,
    /// `repositories.nas.password-command`,
    /// `repositories.nas.options.rclone-command`, `plans.home.hooks.run-after`.
    pub key: String,
    pub runs: Runs,
}

/// What a command an import makes runnable runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runs {
    /// A hook or the password's command: a program and its arguments.
    Command(CommandLine),
    /// An option rustic runs as a command (`rclone-command`), its value as
    /// the configuration keeps it: rustic splits it when it runs it.
    Option(String),
    /// A program rclone runs for the repository's location, named by a
    /// setting of its connection string.
    Location(LocationProgram),
}

/// Which commands the person agreed an import may make runnable (§6.2).
#[derive(Clone, Copy, Debug)]
pub enum Consent<'a> {
    /// Whichever the profile holds: the import was told not to ask.
    Any,
    /// Exactly these, the ones the person was shown and agreed to; none
    /// when nobody has been asked yet.
    Only(&'a [ImportedCommand]),
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
    /// The import would make these commands runnable, which are not the
    /// ones agreed to ([`Consent`]).
    CommandsUnconfirmed(Vec<ImportedCommand>),
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

#[expect(
    clippy::result_large_err,
    reason = "a typed code for the caller to match; none of its variants is large on its own"
)]
impl ImportProfile {
    pub fn new(
        reader: Arc<dyn ProfileReader>,
        config: Arc<dyn ConfigStore>,
        sources: Arc<dyn NormalizeSources>,
    ) -> Self {
        Self {
            reader,
            config,
            sources,
        }
    }

    /// Imports the profile in `file` under the repository alias
    /// `repository`, provided the commands it would make runnable are the
    /// ones `consent` agrees to.
    pub async fn import(
        &self,
        file: &Path,
        repository: RepositoryAlias,
        consent: Consent<'_>,
    ) -> Result<Imported, ImportError> {
        let loaded = self.config.load().await.map_err(ImportError::Config)?;
        let profile = self
            .reader
            .read(file, &repository)
            .await
            .map_err(ImportError::Profile)?;
        let (saved, imported) = self.merged(&loaded, profile, repository).await?;
        // What was agreed to is checked against what this very import would
        // save, read again after the person answered: a profile edited in
        // between is refused, naming what it runs now.
        if let Consent::Only(agreed) = consent
            && agreed != imported.commands.as_slice()
        {
            return Err(ImportError::CommandsUnconfirmed(imported.commands));
        }
        self.config
            .save(&saved)
            .await
            .map_err(ImportError::Config)?;
        Ok(imported)
    }

    /// What importing the profile whose TOML is `text` under the alias
    /// `repository` would add, checked as [`import`](Self::import) checks
    /// it, without saving anything. A refusal names the profile `name`. No
    /// file is read: the profiles it uses are notices (§4.6).
    pub async fn preview(
        &self,
        name: &str,
        text: &str,
        repository: RepositoryAlias,
    ) -> Result<Imported, ImportError> {
        let loaded = self.config.load().await.map_err(ImportError::Config)?;
        let profile = self
            .reader
            .read_text(name, text, &repository)
            .map_err(ImportError::Profile)?;
        let (_, imported) = self.merged(&loaded, profile, repository).await?;
        Ok(imported)
    }

    /// The configuration `loaded` with `profile` added under `repository`,
    /// and what that adds.
    async fn merged(
        &self,
        loaded: &AgentConfig,
        profile: ImportedProfile,
        repository: RepositoryAlias,
    ) -> Result<(AgentConfig, Imported), ImportError> {
        // A profile knows no repository ID, so its location alone tells
        // whether the configuration already holds its repository, at any of
        // the entry's locations (decision 80). The entry the alias names is
        // looked at first: one location may hold two entries whose IDs
        // differ, and the alias says which is meant.
        let kept = match loaded.repository(&repository) {
            Some(entry) if entry.is(&profile.repository.settings, None) => Some(entry),
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

        // A glob rustic reads from the root and that reaches none of the
        // folders the plan's backup walks is kept as written, as rustic
        // would read it, and said (#510).
        for plan in &profile.plans {
            let said = never_matching_globs(plan, self.sources.as_ref()).await;
            notices.extend(
                said.into_iter()
                    .map(|glob| ProfileNotice::GlobNeverMatches {
                        plan: plan.id.as_str().to_owned(),
                        glob: glob.glob,
                        absolute: glob.absolute,
                    }),
            );
        }

        // The global hooks run around every job of the agent: in a
        // configuration that already has plans or repositories, they would
        // run around jobs the profile never had, so they are left out (§6.2).
        let mut commands = Vec::new();
        let mut hooks = loaded.hooks().clone();
        if loaded.plans().is_empty() && loaded.repositories().is_empty() {
            push_hooks(&mut commands, "hooks", &profile.hooks);
            hooks.run_before.extend(profile.hooks.run_before);
            hooks.run_after.extend(profile.hooks.run_after);
            hooks.run_failed.extend(profile.hooks.run_failed);
            hooks.run_finally.extend(profile.hooks.run_finally);
        } else if !profile.hooks.is_empty() {
            notices.push(ProfileNotice::GlobalHooksLeftOut);
        }

        let mut repositories = loaded.repositories().clone();
        if kept.is_none() {
            push_repository(&mut commands, &repository, &profile.repository);
            repositories.insert(repository.clone(), profile.repository);
        }
        for plan in &profile.plans {
            push_hooks(
                &mut commands,
                &format!("plans.{}.hooks", plan.id.as_str()),
                &plan.hooks,
            );
        }
        notices.extend(
            commands
                .iter()
                .map(|command| ProfileNotice::CommandImported {
                    key: command.key.clone(),
                    program: command.runs.program(),
                }),
        );
        // A rustic profile has no schedule: rustic is started from outside
        // it. A plan without one runs only when started, which is said, so
        // nothing reads as fine when nothing will run (#1610).
        notices.extend(
            profile
                .plans
                .iter()
                .filter(|plan| plan.schedule.is_none())
                .map(|plan| ProfileNotice::PlanUnscheduled {
                    plan: plan.id.as_str().to_owned(),
                }),
        );

        let mut agent = loaded.agent().clone();
        let mut plans = loaded.plans().to_vec();
        let first_imported = plans.len();
        let ids: Vec<PlanId> = profile.plans.iter().map(|plan| plan.id.clone()).collect();
        plans.extend(profile.plans);
        apply_hostname_rule(&mut agent.hostname, &mut plans, first_imported);

        let saved = loaded
            .replacing(agent, hooks, repositories, plans)
            .map_err(|violation| match violation {
                Violation::DuplicatePlan { plan, .. } => ImportError::PlanTaken(plan),
                violation => ImportError::Config(ConfigError::Invalid {
                    file: String::new(),
                    key: None,
                    at: None,
                    problem: Problem::Violation(violation),
                    detail: String::new(),
                }),
            })?;
        Ok((
            saved,
            Imported {
                repository,
                plans: ids,
                commands,
                notices,
            },
        ))
    }
}

impl Runs {
    /// The program the command starts: for an option, the first word of
    /// its value as rustic splits it, or, when it can't be split, its first
    /// word at white space, never the rest (#856).
    pub fn program(&self) -> String {
        match self {
            Self::Command(line) => line.program().to_owned(),
            Self::Option(value) => CommandLine::parse(value).map_or_else(
                |_| {
                    value
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                },
                |line| line.program().to_owned(),
            ),
            Self::Location(found) => found.program.clone(),
        }
    }
}

/// Adds the commands of `hooks` to `commands`, each list under `at`.
fn push_hooks(commands: &mut Vec<ImportedCommand>, at: &str, hooks: &Hooks) {
    let lists = [
        ("run-before", &hooks.run_before),
        ("run-after", &hooks.run_after),
        ("run-failed", &hooks.run_failed),
        ("run-finally", &hooks.run_finally),
    ];
    for (list, entries) in lists {
        commands.extend(entries.iter().map(|entry| ImportedCommand {
            key: format!("{at}.{list}"),
            runs: Runs::Command(entry.command.clone()),
        }));
    }
}

/// Adds the commands a new repository entry runs to `commands`: the
/// programs its locations name, its password's command, the options rustic
/// runs as commands, and its hooks.
fn push_repository(
    commands: &mut Vec<ImportedCommand>,
    alias: &RepositoryAlias,
    entry: &RepositoryConfig,
) {
    let at = format!("repositories.{}", alias.as_str());
    let settings = &entry.settings;
    for (field, found) in settings.location_programs() {
        let field = match field {
            RepositoryField::Repository => "repository",
            RepositoryField::RepoHot => "repo-hot",
            // Only a location names a program rclone runs.
            RepositoryField::PasswordCommand
            | RepositoryField::Option(_)
            | RepositoryField::OptionHot(_)
            | RepositoryField::OptionCold(_) => continue,
        };
        commands.push(ImportedCommand {
            key: format!("{at}.{field}"),
            runs: Runs::Location(found),
        });
    }
    if let SecretRef::Command(line) = &settings.password {
        commands.push(ImportedCommand {
            key: format!("{at}.password-command"),
            runs: Runs::Command(line.clone()),
        });
    }
    let tables = [
        ("options", &settings.options),
        ("options-hot", &settings.options_hot),
        ("options-cold", &settings.options_cold),
    ];
    for (table, options) in tables {
        commands.extend(
            options
                .iter()
                .filter(|(name, _)| is_command_option(name))
                .map(|(name, value)| ImportedCommand {
                    key: format!("{at}.{table}.{name}"),
                    runs: Runs::Option(value.clone()),
                }),
        );
    }
    push_hooks(commands, &format!("{at}.hooks"), &entry.hooks);
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

#[expect(
    clippy::result_large_err,
    reason = "a typed code for the caller to match; none of its variants is large on its own"
)]
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

    /// The profile of the plan `plan`, its commands written or left out as
    /// `commands` says.
    pub async fn export(
        &self,
        plan: &PlanId,
        commands: ExportCommands,
    ) -> Result<ExportedProfile, ExportError> {
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
                commands,
            })
            .map_err(ExportError::Render)?;
        Ok(ExportedProfile {
            text: rendered.text,
            notices: rendered.notices,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    use std::collections::BTreeMap;

    use domain::RepositoryId;
    use domain::planning::{
        AgentSettings, GroupBy, HookEntry, Hooks, Maintenance, RepositoryConfig,
        RepositorySettings, Source,
    };
    use domain::secrets::{SecretRef, SecretValue};

    use super::*;
    use crate::fakes::{FakeProfileReader, FakeSources, InMemoryConfigStore};
    use crate::ports::{ImportedProfile, SourcesError};

    fn alias(text: &str) -> RepositoryAlias {
        RepositoryAlias::parse(text).expect("an alias")
    }

    fn host(text: &str) -> Hostname {
        Hostname::parse(text).expect("a hostname")
    }

    fn repository() -> RepositoryConfig {
        RepositoryConfig {
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

    #[expect(clippy::result_large_err, reason = "the use case's own error")]
    async fn import(
        config: &Arc<InMemoryConfigStore>,
        profile: ImportedProfile,
    ) -> Result<Imported, ImportError> {
        ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(profile))),
            config.clone(),
            Arc::new(FakeSources),
        )
        .import(Path::new("nas.toml"), alias("nas"), Consent::Any)
        .await
    }

    async fn saved(config: &InMemoryConfigStore) -> AgentConfig {
        config.load().await.expect("the configuration loads")
    }

    /// rustic reads a glob with a slash in the middle from the root of the
    /// file system: written from the source, it matches nothing, and the
    /// import says so for each, after the profile's own notices, and keeps
    /// it as written (#510).
    #[tokio::test]
    async fn an_imported_glob_that_can_never_match_is_kept_and_warned_of() {
        let config = Arc::new(InMemoryConfigStore::default());
        let mut home = plan("home", None);
        home.filter.globs = vec!["!.cache".to_owned(), "!.local/share/x".to_owned()];
        home.filter.iglobs = vec!["!**/a/b".to_owned(), "c/d".to_owned()];
        let mut profile = profile(vec![home.clone(), plan("docs", None)], Hooks::default());
        profile.notices = vec![ProfileNotice::EnvIgnored];
        let imported = import(&config, profile).await.expect("the import");
        assert_eq!(
            imported.notices,
            [
                ProfileNotice::EnvIgnored,
                ProfileNotice::GlobNeverMatches {
                    plan: "home".to_owned(),
                    glob: "!.local/share/x".to_owned(),
                    absolute: Some("!/home/demo/home/.local/share/x".to_owned()),
                },
                ProfileNotice::GlobNeverMatches {
                    plan: "home".to_owned(),
                    glob: "c/d".to_owned(),
                    absolute: Some("/home/demo/home/c/d".to_owned()),
                },
                unscheduled("home"),
                unscheduled("docs"),
            ]
        );
        let saved = saved(&config).await;
        assert_eq!(saved.plans().first(), Some(&home));
    }

    /// A source is read as the backup walks it, its links followed: the
    /// globs are read against the folder walked, and the whole path is
    /// written under it (#510). The plan keeps its sources as written.
    #[tokio::test]
    async fn an_imported_glob_is_read_against_the_sources_as_the_backup_walks_them() {
        let config = Arc::new(InMemoryConfigStore::default());
        let mut home = plan("home", None);
        home.filter.globs = vec![
            "!home/demo/home/.cache".to_owned(),
            "!var/home/demo/home/.cache".to_owned(),
        ];
        let imported = ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(profile(
                vec![home.clone()],
                Hooks::default(),
            )))),
            config.clone(),
            Arc::new(HomeUnderVar),
        )
        .import(Path::new("nas.toml"), alias("nas"), Consent::Any)
        .await
        .expect("the import");
        assert_eq!(
            imported.notices,
            [
                ProfileNotice::GlobNeverMatches {
                    plan: "home".to_owned(),
                    glob: "!home/demo/home/.cache".to_owned(),
                    absolute: Some("!/var/home/demo/home/.cache".to_owned()),
                },
                unscheduled("home")
            ]
        );
        assert_eq!(saved(&config).await.plans().first(), Some(&home));
    }

    /// Sources as a machine resolves them whose `/home` is a link to
    /// `/var/home`.
    struct HomeUnderVar;

    #[async_trait::async_trait]
    impl NormalizeSources for HomeUnderVar {
        async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
            Ok(sources
                .iter()
                .map(|source| Source::parse(&format!("/var{}", source.as_str())).expect("a source"))
                .collect())
        }
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
    async fn an_import_keeps_the_notifications_and_what_ai_agents_may_do_without_a_token() {
        let no_backup_runs = domain::access::permissions::Permissions::default()
            .with(domain::access::permissions::Permission::BackupRuns, None)
            .expect("a level the row has");
        let config = Arc::new(InMemoryConfigStore::new());
        config
            .save(
                &AgentConfig::default()
                    .with_notifications(notifications())
                    .expect("rules for no plan in particular")
                    .with_without_token(no_backup_runs),
            )
            .await
            .expect("saved");
        import(&config, profile(vec![plan("home", None)], Hooks::default()))
            .await
            .expect("imported");
        let saved = saved(&config).await;
        assert_eq!(saved.notifications(), &notifications());
        assert_eq!(saved.without_token(), no_backup_runs);
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
        assert_matches!(
            refused,
            Err(ImportError::Config(ConfigError::Invalid {
                problem: Problem::Violation(Violation::UnknownRepository { .. }),
                ..
            }))
        );
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
        let imported = ImportProfile::new(reader, config.clone(), Arc::new(FakeSources))
            .import(Path::new("nas.toml"), alias("nas"), Consent::Any)
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

    #[expect(clippy::result_large_err, reason = "the use case's own error")]
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
            Arc::new(FakeSources),
        )
        .import(Path::new("nas.toml"), alias(name), Consent::Any)
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

    /// What an import says of a plan it added without a schedule, as every
    /// profile's plan is (#1610).
    fn unscheduled(plan: &str) -> ProfileNotice {
        ProfileNotice::PlanUnscheduled {
            plan: plan.to_owned(),
        }
    }

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
            Arc::new(FakeSources),
        )
        .import(Path::new("nas.toml"), alias("nas"), Consent::Any)
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
                unscheduled("home"),
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
        assert_eq!(
            imported.notices,
            [ProfileNotice::Prune, unscheduled("home")]
        );
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
        assert_eq!(imported.notices, [kept("password"), unscheduled("home")]);
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
        assert_eq!(imported.notices, [unscheduled("home")]);
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
            assert_eq!(imported.notices, [kept("options"), unscheduled("home")]);
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
        assert_eq!(imported.notices, [unscheduled("home")]);
    }

    /// The ID is what Ladderback read when it opened the repository, which
    /// a profile can't know: it is not a setting the profile sets otherwise.
    #[tokio::test]
    async fn the_entrys_id_is_not_a_setting_kept() {
        let mut entry = repository();
        entry.id = Some(RepositoryId::parse(&"07".repeat(32)).expect("an ID"));
        let (imported, _) = import_over(&entry, repository(), Vec::new()).await;
        assert_eq!(imported.notices, [unscheduled("home")]);
    }

    #[tokio::test]
    async fn the_profile_is_read_once_for_the_alias_it_is_imported_as() {
        let config = Arc::new(InMemoryConfigStore::new());
        let reader = Arc::new(FakeProfileReader::new(Ok(profile(
            vec![plan("home", None)],
            Hooks::default(),
        ))));
        ImportProfile::new(reader.clone(), config.clone(), Arc::new(FakeSources))
            .import(Path::new("nas.toml"), alias("nas"), Consent::Any)
            .await
            .expect("imported");
        assert_eq!(
            reader.asked(),
            vec![(Path::new("nas.toml").to_path_buf(), alias("nas"))]
        );
    }

    /// A preview checks the profile as an import does and tells what it
    /// would add, with the reader's notices, yet saves nothing (§4.6).
    #[tokio::test]
    async fn a_preview_tells_what_an_import_would_add_and_saves_nothing() {
        let config = Arc::new(InMemoryConfigStore::new());
        let mut read = profile(vec![plan("home", None)], Hooks::default());
        read.notices = vec![ProfileNotice::UsedProfileNotRead {
            name: "shared".to_owned(),
        }];
        let reader = Arc::new(FakeProfileReader::new(Ok(read)));
        let before = saved(&config).await;
        let imported = ImportProfile::new(reader.clone(), config.clone(), Arc::new(FakeSources))
            .preview("content", "[repository]", alias("nas"))
            .await
            .expect("previewed");
        assert_eq!(imported.repository, alias("nas"));
        assert_eq!(imported.plans, vec![PlanId::parse("home").expect("an ID")]);
        assert_eq!(
            imported.notices,
            vec![
                ProfileNotice::UsedProfileNotRead {
                    name: "shared".to_owned()
                },
                unscheduled("home")
            ]
        );
        assert_eq!(saved(&config).await, before);
        assert_eq!(
            reader.asked(),
            vec![(Path::new("content").to_path_buf(), alias("nas"))]
        );
    }

    #[tokio::test]
    async fn only_a_plan_without_a_schedule_is_said_to_have_none() {
        let config = Arc::new(InMemoryConfigStore::new());
        let mut scheduled = plan("docs", None);
        scheduled.schedule = Some(domain::planning::Schedule::new(
            domain::planning::Interval::new(domain::SignedDuration::from_hours(6))
                .expect("an interval"),
        ));
        let read = profile(vec![plan("home", None), scheduled], Hooks::default());
        let imported = ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(read))),
            config,
            Arc::new(FakeSources),
        )
        .preview("content", "[repository]", alias("nas"))
        .await
        .expect("previewed");
        assert_eq!(imported.notices, [unscheduled("home")]);
    }

    fn line(text: &str) -> CommandLine {
        CommandLine::parse(text).expect("a command line")
    }

    fn command(key: &str, text: &str) -> ImportedCommand {
        ImportedCommand {
            key: key.to_owned(),
            runs: Runs::Command(line(text)),
        }
    }

    /// An `rclone-command` with a program in one of its flags.
    const RCLONE: &str = "rclone serve restic --stdio --password-command 'pass show rclone'";

    /// A profile that runs a command in every place one can be: its global
    /// hooks, the repository's password, an option of each of two tables and
    /// the repository's hooks, and its plan's hooks.
    fn running_commands() -> ImportedProfile {
        let mut repository = repository();
        repository.settings.password = SecretRef::Command(line("pass show demo/nas"));
        let options = &mut repository.settings.options;
        options.insert("rclone-command".to_owned(), RCLONE.to_owned());
        options.insert("connections".to_owned(), "4".to_owned());
        repository.settings.options_hot.insert(
            "post-create-command".to_owned(),
            "/usr/local/bin/notify created".to_owned(),
        );
        repository.hooks.run_after = vec![hook("curl -fsS https://hc.example/ping")];
        let mut home = plan("home", None);
        home.hooks.run_failed = vec![hook("notify-send failed")];
        home.hooks.run_finally = vec![hook("logger done"), hook("sync")];
        ImportedProfile {
            repository,
            plans: vec![home],
            hooks: Hooks {
                run_before: vec![hook("echo start")],
                ..Hooks::default()
            },
            notices: vec![ProfileNotice::EnvIgnored],
        }
    }

    /// What [`running_commands`] makes runnable in an empty configuration.
    fn commands_listed() -> Vec<ImportedCommand> {
        vec![
            command("hooks.run-before", "echo start"),
            command("repositories.nas.password-command", "pass show demo/nas"),
            ImportedCommand {
                key: "repositories.nas.options.rclone-command".to_owned(),
                runs: Runs::Option(RCLONE.to_owned()),
            },
            ImportedCommand {
                key: "repositories.nas.options-hot.post-create-command".to_owned(),
                runs: Runs::Option("/usr/local/bin/notify created".to_owned()),
            },
            command(
                "repositories.nas.hooks.run-after",
                "curl -fsS https://hc.example/ping",
            ),
            command("plans.home.hooks.run-failed", "notify-send failed"),
            command("plans.home.hooks.run-finally", "logger done"),
            command("plans.home.hooks.run-finally", "sync"),
        ]
    }

    fn imported_notice(key: &str, program: &str) -> ProfileNotice {
        ProfileNotice::CommandImported {
            key: key.to_owned(),
            program: program.to_owned(),
        }
    }

    #[expect(clippy::result_large_err, reason = "the use case's own error")]
    async fn import_with(
        config: &Arc<InMemoryConfigStore>,
        profile: ImportedProfile,
        consent: Consent<'_>,
    ) -> Result<Imported, ImportError> {
        ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(profile))),
            config.clone(),
            Arc::new(FakeSources),
        )
        .import(Path::new("nas.toml"), alias("nas"), consent)
        .await
    }

    /// Each command is listed in full where the configuration keeps it,
    /// in the order the configuration keeps them, and named by a notice
    /// after the profile's own, with the program it starts: for an
    /// option, the first word of its value. An option that runs nothing is
    /// not a command.
    #[tokio::test]
    async fn every_command_an_import_makes_runnable_is_listed_where_the_configuration_keeps_it() {
        let config = Arc::new(InMemoryConfigStore::new());
        let imported = import_with(&config, running_commands(), Consent::Any)
            .await
            .expect("imported");
        assert_eq!(imported.commands, commands_listed());
        assert_eq!(
            imported.notices,
            [
                ProfileNotice::EnvIgnored,
                imported_notice("hooks.run-before", "echo"),
                imported_notice("repositories.nas.password-command", "pass"),
                imported_notice("repositories.nas.options.rclone-command", "rclone"),
                imported_notice(
                    "repositories.nas.options-hot.post-create-command",
                    "/usr/local/bin/notify"
                ),
                imported_notice("repositories.nas.hooks.run-after", "curl"),
                imported_notice("plans.home.hooks.run-failed", "notify-send"),
                imported_notice("plans.home.hooks.run-finally", "logger"),
                imported_notice("plans.home.hooks.run-finally", "sync"),
                unscheduled("home"),
            ]
        );
    }

    #[tokio::test]
    async fn an_import_whose_commands_nobody_agreed_to_names_them_and_saves_nothing() {
        let config = Arc::new(InMemoryConfigStore::new());
        let before = saved(&config).await;
        let refused = import_with(&config, running_commands(), Consent::Only(&[])).await;
        assert_eq!(
            refused,
            Err(ImportError::CommandsUnconfirmed(commands_listed()))
        );
        assert_eq!(saved(&config).await, before);
    }

    #[tokio::test]
    async fn the_commands_agreed_to_are_imported() {
        let config = Arc::new(InMemoryConfigStore::new());
        let listed = commands_listed();
        let imported = import_with(&config, running_commands(), Consent::Only(&listed))
            .await
            .expect("imported");
        assert_eq!(imported.commands, listed);
        let saved = saved(&config).await;
        assert_eq!(saved.hooks().run_before, vec![hook("echo start")]);
        assert_eq!(saved.plans().len(), 1);
    }

    /// The profile changed after its commands were listed: what the person
    /// agreed to is not what would run.
    #[tokio::test]
    async fn commands_other_than_the_ones_agreed_to_save_nothing() {
        let config = Arc::new(InMemoryConfigStore::new());
        let before = saved(&config).await;
        let mut agreed = commands_listed();
        agreed.pop();
        let refused = import_with(&config, running_commands(), Consent::Only(&agreed)).await;
        assert_eq!(
            refused,
            Err(ImportError::CommandsUnconfirmed(commands_listed()))
        );
        assert_eq!(saved(&config).await, before);
    }

    /// rclone runs a program a location's connection string names on every
    /// job of the repository, as it would a command.
    #[tokio::test]
    async fn a_program_an_rclone_location_runs_is_listed_at_the_location() {
        let config = Arc::new(InMemoryConfigStore::new());
        let mut repository = repository();
        repository.settings.repository =
            "rclone::sftp,host=nas,ssh='ssh -i /home/demo/.ssh/nas demo@nas':/srv/repo".to_owned();
        let profile = ImportedProfile {
            repository,
            plans: vec![plan("home", None)],
            hooks: Hooks::default(),
            notices: Vec::new(),
        };
        let imported = import_with(&config, profile, Consent::Any)
            .await
            .expect("imported");
        assert_eq!(
            imported.commands,
            [ImportedCommand {
                key: "repositories.nas.repository".to_owned(),
                runs: Runs::Location(LocationProgram {
                    command: "ssh -i /home/demo/.ssh/nas demo@nas".to_owned(),
                    program: "ssh".to_owned(),
                }),
            }]
        );
        assert_eq!(
            imported.notices,
            [
                imported_notice("repositories.nas.repository", "ssh"),
                unscheduled("home")
            ]
        );
    }

    /// An option that can't be split as a command line names only its first
    /// word as the program, never what follows it (#856).
    #[test]
    fn an_option_that_cannot_be_split_names_only_its_first_word_as_the_program() {
        let runs = Runs::Option("rclone serve restic --stdio --webdav-pass 'hunter2".to_owned());
        assert_eq!(runs.program(), "rclone");
    }

    /// A configuration holding the repository `cloud`, at a location of
    /// its own, and the plans `plans` for it.
    async fn holding_cloud(plans: &[&str]) -> Arc<InMemoryConfigStore> {
        let config = Arc::new(InMemoryConfigStore::new());
        let mut cloud = repository();
        cloud.settings.repository = "/srv/cloud".to_owned();
        let plans = plans
            .iter()
            .map(|id| {
                let mut plan = plan(id, None);
                plan.repository = alias("cloud");
                plan
            })
            .collect();
        let own = AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            BTreeMap::from([(alias("cloud"), cloud)]),
            plans,
        )
        .expect("a configuration");
        config.save(&own).await.expect("saved");
        config
    }

    /// The global hooks would run around jobs that were there before the
    /// import: the plans' backups, and the checks, forgets and prunes of
    /// the repositories, whether or not a plan backs up into one.
    #[tokio::test]
    async fn global_hooks_are_left_out_of_a_configuration_that_has_plans_or_repositories() {
        for plans in [&["work"][..], &[]] {
            let config = holding_cloud(plans).await;
            let imported = import_with(&config, running_commands(), Consent::Any)
                .await
                .expect("imported");
            assert_eq!(saved(&config).await.hooks(), &Hooks::default(), "{plans:?}");
            assert_eq!(
                imported.commands.first(),
                commands_listed().get(1),
                "{plans:?}"
            );
            assert_eq!(
                imported.notices.get(..2),
                Some(&[ProfileNotice::EnvIgnored, ProfileNotice::GlobalHooksLeftOut][..]),
                "{plans:?}"
            );
        }
    }

    /// An entry the configuration already holds keeps its own password,
    /// options and hooks: the profile's make nothing runnable.
    #[tokio::test]
    async fn the_commands_of_a_repository_entry_kept_are_not_listed() {
        // The same location, set up otherwise: a password from the
        // environment, and no hooks.
        let mut own = running_commands().repository;
        own.settings.password = SecretRef::Env("DEMO_PASSWORD".to_owned());
        own.hooks = Hooks::default();
        let config = holding(vec![("nas", own)]).await;
        let imported = import_with(&config, running_commands(), Consent::Any)
            .await
            .expect("imported");
        assert_eq!(imported.commands, commands_listed().split_off(5));
    }

    /// A preview asks nobody: it lists what an import would make runnable.
    #[tokio::test]
    async fn a_preview_lists_the_commands_an_import_would_make_runnable() {
        let config = Arc::new(InMemoryConfigStore::new());
        let before = saved(&config).await;
        let imported = ImportProfile::new(
            Arc::new(FakeProfileReader::new(Ok(running_commands()))),
            config.clone(),
            Arc::new(FakeSources),
        )
        .preview("content", "[repository]", alias("nas"))
        .await
        .expect("previewed");
        assert_eq!(imported.commands, commands_listed());
        assert_eq!(saved(&config).await, before);
    }
}
