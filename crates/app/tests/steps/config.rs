//! Steps for `features/planning/config.feature`: they draft a configuration
//! through the domain's own constructors, and keep it in the world's
//! in-memory `ConfigStore`.

use std::collections::BTreeMap;
use std::sync::Arc;

use app::fakes::InMemoryConfigStore;
use app::ports::ConfigStore;
use cucumber::{given, then, when};
use domain::planning::{
    AgentConfig, AgentSettings, Concurrency, CronExpression, GroupBy, HookEntry, Hooks, Hostname,
    InvalidValue, KeepCount, KeepWithin, Locale, Maintenance, Nice, OnFailure, Plan,
    RepositoryConfig, RepositorySettings, RetryPolicy, Schedule, Source, ValueKind, Violation,
    parse_duration,
};
use domain::secrets::SecretRef;
use domain::{InvalidId, PlanId, RepositoryAlias, SignedDuration};

use super::Agent;

/// The configuration being drafted, the store it is saved to, and what the
/// steps so far gave.
pub struct ConfigWorld {
    store: Arc<InMemoryConfigStore>,
    repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
    plans: Vec<Plan>,
    built: Option<Result<AgentConfig, Violation>>,
    loaded: Option<AgentConfig>,
    read: Option<Read>,
}

impl Default for ConfigWorld {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryConfigStore::new()),
            repositories: BTreeMap::new(),
            plans: Vec::new(),
            built: None,
            loaded: None,
            read: None,
        }
    }
}

/// What reading one value gave: the hook command when it was one, and why it
/// was refused when it was.
struct Read {
    outcome: Result<Option<HookEntry>, Refusal>,
}

/// A refusal: what the value was meant to be, in the scenarios' word for it,
/// and every way the refusal shows itself in text.
struct Refusal {
    kind: &'static str,
    shown: String,
}

impl From<InvalidValue> for Refusal {
    fn from(error: InvalidValue) -> Self {
        let kind = match error.kind {
            ValueKind::Hostname => "hostname",
            ValueKind::Source => "source",
            ValueKind::Locale => "locale",
            ValueKind::Concurrency => "concurrency",
            ValueKind::Nice => "nice",
            ValueKind::Cron => "cron",
            ValueKind::Duration => "duration",
            ValueKind::Attempts => "attempts",
            ValueKind::KeepCount => "keep-count",
            ValueKind::KeepWithin => "keep-within",
            ValueKind::Maintenance => "maintenance",
            ValueKind::GroupBy => "group-by",
            ValueKind::OnFailure => "on-failure",
            ValueKind::HookCommand => "hook-command",
        };
        Self {
            kind,
            shown: format!("{error:?} {error}"),
        }
    }
}

impl From<InvalidId> for Refusal {
    fn from(error: InvalidId) -> Self {
        Self {
            kind: "repository-alias",
            shown: format!("{error:?} {error}"),
        }
    }
}

impl ConfigWorld {
    /// Builds the drafted configuration, and keeps what came of it.
    fn build(&mut self) {
        self.built = Some(AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            self.repositories.clone(),
            self.plans.clone(),
        ));
    }

    /// The drafted configuration, built; the scenario must expect it to be
    /// accepted.
    fn accepted(&mut self) -> AgentConfig {
        self.build();
        match self.built.take() {
            Some(Ok(config)) => config,
            other => panic!("the configuration was refused: {other:?}"),
        }
    }

    /// The configuration the last build accepted.
    fn config(&self) -> &AgentConfig {
        match self.built.as_ref().expect("the configuration was built") {
            Ok(config) => config,
            Err(violation) => panic!("the configuration was refused: {violation:?}"),
        }
    }

    /// Why the last build refused the configuration.
    fn violation(&self) -> &Violation {
        match self.built.as_ref().expect("the configuration was built") {
            Ok(config) => panic!("the configuration was accepted: {config:?}"),
            Err(violation) => violation,
        }
    }

    fn loaded(&self) -> &AgentConfig {
        self.loaded.as_ref().expect("the configuration was loaded")
    }

    fn plan(&self, id: &str) -> &Plan {
        self.config()
            .plan(&plan_id(id))
            .expect("the configuration has the plan")
    }

    fn schedule(&self, id: &str) -> &Schedule {
        self.plan(id)
            .schedule
            .as_ref()
            .expect("the plan has a schedule")
    }

    fn drafted_plan(&mut self, id: &str) -> &mut Plan {
        let id = plan_id(id);
        self.plans
            .iter_mut()
            .find(|plan| plan.id == id)
            .expect("the draft has the plan")
    }

    fn refusal(&self) -> &Refusal {
        match &self.read.as_ref().expect("a value was read").outcome {
            Ok(_) => panic!("the value was accepted"),
            Err(refusal) => refusal,
        }
    }

    fn hook(&self) -> &HookEntry {
        match &self.read.as_ref().expect("a value was read").outcome {
            Ok(Some(hook)) => hook,
            Ok(None) => panic!("the value read was not a hook command"),
            Err(refusal) => panic!("the hook command was refused: {}", refusal.shown),
        }
    }
}

fn alias(text: &str) -> RepositoryAlias {
    RepositoryAlias::parse(text).expect("the scenario's repository alias is valid")
}

fn plan_id(text: &str) -> PlanId {
    PlanId::parse(text).expect("the scenario's plan ID is valid")
}

fn source(text: &str) -> Source {
    Source::parse(text).expect("the scenario's source is valid")
}

fn number(text: &str) -> i64 {
    text.parse().expect("the scenario's number is a number")
}

/// Reads `text` as the kind of value the scenario names.
fn read(kind: &str, text: &str) -> Result<Option<HookEntry>, Refusal> {
    fn value<T>(result: Result<T, InvalidValue>) -> Result<Option<HookEntry>, Refusal> {
        result.map(|_| None).map_err(Refusal::from)
    }
    match kind {
        "hostname" => value(Hostname::parse(text)),
        "source" => value(Source::parse(text)),
        "locale" => value(Locale::parse(text)),
        "repository-alias" => RepositoryAlias::parse(text)
            .map(|_| None)
            .map_err(Refusal::from),
        "concurrency" => value(Concurrency::new(number(text))),
        "nice" => value(Nice::new(number(text))),
        "cron" => value(CronExpression::parse(text)),
        "duration" => value(parse_duration(text)),
        "attempts" => value(RetryPolicy::new(number(text), SignedDuration::from_mins(5))),
        "keep-count" => value(KeepCount::new(number(text))),
        "keep-within" => value(KeepWithin::parse(text)),
        "maintenance" => value(Maintenance::parse(text)),
        "group-by" => value(GroupBy::parse(text)),
        "on-failure" => value(OnFailure::parse(text)),
        "hook-command" => HookEntry::parse(text).map(Some).map_err(Refusal::from),
        other => panic!("the scenario reads a value of the unknown kind {other}"),
    }
}

#[given(expr = "a repository {string}")]
fn a_repository(agent: &mut Agent, name: String) {
    agent.config.repositories.insert(
        alias(&name),
        RepositoryConfig {
            settings: RepositorySettings {
                repository: "/srv/demo-repo".to_owned(),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Maintenance::default(),
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        },
    );
}

#[given(expr = "a plan {string} backing up {string} into {string}")]
fn a_plan(agent: &mut Agent, id: String, path: String, repository: String) {
    agent.config.plans.push(Plan::new(
        plan_id(&id),
        alias(&repository),
        vec![source(&path)],
    ));
}

#[given(expr = "a plan {string} backing up nothing into {string}")]
fn a_plan_with_no_sources(agent: &mut Agent, id: String, repository: String) {
    agent
        .config
        .plans
        .push(Plan::new(plan_id(&id), alias(&repository), Vec::new()));
}

#[given(expr = "the plan {string} follows the cron expression {string}")]
fn a_plan_follows(agent: &mut Agent, id: String, cron: String) {
    let cron = CronExpression::parse(&cron).expect("the scenario's cron expression is valid");
    agent.config.drafted_plan(&id).schedule = Some(Schedule::new(cron));
}

#[given(expr = "the plan {string} is dropped")]
fn a_plan_is_dropped(agent: &mut Agent, id: String) {
    let id = plan_id(&id);
    agent.config.plans.retain(|plan| plan.id != id);
}

async fn save(agent: &mut Agent) {
    let config = agent.config.accepted();
    agent
        .config
        .store
        .save(&config)
        .await
        .expect("the in-memory store saves");
}

#[given("the configuration is saved")]
async fn saved_before(agent: &mut Agent) {
    save(agent).await;
}

#[when("the configuration is saved")]
async fn saved(agent: &mut Agent) {
    save(agent).await;
}

#[when("the configuration is loaded")]
async fn loaded(agent: &mut Agent) {
    let config = agent
        .config
        .store
        .load()
        .await
        .expect("the in-memory store loads");
    agent.config.loaded = Some(config);
}

#[when("the configuration is built")]
fn built(agent: &mut Agent) {
    agent.config.build();
}

#[when(expr = "the {word} {string} is read")]
fn a_value_is_read(agent: &mut Agent, kind: String, text: String) {
    agent.config.read = Some(Read {
        outcome: read(&kind, &text),
    });
}

#[then(expr = "the loaded configuration has {int} repository/repositories")]
fn loaded_repositories(agent: &mut Agent, count: usize) {
    assert_eq!(agent.config.loaded().repositories().len(), count);
}

#[then(expr = "the loaded configuration has {int} plan/plans")]
fn loaded_plans(agent: &mut Agent, count: usize) {
    assert_eq!(agent.config.loaded().plans().len(), count);
}

#[then(expr = "the loaded configuration has the plan {string} backing up {string} into {string}")]
fn loaded_plan(agent: &mut Agent, id: String, path: String, repository: String) {
    let plan = agent
        .config
        .loaded()
        .plan(&plan_id(&id))
        .expect("the loaded configuration has the plan");
    assert_eq!(plan.sources, [source(&path)]);
    assert_eq!(plan.repository, alias(&repository));
}

#[then("the configuration is accepted")]
fn is_accepted(agent: &mut Agent) {
    agent.config.config();
}

#[then(
    expr = "the configuration is refused because plan number {int}, {string}, names the repository {string}, which it doesn't have"
)]
fn refused_for_a_repository(agent: &mut Agent, number: usize, plan: String, repository: String) {
    assert_eq!(
        agent.config.violation(),
        &Violation::UnknownRepository {
            index: number - 1,
            plan: plan_id(&plan),
            repository: alias(&repository),
        }
    );
}

#[then(
    expr = "the configuration is refused because plan number {int}, {string}, repeats the ID of an earlier plan"
)]
fn refused_for_a_repeat(agent: &mut Agent, number: usize, plan: String) {
    assert_eq!(
        agent.config.violation(),
        &Violation::DuplicatePlan {
            index: number - 1,
            plan: plan_id(&plan),
        }
    );
}

#[then(expr = "the configuration is refused because plan number {int}, {string}, backs up nothing")]
fn refused_for_no_sources(agent: &mut Agent, number: usize, plan: String) {
    assert_eq!(
        agent.config.violation(),
        &Violation::NoSources {
            index: number - 1,
            plan: plan_id(&plan),
        }
    );
}

#[then(expr = "the plan {string} runs on the cron expression {string}")]
fn runs_on(agent: &mut Agent, id: String, cron: String) {
    assert_eq!(agent.config.schedule(&id).cron().as_str(), cron);
}

#[then(expr = "the plan {string} makes up for a run it missed")]
fn makes_up(agent: &mut Agent, id: String) {
    assert!(agent.config.schedule(&id).catch_up());
}

#[then(expr = "the plan {string} runs on any power source")]
fn runs_on_any_power(agent: &mut Agent, id: String) {
    assert!(!agent.config.schedule(&id).on_ac_power());
}

#[then(expr = "the plan {string} adds no jitter")]
fn adds_no_jitter(agent: &mut Agent, id: String) {
    assert_eq!(agent.config.schedule(&id).jitter(), None);
}

#[then(expr = "the plan {string} has no schedule")]
fn has_no_schedule(agent: &mut Agent, id: String) {
    assert_eq!(agent.config.plan(&id).schedule, None);
}

#[then(expr = "the repository {string} has the maintenance {string}")]
fn has_maintenance(agent: &mut Agent, name: String, maintenance: String) {
    let repository = agent
        .config
        .config()
        .repository(&alias(&name))
        .expect("the configuration has the repository");
    assert_eq!(repository.maintenance.as_str(), maintenance);
}

#[then(expr = "the repository {string} groups snapshots by {string}")]
fn groups_by(agent: &mut Agent, name: String, group_by: String) {
    let repository = agent
        .config
        .config()
        .repository(&alias(&name))
        .expect("the configuration has the repository");
    assert_eq!(repository.group_by.to_string(), group_by);
}

#[then(expr = "the agent runs {int} operation(s) at a time")]
fn runs_at_a_time(agent: &mut Agent, count: usize) {
    let concurrency = agent.config.config().agent().concurrency;
    assert_eq!(usize::from(concurrency.get()), count);
}

#[then(expr = "the value is accepted as {word}")]
fn value_accepted(agent: &mut Agent, kind: String) {
    let read = agent.config.read.as_ref().expect("a value was read");
    if let Err(refusal) = &read.outcome {
        panic!("the {kind} was refused: {}", refusal.shown);
    }
}

#[then(expr = "the value is refused as {word}")]
fn value_refused(agent: &mut Agent, kind: String) {
    assert_eq!(agent.config.refusal().kind, kind);
}

#[then(expr = "the hook runs the program {string}")]
fn hook_runs(agent: &mut Agent, program: String) {
    assert_eq!(agent.config.hook().command.program(), program);
}

#[then(expr = "the hook's arguments are {string} and {string}")]
fn hook_arguments(agent: &mut Agent, first: String, second: String) {
    assert_eq!(agent.config.hook().command.args(), [first, second]);
}

#[then(expr = "the refusal doesn't mention {string}")]
fn refusal_hides(agent: &mut Agent, secret: String) {
    let shown = &agent.config.refusal().shown;
    assert!(
        !shown.contains(&secret),
        "the refusal shows {secret:?}: {shown}"
    );
}
