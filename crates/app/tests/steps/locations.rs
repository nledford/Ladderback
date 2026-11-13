//! Steps for `features/repositories/repository_locations.feature`: a
//! repository "nas" reached at the locations "lan" and "public", a scripted
//! engine that answers each location's probe as the scenario says, and the
//! choice of location in front of it, with the fake clock that jumps to each
//! probe's bound, and the machine's network link the locations' rules are
//! evaluated against.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use app::conditions::{LinkReading, check_location_rules};
use app::execution::RunOperation;
use app::fakes::{FakeNetworkLink, FakeRetention, FakeSecrets, InMemoryConfigStore};
use app::locations::{ChooseLocation, LocationReport};
use app::maintenance::{MaintenanceKind, RunMaintenance};
use app::ports::{
    BackupJob, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl,
    NormalizeSources, PlatformError, ProbeAnswer, RepositoryAccess, RunningJob, SavedEntry,
    SourcesError,
};
use app::secrets::ResolveRepository;
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::locations::{LocationOutcome, LocationsRecord};
use domain::execution::plan_tags::PlanTags;
use domain::execution::retry::RetrySchedule;
use domain::execution::{Operation, OperationStatus, Trigger, WarningKind};
use domain::planning::{
    AgentConfig, AgentSettings, Alternative, CurrentNetworkName, Hooks, Interval, Link, Location,
    LocationRule, LocationWarning, Locations, Maintenance, NameFilter, NetworkKind, NetworkName, Plan, RepositoryConfig, RepositorySettings, RetryPolicy,
    RuleNote, Schedule, Source, SourceFilter,
};
use domain::secrets::SecretRef;
use domain::{AgentId, OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration, Uuid};
use futures::StreamExt;

use super::Agent;

/// The ID the entry records: every location of "nas" holds this repository.
const REPOSITORY_ID: &str = "4c6f636174696f6e73206f6620746865207265706f7369746f72792066697273";
/// The ID of a repository that isn't "nas".
const OTHER_ID: &str = "0be7a1f0be7a1f0be7a1f0be7a1f0be7a1f0be7a1f0be7a1f0be7a1f0be7a1f0";

const NAS: &str = "nas";
const PLAN: &str = "home";

/// What a location does when it is probed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Holds,
    HoldsAnother,
    Silent,
    Refused,
    NotARepository,
    PasswordNotAccepted,
}

/// The repositories of the scenario, what each location answers, and what
/// the engine and the log were given.
#[derive(Default)]
pub struct LocationsWorld {
    entries: BTreeMap<String, RepositoryConfig>,
    /// By location name: its address as the entry writes it.
    addresses: BTreeMap<String, String>,
    /// By address: what a probe there finds. An address with none answers.
    answers: Arc<Mutex<BTreeMap<String, Answer>>>,
    secrets: Arc<FakeSecrets>,
    /// Every job the engine was given, probes included, in order.
    jobs: Arc<Mutex<Vec<Job>>>,
    reports: Arc<Mutex<Vec<LocationReport>>>,
    /// Every run's operation as it ended, and its result as it shows.
    operations: Vec<Operation>,
    results: Vec<String>,
    listed: Vec<LocationWarning>,
    /// The credentials a location's next step writes, when its address
    /// comes from a variable.
    pending_credentials: Option<String>,
    retention: Arc<Mutex<Vec<RepositoryAccess>>>,
    /// The machine's network link, which the locations' rules are
    /// evaluated against.
    link: Arc<FakeNetworkLink>,
    /// What a person's Test of the locations found.
    tested: Option<LocationsRecord>,
    /// Whether the plan "home" is scheduled, and if so whether it retries
    /// a failed run; `None` for a plan that runs only when asked.
    plan_retries: Option<bool>,
    /// Whether this machine can't tell a wired link from other links.
    cant_tell_wired: bool,
}

impl std::fmt::Debug for LocationsWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocationsWorld")
            .field("addresses", &self.addresses)
            .finish_non_exhaustive()
    }
}

fn repository_id(hex: &str) -> RepositoryId {
    RepositoryId::parse(hex).expect("a repository ID")
}

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("an alias")
}

fn plan_id(name: &str) -> PlanId {
    PlanId::parse(name).expect("a plan ID")
}

fn agent_id() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0042))
}

fn address_of(name: &str) -> String {
    match name {
        "lan" => "rest:https://lan.example.lan/nas".to_owned(),
        "public" => "rest:https://backup.example.net/nas".to_owned(),
        other => format!("/srv/{other}"),
    }
}

fn settings(repository: &str) -> RepositorySettings {
    RepositorySettings {
        repository: repository.to_owned(),
        repo_hot: None,
        password: SecretRef::Env("NAS_PASSWORD".to_owned()),
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    }
}

impl LocationsWorld {
    fn nas(&mut self) -> &mut RepositoryConfig {
        self.entries.get_mut(NAS).expect("the repository nas")
    }

    /// The position, from 1, of the location named `name` in "nas".
    fn position(&self, name: &str) -> u8 {
        let address = &self.addresses[name];
        let entry = &self.entries[NAS];
        if entry.settings.repository == *address {
            return 1;
        }
        let further = entry
            .locations
            .further
            .iter()
            .position(|location| location.repository == *address)
            .expect("a location of nas");
        u8::try_from(further + 2).expect("a position")
    }

    /// Sets what location `name` of "nas" answers.
    fn answers(&self, name: &str, answer: Answer) {
        let address = self.addresses[name].clone();
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(address, answer);
    }

    /// Changes the location `name` of "nas" as `change` says, wherever the
    /// entry keeps it.
    fn change_location(&mut self, name: &str, change: impl FnOnce(&mut Location)) {
        let address = self.addresses[name].clone();
        let entry = self.nas();
        if entry.settings.repository == address {
            let mut first = Location {
                when: entry.locations.first_when.clone(),
                repository: entry.settings.repository.clone(),
                repo_hot: entry.settings.repo_hot.clone(),
                options: entry.settings.options.clone(),
                options_hot: entry.settings.options_hot.clone(),
                options_cold: entry.settings.options_cold.clone(),
            };
            change(&mut first);
            entry.settings.repository = first.repository;
            entry.settings.repo_hot = first.repo_hot;
            entry.settings.options = first.options;
            entry.settings.options_hot = first.options_hot;
            entry.settings.options_cold = first.options_cold;
            entry.locations.first_when = first.when;
            return;
        }
        let location = entry
            .locations
            .further
            .iter_mut()
            .find(|location| location.repository == address)
            .expect("a location of nas");
        change(location);
    }

    /// Renames the address of location `name` to `address`.
    fn readdress(&mut self, name: &str, address: &str) {
        let address = address.to_owned();
        let moved = address.clone();
        self.change_location(name, move |location| location.repository = moved);
        let old = self.addresses.insert(name.to_owned(), address.clone());
        let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(answer) = old.and_then(|old| answers.remove(&old)) {
            answers.insert(address, answer);
        }
    }

    fn engine(&self, agent_clock: Arc<app::fakes::FakeClock>) -> Arc<ChooseLocation> {
        let reports = Arc::clone(&self.reports);
        Arc::new(ChooseLocation::new(
            Arc::new(Sites {
                answers: Arc::clone(&self.answers),
                jobs: Arc::clone(&self.jobs),
            }),
            agent_clock,
            self.link.clone(),
            ResolveRepository::new(self.secrets.clone()),
            Arc::new(move |report| {
                reports
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(report);
            }),
        ))
    }

    fn jobs(&self) -> Vec<Job> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn probes(&self) -> Vec<RepositoryAccess> {
        self.jobs()
            .into_iter()
            .filter_map(|job| match job {
                Job::Probe(access) => Some(access),
                _ => None,
            })
            .collect()
    }

    /// The accesses of every job that opened the repository, not to probe
    /// it, and of every in-process open.
    fn opens(&self) -> Vec<RepositoryAccess> {
        let mut opens: Vec<RepositoryAccess> = self
            .jobs()
            .into_iter()
            .filter(|job| !matches!(job, Job::Probe(_)))
            .filter_map(|job| job.access().cloned())
            .collect();
        opens.extend(
            self.retention
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .cloned(),
        );
        opens
    }

    fn reports(&self) -> Vec<LocationReport> {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn operation(&self) -> &Operation {
        self.operations.last().expect("a run has ended")
    }

    fn record(&self) -> &LocationsRecord {
        self.operation()
            .locations()
            .expect("the operation records its locations")
    }

    /// The rules of the locations of "nas", in order, when the scenario has
    /// it: the plan "home" backs up to it.
    pub(super) fn plan_rules(&self) -> Option<Vec<Option<LocationRule>>> {
        self.entries.get(NAS).map(|entry| {
            entry
                .location_rules()
                .into_iter()
                .map(Option::<&LocationRule>::cloned)
                .collect()
        })
    }

    /// The machine's network link.
    pub(super) fn link(&self) -> Arc<FakeNetworkLink> {
        self.link.clone()
    }
}

/// A rule of `kinds`, in order.
fn rule(kinds: &[NetworkKind]) -> LocationRule {
    LocationRule::new(kinds.iter().copied().map(Into::into).collect()).expect("a rule")
}

/// A rule of one Wi-Fi alternative that lists `names`.
fn named_rule(names: NameFilter) -> LocationRule {
    LocationRule::new(vec![
        Alternative::new(NetworkKind::WiFi, Some(names)).expect("an alternative"),
    ])
    .expect("a rule")
}

fn network_name(name: &str) -> NetworkName {
    NetworkName::parse(name).expect("a network name")
}

/// Sets what the machine's network link reads as, for the locations' rules
/// and the scheduler's alike.
fn machine_on(agent: &mut Agent, answer: Result<Link, PlatformError>) {
    agent.locations.link.set(answer.clone());
    agent.scheduler.link().set(answer);
}

/// Sets what the machine's Wi-Fi network's name reads as, for the
/// locations' rules and the scheduler's alike, with the link on Wi-Fi.
fn machine_named(agent: &mut Agent, answer: Result<Option<CurrentNetworkName>, PlatformError>) {
    machine_on(agent, Ok(Link::WiFi));
    agent.locations.link.set_name(answer.clone());
    agent.scheduler.link().set_name(answer);
}

/// An engine that answers each location's probe as the scenario says, and
/// opens whatever else it is given at once, but refuses, as the engine does
/// (§7.3), a job whose access names a repository ID its location doesn't
/// hold.
struct Sites {
    answers: Arc<Mutex<BTreeMap<String, Answer>>>,
    jobs: Arc<Mutex<Vec<Job>>>,
}

struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Sites {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(job.clone());
        let at = domain::Timestamp::from_second(1_800_000_000).expect("a time");
        let access = job.access().cloned();
        if !matches!(job, Job::Probe(_))
            && let Some(access) = &access
            && let Some(expected) = &access.id
            && self
                .answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&access.repository)
                == Some(&Answer::HoldsAnother)
            && *expected != repository_id(OTHER_ID)
        {
            return Ok(RunningJob {
                events: futures::stream::iter([EngineEvent::Failed(
                    domain::execution::FailureReason::RepositoryReplaced,
                )])
                .boxed(),
                control: Box::new(Stop),
            });
        }
        let events = match &job {
            Job::Probe(access) => {
                let answer = self
                    .answers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&access.repository)
                    .copied()
                    .unwrap_or(Answer::Holds);
                let found = match answer {
                    // A silent location never answers: the probe's bound ends it.
                    Answer::Silent => {
                        return Ok(RunningJob {
                            events: futures::stream::pending().boxed(),
                            control: Box::new(Stop),
                        });
                    }
                    Answer::Holds => ProbeAnswer::Holds(repository_id(REPOSITORY_ID)),
                    Answer::HoldsAnother => ProbeAnswer::Holds(repository_id(OTHER_ID)),
                    Answer::Refused => ProbeAnswer::Refused,
                    Answer::NotARepository => ProbeAnswer::NotARepository,
                    Answer::PasswordNotAccepted => ProbeAnswer::PasswordNotAccepted,
                };
                vec![
                    EngineEvent::Started {
                        index_read_at: at,
                        repository: None,
                    },
                    EngineEvent::Succeeded(EngineResult::Probed(found)),
                ]
            }
            // One snapshot of the plan, so a forget's preview applies the
            // plan's retention, which opens the repository in-process.
            Job::Ls(_) => vec![
                EngineEvent::Started {
                    index_read_at: at,
                    repository: access.as_ref().and_then(|access| access.id.clone()),
                },
                EngineEvent::Succeeded(EngineResult::Snapshots(vec![
                    app::ports::SnapshotSummary {
                        id: domain::SnapshotId::parse(&"5a".repeat(32)).expect("a snapshot ID"),
                        time: at,
                        offset: domain::Offset::UTC,
                        hostname: "demo-machine".to_owned(),
                        label: String::new(),
                        tags: vec![
                            format!(
                                "{}{}",
                                domain::execution::plan_tags::AGENT_TAG_PREFIX,
                                agent_id().as_uuid().hyphenated()
                            ),
                            format!("{}{PLAN}", domain::execution::plan_tags::PLAN_TAG_PREFIX),
                        ],
                        paths: vec!["/home/demo".to_owned()],
                        paths_truncated: false,
                        tags_truncated: false,
                    },
                ])),
            ],
            _ => vec![
                EngineEvent::Started {
                    index_read_at: at,
                    repository: access.as_ref().and_then(|access| access.id.clone()),
                },
                EngineEvent::Succeeded(EngineResult::Repository {
                    id: repository_id(REPOSITORY_ID),
                }),
            ],
        };
        Ok(RunningJob {
            events: futures::stream::iter(events).boxed(),
            control: Box::new(Stop),
        })
    }
}

/// A retention that records the access it was asked to open.
struct RecordingRetention(Arc<Mutex<Vec<RepositoryAccess>>>);

#[async_trait]
impl app::ports::Retention for RecordingRetention {
    async fn removes(
        &self,
        repository: &RepositoryAccess,
        policy: &domain::planning::RetentionPolicy,
        group: &[domain::maintenance::Snapshot],
    ) -> Result<Vec<domain::SnapshotId>, app::ports::RetentionError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(repository.clone());
        FakeRetention.removes(repository, policy, group).await
    }
}

/// Sources as written.
struct AsWritten;

#[async_trait]
impl NormalizeSources for AsWritten {
    async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
        Ok(sources.to_vec())
    }
}

/// Runs a backup of the plan "home" on `repository`, started by `trigger`.
async fn run_backup(agent: &mut Agent, repository: &str, trigger: Trigger) {
    let world = &agent.locations;
    let entry = world.entries[repository].clone();
    let resolved = ResolveRepository::new(world.secrets.clone())
        .resolve_saved(&alias(repository), &entry)
        .await
        .expect("the repository's password resolves");
    let job = Job::Backup(BackupJob {
        repository: resolved.access,
        sources: vec!["/home/demo".to_owned()],
        tags: PlanTags::new(plan_id(PLAN), agent_id()),
        extra_tags: Vec::new(),
        label: None,
        host: None,
        filter: SourceFilter::default(),
        deadline_budget: SignedDuration::from_hours(1),
    });
    let engine = world.engine(agent.clock.clone());
    let n = u128::try_from(world.operations.len()).expect("a count");
    let finished = RunOperation::new(engine, agent.clock.clone(), agent.store.clone())
        .run(
            OperationId::from_uuid(Uuid::from_u128(0x0400 + n)),
            trigger,
            job,
            futures::future::pending(),
        )
        .await;
    agent.locations.operations.push(finished.operation.clone());
    agent
        .locations
        .results
        .push(format!("{:?}", finished.result));
    agent.finished = Some(finished);
}

/// The outcome a scenario's words name.
fn outcome(reason: &str) -> LocationOutcome {
    match reason {
        "it didn't answer" => LocationOutcome::Silent,
        "it refused its credentials" => LocationOutcome::Refused,
        "it held no repository" => LocationOutcome::NotARepository,
        "it didn't accept the repository's password" => LocationOutcome::PasswordNotAccepted,
        "its rule didn't hold" => LocationOutcome::RuleNotMet,
        "its rule couldn't be evaluated" => LocationOutcome::RuleUnknown,
        other => panic!("no outcome is called {other:?}"),
    }
}

/// The failure code a scenario's words name: "no location usable" is
/// `no-location-usable`.
fn code(reason: &str) -> String {
    reason.replace("'s", "").replace(' ', "-")
}

#[given(
    expr = "a repository {string} that records its repository ID, with the locations {string} and {string}, in that order"
)]
fn repository_with_locations(agent: &mut Agent, name: String, first: String, second: String) {
    let world = &mut agent.locations;
    world.secrets.set_env("NAS_PASSWORD", "demo-password");
    for location in [&first, &second] {
        world
            .addresses
            .insert(location.clone(), address_of(location));
    }
    world.entries.insert(
        name,
        RepositoryConfig {
            settings: settings(&address_of(&first)),
            locations: Locations {
                first_when: None,
                further: vec![Location {
                    repository: address_of(&second),
                    ..Location::default()
                }],
            },
            id: Some(repository_id(REPOSITORY_ID)),
            maintenance: Maintenance::Local,
            group_by: domain::planning::GroupBy::default(),
            hooks: Hooks::default(),
        },
    );
}

#[given(expr = "the plan {string} backs up to {string}")]
fn plan_backs_up(_agent: &mut Agent, plan: String, repository: String) {
    assert_eq!((plan.as_str(), repository.as_str()), (PLAN, NAS));
}

#[given(expr = "the location {string} answers")]
fn location_answers(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::Holds);
}

#[given(expr = "the location {string} doesn't answer within 8 seconds")]
fn location_silent_to_bound(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::Silent);
}

#[given(expr = "the location {string} doesn't answer")]
fn location_silent(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::Silent);
}

#[given(expr = "the location {string} answers with a repository whose ID isn't the repository's")]
fn location_holds_another(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::HoldsAnother);
}

#[given(expr = "the location {string} answers and refuses its credentials")]
fn location_refuses(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::Refused);
}

#[given(expr = "the location {string} answers and holds no repository")]
fn location_holds_none(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::NotARepository);
}

#[given(expr = "the location {string} answers and doesn't accept the repository's password")]
fn location_refuses_password(agent: &mut Agent, name: String) {
    agent.locations.answers(&name, Answer::PasswordNotAccepted);
}

#[given("both locations answer and don't accept the repository's password")]
fn both_refuse_password(agent: &mut Agent) {
    for name in ["lan", "public"] {
        agent.locations.answers(name, Answer::PasswordNotAccepted);
    }
}

#[given("neither location answers")]
fn neither_answers(agent: &mut Agent) {
    for name in ["lan", "public"] {
        agent.locations.answers(name, Answer::Silent);
    }
}

#[given(expr = "the location {string} has a credential reference that doesn't resolve")]
fn location_reference_unresolved(agent: &mut Agent, name: String) {
    agent.locations.change_location(&name, |location| {
        location
            .options
            .insert("password".to_owned(), "${LAN_KEY}".to_owned());
    });
}

#[given(expr = "the plan {string} retries up to {int} times")]
fn plan_retries(_agent: &mut Agent, plan: String, attempts: u32) {
    // A retry is planned for any failed run the schedule started (decision
    // 76); the policy's attempts decide only how many.
    assert_eq!(plan, PLAN);
    assert!(attempts > 0);
}

#[given(expr = "the location {string} carries the canary secret {string} in its options")]
fn location_carries_canary(agent: &mut Agent, name: String, canary: String) {
    agent.locations.change_location(&name, |location| {
        location
            .options
            .insert("application_key".to_owned(), canary);
    });
}

#[given(expr = "a repository {string} with one location")]
fn repository_with_one_location(agent: &mut Agent, name: String) {
    let world = &mut agent.locations;
    world.secrets.set_env("NAS_PASSWORD", "demo-password");
    world.addresses.insert(name.clone(), address_of(&name));
    world.entries.insert(
        name.clone(),
        RepositoryConfig {
            settings: settings(&address_of(&name)),
            locations: Locations::default(),
            id: Some(repository_id(REPOSITORY_ID)),
            maintenance: Maintenance::None,
            group_by: domain::planning::GroupBy::default(),
            hooks: Hooks::default(),
        },
    );
}

#[given(
    expr = "the location {string} is used for one backup and the location {string} for the next"
)]
async fn one_then_other(agent: &mut Agent, first: String, second: String) {
    let other = |name: &str| if name == "lan" { "public" } else { "lan" };
    agent.locations.answers(other(&first), Answer::Silent);
    agent.locations.answers(&first, Answer::Holds);
    run_backup(agent, NAS, Trigger::Schedule).await;
    agent.locations.answers(other(&second), Answer::Silent);
    agent.locations.answers(&second, Answer::Holds);
    run_backup(agent, NAS, Trigger::Schedule).await;
}

#[given(regex = r#"^the location "([^"]+)" is "([^"]+)" with (.+)$"#)]
fn location_is(agent: &mut Agent, name: String, address: String, credentials: String) {
    let world = &mut agent.locations;
    let user = "demo:canary-pass@";
    let written = match (credentials.as_str(), address.split_once("://")) {
        ("a user and a password in it", Some((scheme, rest))) => format!("{scheme}://{user}{rest}"),
        ("a user and a password in it", None) => {
            world.pending_credentials = Some(user.to_owned());
            address.clone()
        }
        _ => address.clone(),
    };
    world.readdress(&name, &written);
    let options: Vec<(&str, &str)> = match credentials.as_str() {
        "a user and a password in it" => Vec::new(),
        "a credential option" => vec![("password", "canary-pass")],
        "the option http-insecure-tls set to true" => vec![("http-insecure-tls", "true")],
        "an http endpoint and a credential option" => vec![
            ("endpoint", "http://s3.example.lan"),
            ("secret_access_key", "canary-pass"),
        ],
        other => panic!("no credentials are called {other:?}"),
    };
    world.change_location(&name, |location| {
        for (key, value) in options {
            location.options.insert(key.to_owned(), value.to_owned());
        }
    });
}

#[given(expr = "the variable {string} holds an http address")]
fn variable_holds_http(agent: &mut Agent, variable: String) {
    let world = &mut agent.locations;
    let user = world.pending_credentials.take().unwrap_or_default();
    world
        .secrets
        .set_env(&variable, &format!("http://{user}lan.example.lan/r"));
}

#[when(expr = "the plan {string} runs")]
async fn plan_runs(agent: &mut Agent, plan: String) {
    assert_eq!(plan, PLAN);
    run_backup(agent, NAS, Trigger::Schedule).await;
}

#[when(expr = "the plan {string} runs as a scheduled run")]
async fn plan_runs_scheduled(agent: &mut Agent, plan: String) {
    assert_eq!(plan, PLAN);
    run_backup(agent, NAS, Trigger::Schedule).await;
}

#[when(expr = "a person runs the plan {string} now")]
async fn person_runs_plan(agent: &mut Agent, plan: String) {
    assert_eq!(plan, PLAN);
    run_backup(agent, NAS, Trigger::Manual).await;
}

#[when(expr = "a person runs a backup of {string} now")]
async fn person_runs_backup(agent: &mut Agent, repository: String) {
    run_backup(agent, &repository, Trigger::Manual).await;
}

#[when(expr = "a backup of {string} runs")]
async fn backup_runs(agent: &mut Agent, repository: String) {
    run_backup(agent, &repository, Trigger::Schedule).await;
}

#[when(expr = "a forget is previewed for the repository {string}")]
async fn forget_previewed(agent: &mut Agent, repository: String) {
    let world = &agent.locations;
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        world
            .entries
            .clone()
            .into_iter()
            .map(|(name, entry)| (alias(&name), entry))
            .collect(),
        vec![Plan::new(
            plan_id(PLAN),
            alias(&repository),
            vec![Source::parse("/home/demo").expect("a source")],
        )],
    )
    .expect("a configuration");
    let store = Arc::new(InMemoryConfigStore::new());
    store.save(&config).await.expect("the configuration saves");
    let locations = world.engine(agent.clock.clone());
    let maintenance = RunMaintenance::new(
        locations.clone(),
        agent.clock.clone(),
        agent.store.clone(),
        ResolveRepository::new(world.secrets.clone()),
        Arc::new(RecordingRetention(Arc::clone(&world.retention))),
        store,
        agent_id(),
        "demo-machine".to_owned(),
        Arc::new(AsWritten),
    )
    .with_locations(locations);
    maintenance
        .preview(
            OperationId::from_uuid(Uuid::from_u128(0x0500)),
            &alias(&repository),
            &MaintenanceKind::Forget,
        )
        .await
        .expect("the forget is previewed");
}

#[when("the repositories are listed")]
fn repositories_listed(agent: &mut Agent) {
    let world = &mut agent.locations;
    world.listed = world.entries[NAS].location_warnings();
}

#[then(expr = "the operation used the location {string}")]
fn used_location(agent: &mut Agent, name: String) {
    let world = &agent.locations;
    assert_eq!(world.record().used(), Some(world.position(&name)));
    let address = &world.addresses[&name];
    let opens = world.opens();
    assert!(!opens.is_empty(), "the job opened the repository");
    for access in opens {
        assert_eq!(&access.repository, address, "the job opened {name} only");
    }
}

#[then(expr = "the location {string} was never tried")]
fn never_tried(agent: &mut Agent, name: String) {
    let world = &agent.locations;
    let address = &world.addresses[&name];
    for job in world.jobs() {
        if let Some(access) = job.access() {
            assert_ne!(&access.repository, address, "{name} was tried: {job:?}");
        }
    }
}

#[then(expr = "the operation shows the location {string} as not tried")]
fn shows_not_tried(agent: &mut Agent, name: String) {
    let world = &agent.locations;
    let tried = world
        .record()
        .at(world.position(&name))
        .expect("the location");
    assert_eq!(tried.outcome, LocationOutcome::NotTried);
}

#[then(regex = r#"^the operation says the location "([^"]+)" was skipped because (.+)$"#)]
fn says_skipped(agent: &mut Agent, name: String, reason: String) {
    let world = &agent.locations;
    let tried = world
        .record()
        .at(world.position(&name))
        .expect("the location");
    assert_eq!(tried.outcome, outcome(&reason));
}

#[then("the operation has no warnings")]
fn no_warnings(agent: &mut Agent) {
    let operation = agent.locations.operation();
    assert!(
        operation.warnings().is_empty(),
        "{:?}",
        operation.warnings()
    );
    assert_eq!(*operation.status(), OperationStatus::Succeeded);
}

/// The operation's `location-skipped` warning for the location `name`.
fn skipped_warning(agent: &Agent, name: &str) -> domain::execution::Warning {
    let world = &agent.locations;
    let position = world.position(name).to_string();
    world
        .operation()
        .warnings()
        .iter()
        .find(|warning| {
            warning.kind == WarningKind::LocationSkipped
                && warning.item.as_deref() == Some(&*position)
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no warning about {name}: {:?}",
                world.operation().warnings()
            )
        })
}

#[then(expr = "the operation warns that the location {string} held another repository")]
fn warns_other_repository(agent: &mut Agent, name: String) {
    let warning = skipped_warning(agent, &name);
    assert_eq!(warning.detail, LocationOutcome::OtherRepository.code());
    assert!(
        agent
            .locations
            .reports()
            .iter()
            .any(|report| matches!(report, LocationReport::OtherRepository { position: 1, .. }))
    );
}

#[then("nothing was written to the other repository")]
fn nothing_written(agent: &mut Agent) {
    let lan = &agent.locations.addresses["lan"];
    for access in agent.locations.opens() {
        assert_ne!(&access.repository, lan);
    }
}

#[then(
    expr = "the operation warns that the location {string} couldn't be tried because a secret was unavailable"
)]
fn warns_secret_unavailable(agent: &mut Agent, name: String) {
    let warning = skipped_warning(agent, &name);
    assert_eq!(warning.detail, LocationOutcome::SecretUnavailable.code());
}

#[then("the warning names the source and never a value")]
fn names_source(agent: &mut Agent) {
    let world = &agent.locations;
    let tried = world.record().at(world.position("lan")).expect("lan");
    let source = tried.source.as_deref().expect("the source");
    assert!(source.contains("LAN_KEY"), "{source}");
    assert!(!format!("{:?}", world.operation()).contains("demo-password"));
}

#[then("the operation ends as failed with a password error")]
fn fails_password(agent: &mut Agent) {
    match agent.locations.operation().status() {
        OperationStatus::Failed { reason } => assert_eq!(reason.code(), "password-not-accepted"),
        other => panic!("the operation should have failed, got {other:?}"),
    }
}

#[then(expr = "the operation doesn't end with the reason {string}")]
fn not_reason(agent: &mut Agent, reason: String) {
    if let OperationStatus::Failed { reason: failed } = agent.locations.operation().status() {
        assert_ne!(failed.code(), code(&reason));
    }
}

#[then(expr = "the operation ends as failed with the reason {string}")]
fn fails_with(agent: &mut Agent, reason: String) {
    match agent.locations.operation().status() {
        OperationStatus::Failed { reason: failed } => assert_eq!(failed.code(), code(&reason)),
        other => panic!("the operation should have failed, got {other:?}"),
    }
}

/// The code a repository of one location fails with when another repository
/// is at its location (§7.3).
#[then("the operation ends as failed because its location holds another repository")]
fn fails_as_replaced(agent: &mut Agent) {
    assert_eq!(
        agent.locations.operation().status(),
        &OperationStatus::Failed {
            reason: domain::execution::FailureReason::RepositoryReplaced,
        }
    );
}

#[then("the operation names each location and why it wasn't used")]
fn names_each(agent: &mut Agent) {
    let record = agent.locations.record();
    let outcomes: Vec<(u8, LocationOutcome)> = record
        .locations
        .iter()
        .map(|tried| (tried.position, tried.outcome))
        .collect();
    assert_eq!(
        outcomes,
        vec![(1, LocationOutcome::Silent), (2, LocationOutcome::Silent)]
    );
}

#[then(expr = "a retry of the plan {string} is scheduled")]
fn retry_scheduled(agent: &mut Agent, _plan: String) {
    let operation = agent.locations.operation();
    assert!(matches!(operation.status(), OperationStatus::Failed { .. }));
    assert!(RetrySchedule::retried(operation.trigger()));
}

#[then(expr = "no retry of the plan {string} is scheduled")]
fn no_retry(agent: &mut Agent, _plan: String) {
    let operation = agent.locations.operation();
    assert!(matches!(operation.status(), OperationStatus::Failed { .. }));
    assert!(!RetrySchedule::retried(operation.trigger()));
}

#[then(expr = "the repository was opened at the location {string} only")]
fn opened_only_at(agent: &mut Agent, name: String) {
    let world = &agent.locations;
    let address = &world.addresses[&name];
    let opens = world.opens();
    assert!(!opens.is_empty(), "the repository was opened");
    for access in opens {
        assert_eq!(&access.repository, address);
    }
}

#[then(expr = "the plan's retention was applied at the location {string}")]
fn retention_applied_at(agent: &mut Agent, name: String) {
    let world = &agent.locations;
    let applied: Vec<String> = world
        .retention
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|access| access.repository.clone())
        .collect();
    assert_eq!(applied, vec![world.addresses[&name].clone()]);
}

#[then(expr = "the location {string} was never opened")]
fn never_opened(agent: &mut Agent, name: String) {
    let address = &agent.locations.addresses[&name];
    for access in agent.locations.opens() {
        assert_ne!(&access.repository, address);
    }
}

#[then(expr = "the job's worker was given no secret of the location {string}")]
fn no_secret_of(agent: &mut Agent, name: String) {
    let world = &agent.locations;
    assert!(world.opens().iter().all(|access| {
        access.repository != world.addresses[&name]
            && !access
                .options
                .values()
                .any(|value| value.contains("canary-value"))
            && !access
                .secrets
                .iter()
                .any(|secret| secret.expose().contains("canary-value"))
    }));
    assert_eq!(world.record().used(), Some(world.position("lan")));
}

#[then("the canary appears in no log line, event, operation record or page")]
fn canary_nowhere(agent: &mut Agent) {
    let world = &agent.locations;
    let shown = [
        format!("{:?}", world.reports()),
        format!("{:?}", world.operation()),
        format!("{:?}", world.results),
        format!("{:?}", world.entries[NAS].location_warnings()),
        format!("{:?}", world.entries[NAS]),
    ];
    for text in shown {
        assert!(!text.contains("canary-value"), "{text}");
    }
    assert!(world.operation().locations().is_some());
}

#[then("no location was probed")]
fn none_probed(agent: &mut Agent) {
    assert_eq!(agent.locations.probes(), Vec::new());
}

#[then("the operation records no location")]
fn records_none(agent: &mut Agent) {
    assert_eq!(agent.locations.operation().locations(), None);
}

#[then("both backups read the cache slot named by the repository ID")]
fn both_same_slot(agent: &mut Agent) {
    let world = &agent.locations;
    let backups: Vec<RepositoryAccess> = world
        .jobs()
        .into_iter()
        .filter_map(|job| match job {
            Job::Backup(backup) => Some(backup.repository),
            _ => None,
        })
        .collect();
    assert_eq!(backups.len(), 2);
    assert_ne!(backups[0].repository, backups[1].repository);
    for access in backups {
        assert_eq!(access.id, Some(repository_id(REPOSITORY_ID)));
    }
}

#[then("no probe took a cache slot")]
fn no_probe_slot(agent: &mut Agent) {
    let probes = agent.locations.probes();
    assert!(!probes.is_empty());
    for probe in probes {
        assert_eq!(
            probe.id, None,
            "a probe names no repository, so it gets no cache"
        );
    }
}

#[then(
    expr = "the repository {string} carries the warning {string} for the location {int} with the cause {string}"
)]
fn carries_warning(agent: &mut Agent, _name: String, code: String, position: u8, cause: String) {
    assert_eq!(code, "location-plain-http-credentials");
    let listed = &agent.locations.listed;
    assert!(
        listed
            .iter()
            .any(|warning| warning.position == position && warning.cause.code() == cause),
        "{listed:?}"
    );
}

#[then("the warning names neither the address nor a credential")]
fn warning_names_neither(agent: &mut Agent) {
    let shown = format!("{:?}", agent.locations.listed);
    for hidden in ["lan.example.lan", "canary-pass", "s3.example.lan"] {
        assert!(!shown.contains(hidden), "{shown}");
    }
}

#[given(expr = "the plan {string} is scheduled and doesn't retry a failed run")]
fn scheduled_without_retry(agent: &mut Agent, plan: String) {
    assert_eq!(plan, PLAN);
    agent.locations.plan_retries = Some(false);
}

#[given(expr = "the plan {string} is scheduled and retries up to {int} times")]
fn scheduled_with_retry(agent: &mut Agent, plan: String, attempts: u32) {
    assert_eq!(plan, PLAN);
    assert!(attempts > 0);
    agent.locations.plan_retries = Some(true);
}

#[given("the machine can't tell a wired network from other links")]
fn cant_tell_wired(agent: &mut Agent) {
    agent.locations.cant_tell_wired = true;
}

/// What the Repositories page notes about the rules of the locations of
/// "nas", with the plan "home" as the scenario sets it.
fn rule_notes(agent: &Agent) -> Vec<RuleNote> {
    let world = &agent.locations;
    let mut plan = Plan::new(plan_id(PLAN), alias(NAS), Vec::new());
    if let Some(retries) = world.plan_retries {
        plan.schedule = Some(Schedule::new(Interval::parse("1d").expect("an interval")));
        plan.retry = retries
            .then(|| RetryPolicy::new(3, SignedDuration::from_mins(10)).expect("a retry policy"));
    }
    world.entries[NAS].rule_notes(&[&plan], !world.cant_tell_wired, true)
}

#[then(expr = "the repository {string} notes that every location has a rule")]
fn notes_every_ruled(agent: &mut Agent, _name: String) {
    let notes = rule_notes(agent);
    assert!(notes.contains(&RuleNote::EveryLocationRuled), "{notes:?}");
}

#[then(
    expr = "the repository {string} notes that its last location has a rule and the plan {string} doesn't retry"
)]
fn notes_last_ruled(agent: &mut Agent, _name: String, plan: String) {
    let notes = rule_notes(agent);
    let note = RuleNote::LastRuledWithoutRetry {
        plan: plan_id(&plan),
    };
    assert!(notes.contains(&note), "{notes:?}");
}

#[then(
    expr = "the repository {string} notes that the location {int}'s rule asks for a wired network, which this machine can't tell"
)]
fn notes_wired_unknown(agent: &mut Agent, _name: String, position: u8) {
    let notes = rule_notes(agent);
    assert!(
        notes.contains(&RuleNote::WiredUnknown { position }),
        "{notes:?}"
    );
}

#[then(expr = "the repository {string} carries no note about its locations' rules")]
fn carries_no_note(agent: &mut Agent, _name: String) {
    assert_eq!(rule_notes(agent), Vec::new());
}

#[then(expr = "the repository {string} carries no warning about its locations")]
fn carries_no_warning(agent: &mut Agent, _name: String) {
    assert_eq!(agent.locations.listed, Vec::new());
}

#[then(
    expr = "the daemon's log warns that the location {int} sends its credentials over plain HTTP"
)]
fn log_warns_plain_http(agent: &mut Agent, position: u8) {
    let reports = agent.locations.reports();
    assert!(
        reports.iter().any(|report| matches!(
            report,
            LocationReport::ExposedCredentials {
                position: at,
                cause: domain::planning::ExposedCredentials::PlainHttp,
                ..
            } if *at == position
        )),
        "{reports:?}"
    );
    let shown = format!("{reports:?}");
    assert!(!shown.contains("canary-pass"), "{shown}");
}

#[given(expr = "the location {string} is eligible only on a wired network")]
fn eligible_wired(agent: &mut Agent, name: String) {
    agent.locations.change_location(&name, |location| {
        location.when = Some(rule(&[NetworkKind::Wired]))
    });
}

#[given("both locations are eligible only on a wired network")]
fn both_eligible_wired(agent: &mut Agent) {
    for name in ["lan", "public"] {
        agent.locations.change_location(name, |location| {
            location.when = Some(rule(&[NetworkKind::Wired]))
        });
    }
}

#[given(expr = "the location {string} is eligible on a wired network or on any Wi-Fi network")]
fn eligible_wired_or_wifi(agent: &mut Agent, name: String) {
    agent.locations.change_location(&name, |location| {
        location.when = Some(rule(&[NetworkKind::Wired, NetworkKind::WiFi]));
    });
}

#[given(expr = "a repository {string} with one location that is eligible only on a wired network")]
fn one_location_eligible_wired(agent: &mut Agent, name: String) {
    repository_with_one_location(agent, name.clone());
    let entry = agent
        .locations
        .entries
        .get_mut(&name)
        .expect("the repository");
    entry.locations.first_when = Some(rule(&[NetworkKind::Wired]));
}

#[given(expr = "the location {string} is eligible only on the Wi-Fi network {string}")]
fn eligible_only_on(agent: &mut Agent, name: String, network: String) {
    agent.locations.change_location(&name, |location| {
        location.when = Some(named_rule(NameFilter::Only(vec![network_name(&network)])));
    });
}

#[given(expr = "the location {string} is eligible on any Wi-Fi network except {string}")]
fn eligible_except(agent: &mut Agent, name: String, network: String) {
    agent.locations.change_location(&name, |location| {
        location.when = Some(named_rule(NameFilter::Except(vec![network_name(&network)])));
    });
}

#[given(expr = "the machine is on the Wi-Fi network {string}")]
fn on_named_wifi(agent: &mut Agent, network: String) {
    let name = CurrentNetworkName::new(network.as_bytes()).expect("a network's name");
    machine_named(agent, Ok(Some(name)));
}

#[given("the machine can't read the Wi-Fi network's name")]
fn name_unreadable(agent: &mut Agent) {
    machine_named(
        agent,
        Err(PlatformError::Unreadable {
            from: "the demo machine".to_owned(),
            detail: "the name is hidden".to_owned(),
        }),
    );
}

#[then(expr = "{string} appears in no log line, event or operation record")]
fn network_name_nowhere(agent: &mut Agent, network: String) {
    let world = &agent.locations;
    assert!(
        world.link.names_asked() > 0,
        "the name was read, so the check means something"
    );
    let shown = [
        format!("{:?}", world.reports()),
        format!("{:?}", world.operations),
        format!("{:?}", world.results),
        format!("{:?}", world.jobs()),
    ];
    for text in shown {
        assert!(!text.contains(&network), "{text}");
    }
    let lan = world.record().at(world.position("lan")).expect("the location");
    assert_eq!(lan.outcome, LocationOutcome::RuleNotMet);
}

#[given("the machine is on Wi-Fi")]
fn on_wifi(agent: &mut Agent) {
    machine_on(agent, Ok(Link::WiFi));
}

#[given("the machine is on a wired network")]
fn on_wired(agent: &mut Agent) {
    machine_on(agent, Ok(Link::Wired));
}

#[given("the machine can't tell what kind of network it is on")]
fn link_unknown(agent: &mut Agent) {
    machine_on(
        agent,
        Err(PlatformError::Unreadable {
            from: "the demo machine".to_owned(),
            detail: "nothing to read".to_owned(),
        }),
    );
}

#[given(expr = "a run of the plan {string} is queued while the machine is on a wired network")]
async fn queued_on_wired(agent: &mut Agent, plan: String) {
    assert_eq!(plan, PLAN);
    machine_on(agent, Ok(Link::Wired));
    let rules = agent.locations.plan_rules().expect("the repository nas");
    let mut reading = LinkReading::new(agent.locations.link());
    assert_eq!(check_location_rules(&rules, &mut reading).await, Ok(()));
}

#[when("the machine moves to Wi-Fi before the run starts")]
async fn moves_to_wifi(agent: &mut Agent) {
    machine_on(agent, Ok(Link::WiFi));
    run_backup(agent, NAS, Trigger::Schedule).await;
}

/// Runs the backup of the plan "home" the scheduler started.
pub(super) async fn run_scheduled(agent: &mut Agent) {
    run_backup(agent, NAS, Trigger::Schedule).await;
}

#[when(expr = "a person tests the locations of the repository {string}")]
async fn person_tests(agent: &mut Agent, repository: String) {
    let world = &agent.locations;
    let saved = SavedEntry {
        alias: alias(&repository),
        entry: world.entries[&repository].clone(),
    };
    let tested = world
        .engine(agent.clock.clone())
        .test_each(OperationId::from_uuid(Uuid::from_u128(0x0600)), &saved)
        .await
        .expect("the test runs");
    agent.locations.tested = Some(tested);
}

fn tested_outcome(agent: &Agent, name: &str) -> LocationOutcome {
    let world = &agent.locations;
    world
        .tested
        .as_ref()
        .expect("a person tested the locations")
        .at(world.position(name))
        .expect("the location")
        .outcome
}

#[then(regex = r#"^the test says the location "([^"]+)" was skipped because (.+)$"#)]
fn test_says_skipped(agent: &mut Agent, name: String, reason: String) {
    assert_eq!(tested_outcome(agent, &name), outcome(&reason));
}

#[then(expr = "the test says the location {string} answered")]
fn test_says_answered(agent: &mut Agent, name: String) {
    assert_eq!(tested_outcome(agent, &name), LocationOutcome::Used);
}

#[then(expr = "the location {string} was never probed")]
fn never_probed(agent: &mut Agent, name: String) {
    let address = &agent.locations.addresses[&name];
    let probes = agent.locations.probes();
    assert!(
        probes.iter().all(|probe| &probe.repository != address),
        "{name} was probed: {probes:?}"
    );
}

#[then(expr = "the location {string} is probed")]
fn is_probed(agent: &mut Agent, name: String) {
    let address = &agent.locations.addresses[&name];
    assert!(
        agent
            .locations
            .probes()
            .iter()
            .any(|probe| &probe.repository == address),
        "{name} was never probed"
    );
}

#[then(expr = "the operation warns that the location {string} was skipped though its rule held")]
fn warns_skipped_though_held(agent: &mut Agent, name: String) {
    let warning = skipped_warning(agent, &name);
    assert_eq!(warning.detail, LocationOutcome::Silent.code());
}

#[then("the operation names each location and why its rule didn't hold")]
fn names_each_rule(agent: &mut Agent) {
    let outcomes: Vec<(u8, LocationOutcome)> = agent
        .locations
        .record()
        .locations
        .iter()
        .map(|tried| (tried.position, tried.outcome))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            (1, LocationOutcome::RuleNotMet),
            (2, LocationOutcome::RuleNotMet)
        ]
    );
}

#[then("no operation is recorded")]
fn no_operation(agent: &mut Agent) {
    assert_eq!(agent.locations.operations.len(), 0);
    assert!(agent.locations.jobs().is_empty());
}
