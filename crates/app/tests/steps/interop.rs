//! Steps for `features/interop/*.feature` that run against fakes: the use
//! cases decide what is saved and what is refused, so the scenarios drive
//! them with a reader that gives a scripted profile, a writer that records
//! what it was asked to render, and the in-memory config store. Rustic's
//! own format is the real adapter's, and is checked by the `@e2e` scenarios.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use app::fakes::{FakeProfileReader, FakeProfileWriter, FakeSources, InMemoryConfigStore};
use app::interop::{
    Consent, ExportError, ExportProfile, ExportedProfile, ImportError, ImportProfile, Imported,
    ImportedCommand, Runs,
};
use app::ports::{
    ConfigStore, ExportCommands, ExportRefusal, ImportedProfile, ProfileError, ProfileNotice,
    ProfileReader,
};
use cucumber::{given, then, when};
use domain::execution::plan_tags::PlanTags;
use domain::planning::{
    AgentConfig, AgentSettings, HookEntry, Hooks, Hostname, Maintenance, Plan, RepositoryConfig,
    RepositorySettings, Source,
};
use domain::secrets::{CommandLine, FileRead, SecretRef};
use domain::{AgentId, PlanId, RepositoryAlias, Uuid};

use super::Agent;

/// The configuration being drafted, the profile being scripted, and what the
/// use cases gave.
pub struct InteropWorld {
    store: Arc<InMemoryConfigStore>,
    reader: Arc<FakeProfileReader>,
    writer: Arc<FakeProfileWriter>,
    hostname: Option<Hostname>,
    agent_hooks: Hooks,
    repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
    plans: Vec<Plan>,
    /// What the scripted profile holds: its repository, when not the demo
    /// one, its plans, global hooks and notices, or that it can't be read.
    profile_repository: Option<RepositoryConfig>,
    profile_plans: Vec<Plan>,
    profile_hooks: Hooks,
    /// The global hooks the profile is changed to run once its commands
    /// have been listed, before the owner's confirmation reaches the import.
    changed_hooks: Option<Hooks>,
    profile_notices: Vec<ProfileNotice>,
    unreadable: bool,
    /// The configuration as the owner's command found it.
    before: Option<AgentConfig>,
    imported: Option<Result<Imported, ImportError>>,
    exported: Option<Result<ExportedProfile, ExportError>>,
}

impl Default for InteropWorld {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryConfigStore::new()),
            reader: Arc::new(FakeProfileReader::new(Err(ProfileError::Unreadable {
                file: String::new(),
            }))),
            writer: Arc::new(FakeProfileWriter::new()),
            hostname: None,
            agent_hooks: Hooks::default(),
            repositories: BTreeMap::new(),
            plans: Vec::new(),
            profile_repository: None,
            profile_plans: Vec::new(),
            profile_hooks: Hooks::default(),
            changed_hooks: None,
            profile_notices: Vec::new(),
            unreadable: false,
            before: None,
            imported: None,
            exported: None,
        }
    }
}

/// The agent every export in these scenarios is made for.
fn this_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0xa6e17))
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("a repository alias")
}

fn plan_id(id: &str) -> PlanId {
    PlanId::parse(id).expect("a plan ID")
}

fn hostname(name: &str) -> Hostname {
    Hostname::parse(name).expect("a hostname")
}

fn demo_repository() -> RepositoryConfig {
    RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings: RepositorySettings {
            repository: "/srv/demo-repo".to_owned(),
            repo_hot: None,
            // A profile can't hold a password written in the config.
            password: SecretRef::File {
                path: "/srv/demo-password".to_owned(),
                read: FileRead::FirstLine,
            },
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Default::default(),
        group_by: Default::default(),
        hooks: Hooks::default(),
    }
}

fn plan_for(id: &str, repository: &str) -> Plan {
    Plan::new(
        plan_id(id),
        alias(repository),
        vec![Source::parse(&format!("/home/demo/{id}")).expect("a source")],
    )
}

fn hooks_before(programs: &[&str]) -> Hooks {
    Hooks {
        run_before: programs
            .iter()
            .map(|program| HookEntry::parse(program).expect("a hook command"))
            .collect(),
        ..Hooks::default()
    }
}

fn programs_before(hooks: &Hooks) -> Vec<&str> {
    hooks
        .run_before
        .iter()
        .map(|entry| entry.command.program())
        .collect()
}

impl InteropWorld {
    /// Saves the drafted configuration into the store, once a command is
    /// about to run on it, and keeps it as the configuration it found.
    async fn saved(&mut self) -> AgentConfig {
        let config = AgentConfig::new(
            AgentSettings {
                hostname: self.hostname.clone(),
                ..AgentSettings::default()
            },
            self.agent_hooks.clone(),
            self.repositories.clone(),
            self.plans.clone(),
        )
        .expect("the drafted configuration is valid");
        self.store.save(&config).await.expect("the config saves");
        self.before = Some(config.clone());
        config
    }

    async fn now(&self) -> AgentConfig {
        self.store.load().await.expect("the config loads")
    }

    fn imported(&self) -> &Result<Imported, ImportError> {
        self.imported.as_ref().expect("the profile was imported")
    }

    fn exported(&self) -> &Result<ExportedProfile, ExportError> {
        self.exported.as_ref().expect("the plan was exported")
    }
}

#[given(expr = "a profile holding the plan {string}")]
fn profile_holds_plan(agent: &mut Agent, id: String) {
    agent.interop.profile_plans.push(plan_for(&id, "nas"));
}

#[given(expr = "a profile holding the plan {string} on the host {string}")]
fn profile_holds_plan_on_host(agent: &mut Agent, id: String, host: String) {
    let mut plan = plan_for(&id, "nas");
    plan.host = Some(hostname(&host));
    agent.interop.profile_plans.push(plan);
}

#[given("a profile that cannot be read")]
fn profile_unreadable(agent: &mut Agent) {
    agent.interop.unreadable = true;
}

#[given(expr = "the profile could not carry over the key {string}")]
fn profile_left_out(agent: &mut Agent, key: String) {
    agent
        .interop
        .profile_notices
        .push(ProfileNotice::KeyIgnored { key });
}

#[given(expr = "the profile's global hooks run {string} before everything")]
fn profile_hooks(agent: &mut Agent, program: String) {
    agent.interop.profile_hooks = hooks_before(&[&program]);
}

#[given(expr = "the profile reads the repository's password with the command {string}")]
fn profile_password_command(agent: &mut Agent, command: String) {
    let mut repository = demo_repository();
    repository.settings.password =
        SecretRef::Command(CommandLine::parse(&command).expect("a command line"));
    agent.interop.profile_repository = Some(repository);
}

#[given(
    expr = "the profile changes to run {string} before everything once its commands are listed"
)]
fn profile_changes(agent: &mut Agent, program: String) {
    agent.interop.changed_hooks = Some(hooks_before(&[&program]));
}

/// A repository the configuration holds, at a location of its own: not the
/// profile's, which is another repository.
fn configured_repository(name: &str) -> RepositoryConfig {
    let mut repository = demo_repository();
    repository.settings.repository = format!("/srv/{name}");
    repository
}

#[given(expr = "the configuration holds the repository {string}")]
fn holds_repository(agent: &mut Agent, name: String) {
    agent
        .interop
        .repositories
        .insert(alias(&name), configured_repository(&name));
}

#[given(expr = "the configuration holds the repository {string} at the profile's location")]
fn holds_profile_repository(agent: &mut Agent, name: String) {
    agent
        .interop
        .repositories
        .insert(alias(&name), demo_repository());
}

#[given(
    expr = "the configuration holds the repository {string} at the profile's location, maintained by the hub"
)]
fn holds_profile_repository_for_hub(agent: &mut Agent, name: String) {
    let mut repository = demo_repository();
    repository.maintenance = Maintenance::Hub;
    agent.interop.repositories.insert(alias(&name), repository);
}

/// One option more is another location (an `opendal:` backend keeps its
/// bucket in one), unless it is a credential, which is how to open it.
#[given(
    expr = "the configuration holds the repository {string} at the profile's location with the option {string} set to {string}"
)]
fn holds_profile_repository_with_option(
    agent: &mut Agent,
    name: String,
    option: String,
    value: String,
) {
    let mut repository = demo_repository();
    repository.settings.options.insert(option, value);
    agent.interop.repositories.insert(alias(&name), repository);
}

/// The demo repository moved to `location`, with one option more: on a
/// drive the credential names the account, so it is part of the location.
fn at_with_option(location: String, option: String, value: String) -> RepositoryConfig {
    let mut repository = demo_repository();
    repository.settings.repository = location;
    repository.settings.options.insert(option, value);
    repository
}

#[given(
    expr = "the configuration holds the repository {string} at {string} with the option {string} set to {string}"
)]
fn holds_repository_at_with_option(
    agent: &mut Agent,
    name: String,
    location: String,
    option: String,
    value: String,
) {
    agent
        .interop
        .repositories
        .insert(alias(&name), at_with_option(location, option, value));
}

#[given(expr = "the profile's repository is at {string} with the option {string} set to {string}")]
fn profile_repository_at_with_option(
    agent: &mut Agent,
    location: String,
    option: String,
    value: String,
) {
    agent.interop.profile_repository = Some(at_with_option(location, option, value));
}

#[given(expr = "the configuration holds the repositories {string} and {string}")]
fn holds_repositories(agent: &mut Agent, one: String, other: String) {
    for name in [one, other] {
        agent
            .interop
            .repositories
            .insert(alias(&name), configured_repository(&name));
    }
}

#[given(expr = "the configuration holds the plan {string} for the repository {string}")]
fn holds_plan(agent: &mut Agent, id: String, repository: String) {
    agent.interop.plans.push(plan_for(&id, &repository));
}

#[given(expr = "the agent's hostname is {string}")]
fn agent_hostname(agent: &mut Agent, name: String) {
    agent.interop.hostname = Some(hostname(&name));
}

#[given(expr = "the agent's hooks run {string} before everything")]
fn agent_hooks(agent: &mut Agent, program: String) {
    agent.interop.agent_hooks = hooks_before(&[&program]);
}

#[given(expr = "the profile writer cannot put the key {string} into a profile")]
fn writer_refuses(agent: &mut Agent, key: String) {
    agent
        .interop
        .writer
        .refuse(ExportRefusal::Reference { key });
}

#[given(expr = "the profile writer leaves out the option {string} for the variable {string}")]
fn writer_leaves_out(agent: &mut Agent, option: String, variable: String) {
    agent
        .interop
        .writer
        .notice(ProfileNotice::OptionLeftOut { option, variable });
}

#[when(expr = "the owner imports the profile as the repository {string}")]
async fn imports(agent: &mut Agent, name: String) {
    let world = &mut agent.interop;
    world.saved().await;
    world.imported = Some(world.import(&name, Consent::Only(&[])).await);
}

/// The owner is shown the commands the import would make runnable, and
/// agrees to them: the import is run again with them.
#[when(expr = "the owner imports the profile as the repository {string}, confirming its commands")]
async fn imports_confirming(agent: &mut Agent, name: String) {
    imports(agent, name.clone()).await;
    let world = &mut agent.interop;
    if let Some(Err(ImportError::CommandsUnconfirmed(listed))) = world.imported.clone() {
        if let Some(hooks) = world.changed_hooks.take() {
            world.profile_hooks = hooks;
        }
        world.imported = Some(world.import(&name, Consent::Only(&listed)).await);
    }
}

impl InteropWorld {
    /// Imports the scripted profile as the repository `name`.
    #[expect(clippy::result_large_err, reason = "the use case's own error")]
    async fn import(&self, name: &str, consent: Consent<'_>) -> Result<Imported, ImportError> {
        let repository = alias(name);
        // What the real reader does: every plan backs up into the alias.
        let plans = self
            .profile_plans
            .iter()
            .cloned()
            .map(|mut plan| {
                plan.repository = repository.clone();
                plan
            })
            .collect();
        self.reader.script(if self.unreadable {
            Err(ProfileError::Unreadable {
                file: "nas.toml".to_owned(),
            })
        } else {
            Ok(ImportedProfile {
                repository: self
                    .profile_repository
                    .clone()
                    .unwrap_or_else(demo_repository),
                plans,
                hooks: self.profile_hooks.clone(),
                notices: self.profile_notices.clone(),
            })
        });
        let reader: Arc<dyn ProfileReader> = self.reader.clone();
        ImportProfile::new(reader, self.store.clone(), Arc::new(FakeSources))
            .import(Path::new("nas.toml"), repository, consent)
            .await
    }
}

#[then("the import is refused until its commands are confirmed")]
fn refused_unconfirmed(agent: &mut Agent) {
    let imported = agent.interop.imported();
    assert!(
        matches!(imported, Err(ImportError::CommandsUnconfirmed(_))),
        "not refused for its commands: {imported:?}"
    );
}

#[then(expr = "the import lists the command {string} at {string}")]
fn lists_command(agent: &mut Agent, command: String, key: String) {
    let listed = match agent.interop.imported() {
        Ok(imported) => &imported.commands,
        Err(ImportError::CommandsUnconfirmed(listed)) => listed,
        Err(error) => panic!("the import was refused: {error:?}"),
    };
    let expected = ImportedCommand {
        key,
        runs: Runs::Command(CommandLine::parse(&command).expect("a command line")),
    };
    assert!(
        listed.contains(&expected),
        "{expected:?} is not in {listed:?}"
    );
}

#[then(expr = "the import notes that it made {string} runnable at {string}")]
fn notes_command(agent: &mut Agent, program: String, key: String) {
    let notices = match agent.interop.imported() {
        Ok(imported) => &imported.notices,
        Err(error) => panic!("the import was refused: {error:?}"),
    };
    let expected = ProfileNotice::CommandImported { key, program };
    assert!(
        notices.contains(&expected),
        "{expected:?} is not in {notices:?}"
    );
}

#[then("the import notes that the profile's global hooks were left out")]
fn notes_global_hooks_left_out(agent: &mut Agent) {
    let notices = match agent.interop.imported() {
        Ok(imported) => &imported.notices,
        Err(error) => panic!("the import was refused: {error:?}"),
    };
    assert!(
        notices.contains(&ProfileNotice::GlobalHooksLeftOut),
        "no notice in {notices:?}"
    );
}

#[then("the agent's hooks run nothing before everything")]
async fn agent_hooks_empty(agent: &mut Agent) {
    let config = agent.interop.now().await;
    assert!(programs_before(config.hooks()).is_empty());
}

#[when(expr = "the owner exports the plan {string}")]
async fn exports(agent: &mut Agent, id: String) {
    exports_with(agent, &id, ExportCommands::Written).await;
}

#[when(expr = "the plan {string} is exported for an AI agent")]
async fn exports_for_an_ai_agent(agent: &mut Agent, id: String) {
    exports_with(agent, &id, ExportCommands::LeftOut).await;
}

async fn exports_with(agent: &mut Agent, id: &str, commands: ExportCommands) {
    let world = &mut agent.interop;
    world.saved().await;
    let use_case = ExportProfile::new(world.writer.clone(), world.store.clone(), this_agent());
    world.exported = Some(use_case.export(&plan_id(id), commands).await);
}

#[then("the profile writer was asked to leave out every command the profile would run")]
fn asked_to_leave_commands_out(agent: &mut Agent) {
    assert_eq!(rendered(agent).commands, ExportCommands::LeftOut);
}

#[then("the import succeeds")]
fn import_succeeds(agent: &mut Agent) {
    if let Err(error) = agent.interop.imported() {
        panic!("the import was refused: {error:?}");
    }
}

#[then(expr = "the configuration now holds the repository {string}")]
async fn now_holds_repository(agent: &mut Agent, name: String) {
    let config = agent.interop.now().await;
    assert!(
        config.repository(&alias(&name)).is_some(),
        "no repository {name:?} in {:?}",
        config.repositories().keys().collect::<Vec<_>>()
    );
}

#[then(expr = "the configuration now holds the plan {string}")]
async fn now_holds_plan(agent: &mut Agent, id: String) {
    let config = agent.interop.now().await;
    assert!(
        config.plan(&plan_id(&id)).is_some(),
        "no plan {id:?} in {:?}",
        config
            .plans()
            .iter()
            .map(|plan| &plan.id)
            .collect::<Vec<_>>()
    );
}

#[then(expr = "the import is refused because the plan {string} is already taken")]
fn refused_plan_taken(agent: &mut Agent, id: String) {
    match agent.interop.imported() {
        Err(ImportError::PlanTaken(taken)) => assert_eq!(taken, &plan_id(&id)),
        other => panic!("the import should be refused for the plan ID, got {other:?}"),
    }
}

#[then(expr = "the import is refused because the repository {string} is already taken")]
fn refused_alias_taken(agent: &mut Agent, name: String) {
    match agent.interop.imported() {
        Err(ImportError::AliasTaken(taken)) => assert_eq!(taken, &alias(&name)),
        other => panic!("the import should be refused for the alias, got {other:?}"),
    }
}

#[then("the import is refused because the profile cannot be read")]
fn refused_unreadable(agent: &mut Agent) {
    match agent.interop.imported() {
        Err(ImportError::Profile(ProfileError::Unreadable { .. })) => {}
        other => panic!("the import should be refused as unreadable, got {other:?}"),
    }
}

#[then(expr = "the import is refused because the repository is configured as {string}")]
fn refused_already_configured(agent: &mut Agent, name: String) {
    match agent.interop.imported() {
        Err(ImportError::AlreadyConfigured(configured)) => assert_eq!(configured, &alias(&name)),
        other => panic!("the import should be refused for the repository, got {other:?}"),
    }
}

#[then(expr = "the configuration now holds the plan {string} for the repository {string}")]
async fn now_holds_plan_for(agent: &mut Agent, id: String, repository: String) {
    let config = agent.interop.now().await;
    let plan = config
        .plan(&plan_id(&id))
        .unwrap_or_else(|| panic!("no plan {id:?}"));
    assert_eq!(plan.repository, alias(&repository));
}

#[then(expr = "the configuration now holds {int} repository/repositories")]
async fn now_holds_repositories(agent: &mut Agent, count: usize) {
    assert_eq!(agent.interop.now().await.repositories().len(), count);
}

#[then("the configuration is unchanged")]
async fn unchanged(agent: &mut Agent) {
    let now = agent.interop.now().await;
    assert_eq!(Some(&now), agent.interop.before.as_ref());
}

#[then(expr = "the import notes that the key {string} was left out")]
fn notes_key(agent: &mut Agent, key: String) {
    let notices = match agent.interop.imported() {
        Ok(imported) => &imported.notices,
        Err(error) => panic!("the import was refused: {error:?}"),
    };
    assert!(
        notices.contains(&ProfileNotice::KeyIgnored { key: key.clone() }),
        "no notice for {key:?} in {notices:?}"
    );
}

#[then(expr = "the configuration's repository {string} is as it was")]
async fn repository_as_it_was(agent: &mut Agent, name: String) {
    let name = alias(&name);
    let before = agent.interop.before.as_ref().expect("the command ran");
    assert_eq!(
        agent.interop.now().await.repository(&name),
        before.repository(&name)
    );
}

#[then(expr = "the import notes that the repository {string} kept its own {string}")]
fn notes_kept(agent: &mut Agent, repository: String, setting: String) {
    let notices = match agent.interop.imported() {
        Ok(imported) => &imported.notices,
        Err(error) => panic!("the import was refused: {error:?}"),
    };
    let kept = ProfileNotice::RepositorySettingKept {
        repository,
        setting,
    };
    assert!(notices.contains(&kept), "no {kept:?} in {notices:?}");
}

#[then(expr = "the agent's hostname is now {string}")]
async fn hostname_now(agent: &mut Agent, name: String) {
    let config = agent.interop.now().await;
    assert_eq!(config.agent().hostname, Some(hostname(&name)));
}

#[then("the agent has no hostname of its own")]
async fn no_hostname(agent: &mut Agent) {
    let config = agent.interop.now().await;
    assert_eq!(config.agent().hostname, None);
}

#[then(expr = "the plan {string} has no host of its own")]
async fn plan_without_host(agent: &mut Agent, id: String) {
    let config = agent.interop.now().await;
    let plan = config.plan(&plan_id(&id)).expect("the plan was imported");
    assert_eq!(plan.host, None);
}

#[then(expr = "the plan {string} has the host {string}")]
async fn plan_with_host(agent: &mut Agent, id: String, host: String) {
    let config = agent.interop.now().await;
    let plan = config.plan(&plan_id(&id)).expect("the plan was imported");
    assert_eq!(plan.host, Some(hostname(&host)));
}

#[then(expr = "the agent's hooks run {string} and then {string} before everything")]
async fn agent_hooks_order(agent: &mut Agent, first: String, second: String) {
    let config = agent.interop.now().await;
    assert_eq!(
        programs_before(config.hooks()),
        [first.as_str(), second.as_str()]
    );
}

#[then("the export succeeds")]
fn export_succeeds(agent: &mut Agent) {
    if let Err(error) = agent.interop.exported() {
        panic!("the export was refused: {error:?}");
    }
}

/// The one export the writer was asked to render.
fn rendered(agent: &Agent) -> app::ports::PlanExport {
    let mut exports = agent.interop.writer.exports();
    assert_eq!(exports.len(), 1, "one profile was rendered: {exports:?}");
    exports.remove(0)
}

#[then(expr = "the profile is made for the plan {string} of this agent")]
fn made_for_plan(agent: &mut Agent, id: String) {
    let export = rendered(agent);
    assert_eq!(export.plan.id, plan_id(&id));
    assert_eq!(export.tags, PlanTags::new(plan_id(&id), this_agent()));
}

#[then(expr = "the profile is made from the repository {string}")]
async fn made_from_repository(agent: &mut Agent, name: String) {
    let config = agent.interop.now().await;
    let expected = config.repository(&alias(&name)).expect("the repository");
    assert_eq!(&rendered(agent).repository, expected);
}

#[then(expr = "the profile carries the agent's hostname {string}")]
fn carries_hostname(agent: &mut Agent, name: String) {
    assert_eq!(rendered(agent).agent.hostname, Some(hostname(&name)));
}

#[then("the profile carries the agent's hooks")]
fn carries_hooks(agent: &mut Agent) {
    let hooks = rendered(agent).agent_hooks;
    assert!(!hooks.is_empty(), "the agent's hooks were handed over");
    assert_eq!(hooks, agent.interop.agent_hooks);
}

#[then(expr = "the export is refused because the plan {string} is not configured")]
fn refused_unknown_plan(agent: &mut Agent, id: String) {
    match agent.interop.exported() {
        Err(ExportError::PlanUnknown(unknown)) => assert_eq!(unknown, &plan_id(&id)),
        other => panic!("the export should be refused for an unknown plan, got {other:?}"),
    }
}

#[then(expr = "the export is refused because the profile cannot hold the key {string}")]
fn refused_key(agent: &mut Agent, key: String) {
    match agent.interop.exported() {
        Err(ExportError::Render(ExportRefusal::Reference { key: found })) => {
            assert_eq!(found, &key)
        }
        other => panic!("the export should be refused for the key, got {other:?}"),
    }
}

#[then("no profile was made")]
fn no_profile(agent: &mut Agent) {
    let exported = agent.interop.exported();
    assert!(exported.is_err(), "a profile was made: {exported:?}");
}

#[then(expr = "the export notes that the option {string} was left out for the variable {string}")]
fn export_notes_option(agent: &mut Agent, option: String, variable: String) {
    let notices = match agent.interop.exported() {
        Ok(exported) => &exported.notices,
        Err(error) => panic!("the export was refused: {error:?}"),
    };
    assert!(
        notices.contains(&ProfileNotice::OptionLeftOut {
            option: option.clone(),
            variable
        }),
        "no notice for {option:?} in {notices:?}"
    );
}
