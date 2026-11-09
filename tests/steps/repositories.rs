//! Steps for the `@e2e` scenarios of `features/repositories/connect.feature`:
//! the use cases run against the real worker and real scratch repositories,
//! and the configuration is the real file in the scenario's scratch folder.
//! The repositories are made and inspected with `rustic_core` itself, never
//! through Ladderback, so the checks don't trust the code under test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use app::ports::ConfigStore;
use app::repositories::{InitOutcome, RepositoryError};
use app::secrets::ResolveRepository;
use config_adapter::FileConfigStore;
use cucumber::{given, then, when};
use domain::planning::RepositorySettings;
use domain::secrets::{FileRead, SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, RepositoryId, Uuid};
use platform::SystemClock;
use secrets::SystemSecrets;

use super::Agent;

/// Not a real secret: the scratch repositories' password.
const PASSWORD: &str = "demo-password";

fn credentials() -> rustic_core::Credentials {
    rustic_core::Credentials::password(PASSWORD)
}

/// The scratch repository, without a cache.
fn repository(cold: &Path, hot: Option<&Path>) -> rustic_core::Repository<()> {
    let options = rustic_backend::BackendOptions::default()
        .repository(cold.to_str().expect("a UTF-8 scratch path"));
    let options = match hot {
        Some(hot) => options.repo_hot(hot.to_str().expect("a UTF-8 scratch path")),
        None => options,
    };
    let backends = options
        .to_backends()
        .expect("the scratch repository's backend");
    rustic_core::Repository::new(
        &rustic_core::RepositoryOptions::default().no_cache(true),
        &backends,
    )
    .expect("the scratch repository")
}

/// Every file's name and bytes under `dir`, sorted: the repository as it
/// stands, for checking that a refused initialisation wrote nothing.
fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries = fs::read_dir(dir)
            .expect("the repository folder")
            .collect::<Result<Vec<_>, _>>()
            .expect("the repository folder");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push((
                    path.display().to_string(),
                    fs::read(&path).expect("the file"),
                ));
            }
        }
    }
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, &mut out);
    }
    out
}

/// What the scenarios set up, and what the use cases gave.
#[derive(Default)]
pub struct RepositoriesSetup {
    cold: Option<PathBuf>,
    hot: Option<PathBuf>,
    settings: Option<RepositorySettings>,
    init: Option<Result<InitOutcome, RepositoryError>>,
    test: Option<Result<RepositoryId, RepositoryError>>,
    connect: Option<Result<RepositoryId, RepositoryError>>,
    /// Every repository folder's contents, as a refusal found them.
    before: Vec<(String, Vec<u8>)>,
}

impl Agent {
    fn settings(&mut self) -> &mut RepositorySettings {
        self.repositories
            .settings
            .get_or_insert_with(|| RepositorySettings {
                repository: String::new(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new(PASSWORD)),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            })
    }

    /// A fresh operation ID for each use-case call.
    fn next_op(&mut self) -> OperationId {
        self.runs += 1;
        OperationId::from_uuid(Uuid::from_u128(self.runs))
    }

    fn config_store(&self) -> Arc<dyn ConfigStore> {
        Arc::new(FileConfigStore::new(
            self.scratch().join("config").join("config.toml"),
            Arc::new(SystemClock),
        ))
    }

    fn resolver() -> ResolveRepository {
        ResolveRepository::new(Arc::new(SystemSecrets::new()))
    }
}

fn text(path: &Path) -> String {
    path.to_str().expect("a UTF-8 scratch path").to_owned()
}

#[given("an empty repository location")]
fn empty_location(agent: &mut Agent) {
    let cold = agent.scratch().join("repo");
    fs::create_dir_all(&cold).expect("the repository folder");
    agent.repositories.cold = Some(cold.clone());
    agent.settings().repository = text(&cold);
}

#[given("an empty hot/cold repository location")]
fn empty_hot_cold_location(agent: &mut Agent) {
    empty_location(agent);
    let hot = agent.scratch().join("hot");
    fs::create_dir_all(&hot).expect("the hot half's folder");
    agent.repositories.hot = Some(hot.clone());
    agent.settings().repo_hot = Some(text(&hot));
}

#[given("an initialised repository")]
fn initialised_repository(agent: &mut Agent) {
    empty_location(agent);
    let cold = agent.repositories.cold.clone().expect("the location");
    let hot = agent.repositories.hot.clone();
    repository(&cold, hot.as_deref())
        .init(
            &credentials(),
            &rustic_core::KeyOptions::default(),
            &rustic_core::ConfigOptions::default(),
        )
        .expect("the scratch repository initialises");
}

#[given("the agent uses the wrong password")]
fn wrong_password(agent: &mut Agent) {
    agent.settings().password = SecretRef::Inline(SecretValue::new("not-the-password"));
}

#[given("the repository's password is in a file")]
fn password_in_file(agent: &mut Agent) {
    let path = agent.scratch().join("password.txt");
    fs::write(&path, PASSWORD).expect("the password file");
    agent.settings().password = SecretRef::File {
        path: text(&path),
        read: FileRead::FirstLine,
    };
}

fn settings(agent: &Agent) -> RepositorySettings {
    agent
        .repositories
        .settings
        .clone()
        .expect("the scenario set a location")
}

#[when("the agent tests the repository")]
async fn tests(agent: &mut Agent) {
    let use_case = app::repositories::TestRepository::new(agent.engine.clone(), Agent::resolver());
    agent.repositories.test = Some(use_case.test(agent.next_op(), &settings(agent)).await);
}

#[when("the agent initialises the repository")]
async fn initialises(agent: &mut Agent) {
    initialise(agent, None).await;
}

#[when(expr = "the agent initialises the repository as {string}")]
async fn initialises_as(agent: &mut Agent, name: String) {
    let alias = RepositoryAlias::try_from(name).expect("a valid alias");
    initialise(agent, Some(alias)).await;
}

async fn initialise(agent: &mut Agent, alias: Option<RepositoryAlias>) {
    agent.repositories.before = agent
        .repositories
        .cold
        .as_ref()
        .map(|cold| contents(cold))
        .unwrap_or_default();
    let use_case = app::repositories::InitRepository::new(
        agent.engine.clone(),
        Agent::resolver(),
        agent.config_store(),
    );
    agent.repositories.init = Some(
        use_case
            .init(agent.next_op(), &settings(agent), alias)
            .await,
    );
}

#[when(expr = "the agent connects it as {string}")]
async fn connects(agent: &mut Agent, name: String) {
    let alias = RepositoryAlias::try_from(name).expect("a valid alias");
    let use_case = app::repositories::ConnectRepository::new(
        agent.engine.clone(),
        Agent::resolver(),
        agent.config_store(),
    );
    agent.repositories.connect = Some(
        use_case
            .connect(agent.next_op(), alias, &settings(agent))
            .await,
    );
}

/// The repository's ID as rustic knows it, straight from its config.
fn rustic_id(agent: &Agent) -> String {
    let cold = agent.repositories.cold.as_ref().expect("the location");
    repository(cold, agent.repositories.hot.as_deref())
        .open(&credentials())
        .expect("the scratch repository opens")
        .config()
        .id
        .to_hex()
        .to_string()
}

async fn config(agent: &Agent) -> domain::planning::AgentConfig {
    agent
        .config_store()
        .load()
        .await
        .expect("the configuration loads")
}

#[then("the initialisation reports the repository's ID")]
fn init_reports_id(agent: &mut Agent) {
    let id = agent
        .repositories
        .init
        .as_ref()
        .expect("the initialisation has run")
        .as_ref()
        .expect("the initialisation succeeded")
        .id
        .clone();
    assert_eq!(id.as_str().len(), 64, "the ID is 64 hex characters");
    assert_eq!(id.as_str(), rustic_id(agent));
}

#[then("the repository at the location opens with the password")]
fn opens_with_password(agent: &mut Agent) {
    let id = rustic_id(agent);
    assert_eq!(id.len(), 64);
}

#[then("the initialisation is refused because a repository already exists")]
fn init_refused(agent: &mut Agent) {
    assert!(matches!(
        agent.repositories.init.as_ref().expect("the run"),
        Err(RepositoryError::Exists)
    ));
}

#[then("the repository is unchanged")]
fn unchanged(agent: &mut Agent) {
    let cold = agent.repositories.cold.as_ref().expect("the location");
    assert_eq!(
        contents(cold),
        agent.repositories.before,
        "the initialisation wrote something"
    );
}

#[then("both halves hold the repository's config")]
fn both_halves_hold_config(agent: &mut Agent) {
    agent
        .repositories
        .init
        .as_ref()
        .expect("the run")
        .as_ref()
        .expect("the initialisation succeeded");
    for half in [
        agent.repositories.cold.as_ref().expect("the location"),
        agent.repositories.hot.as_ref().expect("the hot half"),
    ] {
        assert!(
            half.join("config").is_file(),
            "the {} half holds no config",
            half.display()
        );
    }
}

#[then("the test reports the repository's ID")]
async fn test_reports_id(agent: &mut Agent) {
    let id = agent
        .repositories
        .test
        .as_ref()
        .expect("the test has run")
        .as_ref()
        .expect("the test succeeded")
        .clone();
    assert_eq!(id.as_str(), rustic_id(agent));
}

#[then("the test fails")]
fn test_fails(agent: &mut Agent) {
    assert!(
        agent
            .repositories
            .test
            .as_ref()
            .expect("the test has run")
            .is_err(),
        "the test succeeded"
    );
}

#[then("the agent has no repository configured")]
async fn none_configured(agent: &mut Agent) {
    let config = config(agent).await;
    assert!(config.repositories().is_empty(), "something was saved");
}

#[then(expr = "the agent has the repository {string} configured with its settings")]
async fn configured_with_settings(agent: &mut Agent, name: String) {
    let alias = RepositoryAlias::try_from(name).expect("a valid alias");
    let config = config(agent).await;
    let repository = config.repository(&alias).expect("the repository was saved");
    assert_eq!(repository.settings, settings(agent));
}
