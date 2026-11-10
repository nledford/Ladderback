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

use super::{Agent, blocking};

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
    /// The client sends a password the repository doesn't have.
    api_wrong_password: bool,
    /// A file holding the right password, when the client names a file.
    api_password_file: Option<PathBuf>,
    /// Every body the daemon answered, to check that none holds a password.
    api_answers: Vec<String>,
    /// The last answer: its status and body.
    api_last: Option<(u16, String)>,
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
async fn initialised_repository(agent: &mut Agent) {
    empty_location(agent);
    let cold = agent.repositories.cold.clone().expect("the location");
    let hot = agent.repositories.hot.clone();
    // Deriving the repository's key takes a second in a debug build: see
    // [`blocking`].
    blocking(move || {
        repository(&cold, hot.as_deref())
            .init(
                &credentials(),
                &rustic_core::KeyOptions::default(),
                &rustic_core::ConfigOptions::default(),
            )
            .expect("the scratch repository initialises");
    })
    .await;
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
    let use_case = app::repositories::TestRepository::new(
        Arc::new(platform::SystemClock),
        agent.engine.clone(),
        Agent::resolver(),
    );
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
        Arc::new(platform::SystemClock),
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
        Arc::new(platform::SystemClock),
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

/// The repository's ID as rustic knows it, straight from its config. Opening
/// it derives its key, which takes a second in a debug build: see [`blocking`].
async fn rustic_id(agent: &Agent) -> String {
    let cold = agent.repositories.cold.clone().expect("the location");
    let hot = agent.repositories.hot.clone();
    blocking(move || {
        repository(&cold, hot.as_deref())
            .open(&credentials())
            .expect("the scratch repository opens")
            .config()
            .id
            .to_hex()
            .to_string()
    })
    .await
}

async fn config(agent: &Agent) -> domain::planning::AgentConfig {
    agent
        .config_store()
        .load()
        .await
        .expect("the configuration loads")
}

#[then("the initialisation reports the repository's ID")]
async fn init_reports_id(agent: &mut Agent) {
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
    assert_eq!(id.as_str(), rustic_id(agent).await);
}

#[then("the repository at the location opens with the password")]
async fn opens_with_password(agent: &mut Agent) {
    let id = rustic_id(agent).await;
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
    assert_eq!(id.as_str(), rustic_id(agent).await);
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

// The same behaviours over HTTP, for `features/repositories/api.feature`: the
// real daemon, spoken to as the Repositories page speaks to it. The
// repositories are still made and inspected with `rustic_core` itself.

use super::operations_steps::{exchange, exchange_at};

/// The settings a request carries: the scenario's location, its hot half when
/// it has one, and the password the client has.
fn api_settings(agent: &Agent) -> serde_json::Value {
    let cold = agent
        .repositories
        .cold
        .as_ref()
        .expect("the scenario set a location");
    let password = match &agent.repositories.api_password_file {
        Some(path) => serde_json::json!({"source": "file", "path": text(path)}),
        None if agent.repositories.api_wrong_password => {
            serde_json::json!({"source": "inline", "value": api_secret(agent)})
        }
        None => serde_json::json!({"source": "inline", "value": PASSWORD}),
    };
    let mut settings = serde_json::json!({"repository": text(cold), "password": password});
    if let Some(hot) = &agent.repositories.hot {
        settings["repo-hot"] = serde_json::json!(text(hot));
    }
    settings
}

/// The password the client types: the repository's, or the wrong one.
fn api_secret(agent: &Agent) -> &'static str {
    if agent.repositories.api_wrong_password {
        "not-the-password"
    } else {
        PASSWORD
    }
}

/// Posts `body` to `path`, remembering the answer. A request that opens or
/// creates a repository takes seconds, so the wait is on a blocking thread:
/// see [`blocking`].
async fn api_post(agent: &mut Agent, path: &str, body: &str) {
    let addr = agent.events.addr();
    let (path, body) = (path.to_owned(), body.to_owned());
    let (status, answer) = blocking(move || exchange_at(addr, "POST", &path, &body)).await;
    agent.repositories.api_answers.push(answer.clone());
    agent.repositories.api_last = Some((status, answer));
}

/// The last answer's status and its body as JSON.
fn api_answer(agent: &Agent) -> (u16, serde_json::Value) {
    let (status, body) = agent
        .repositories
        .api_last
        .as_ref()
        .expect("the client has made a request");
    // A route that isn't there answers with no body; its status says so.
    let json = if body.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body)
            .unwrap_or_else(|error| panic!("the answer is JSON: {error}: {body}"))
    };
    (*status, json)
}

/// The daemon's list of repositories, remembered like any answer.
fn api_list(agent: &mut Agent) -> Vec<serde_json::Value> {
    let (status, body) = exchange(agent, "GET", "/api/v1/repositories", "");
    assert_eq!(status, 200, "the list answers: {body}");
    agent.repositories.api_answers.push(body.clone());
    let list: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("the list is JSON: {error}"));
    list["repositories"]
        .as_array()
        .unwrap_or_else(|| panic!("the list has rows: {body}"))
        .clone()
}

#[given("the client uses the wrong password")]
fn api_uses_wrong_password(agent: &mut Agent) {
    agent.repositories.api_wrong_password = true;
}

#[given("the client's password is in a file")]
fn api_password_in_file(agent: &mut Agent) {
    let path = agent.scratch().join("password.txt");
    fs::write(&path, PASSWORD).expect("the password file");
    agent.repositories.api_password_file = Some(path);
}

#[given("the client's repository is at another, empty location")]
fn api_other_empty_location(agent: &mut Agent) {
    let other = agent.scratch().join("other-repo");
    fs::create_dir_all(&other).expect("the other repository folder");
    agent.repositories.cold = Some(other);
}

#[given(expr = "the client has connected it as {string}")]
async fn api_has_connected(agent: &mut Agent, alias: String) {
    api_connect(agent, &alias).await;
    assert_eq!(api_answer(agent).0, 201, "the first connection succeeds");
}

async fn api_connect(agent: &mut Agent, alias: &str) {
    let body = serde_json::json!({"alias": alias, "settings": api_settings(agent)});
    api_post(agent, "/api/v1/repositories", &body.to_string()).await;
}

#[when("the client tests the repository")]
async fn api_tests(agent: &mut Agent) {
    let body = serde_json::json!({"settings": api_settings(agent)});
    api_post(agent, "/api/v1/repositories/_test", &body.to_string()).await;
}

#[when(expr = "the client connects it as {string}")]
async fn api_connects(agent: &mut Agent, alias: String) {
    api_connect(agent, &alias).await;
}

#[when(expr = "the client connects it as {string} and as {string} at the same moment")]
async fn api_connects_at_once(agent: &mut Agent, first: String, second: String) {
    let addr = agent.events.addr();
    let settings = api_settings(agent);
    let requests = [first, second].into_iter().map(|alias| {
        let body = serde_json::json!({"alias": alias, "settings": settings}).to_string();
        blocking(move || exchange_at(addr, "POST", "/api/v1/repositories", &body))
    });
    for (status, body) in futures::future::join_all(requests).await {
        assert_eq!(status, 201, "a connection made at the same moment: {body}");
        agent.repositories.api_answers.push(body);
    }
}

#[when("the client initialises the repository")]
async fn api_initialises(agent: &mut Agent) {
    api_initialise(agent, None).await;
}

#[when(expr = "the client initialises the repository as {string}")]
async fn api_initialises_as(agent: &mut Agent, alias: String) {
    api_initialise(agent, Some(alias)).await;
}

async fn api_initialise(agent: &mut Agent, alias: Option<String>) {
    agent.repositories.before = agent
        .repositories
        .cold
        .as_ref()
        .map(|cold| contents(cold))
        .unwrap_or_default();
    let mut body = serde_json::json!({"settings": api_settings(agent)});
    if let Some(alias) = alias {
        body["alias"] = serde_json::json!(alias);
    }
    api_post(agent, "/api/v1/repositories/_init", &body.to_string()).await;
}

#[when(regex = r#"^the client sends (.+) to "([^"]+)"$"#)]
async fn api_sends(agent: &mut Agent, body: String, route: String) {
    api_post(agent, &route, &body).await;
}

#[then(expr = "the answer has status {int} and the repository's ID")]
async fn api_answers_with_id(agent: &mut Agent, status: u16) {
    let (actual, json) = api_answer(agent);
    assert_eq!(actual, status, "the answer: {json}");
    assert_eq!(json["id"], serde_json::json!(rustic_id(agent).await));
}

#[then(expr = "the answer has status {int} and the repository's ID, not saved")]
async fn api_answers_with_id_not_saved(agent: &mut Agent, status: u16) {
    api_answers_with_id(agent, status).await;
    assert_eq!(api_answer(agent).1["saved"], serde_json::json!(false));
}

#[then(expr = "the answer has status {int} and the repository's ID, saved")]
async fn api_answers_with_id_saved(agent: &mut Agent, status: u16) {
    api_answers_with_id(agent, status).await;
    assert_eq!(api_answer(agent).1["saved"], serde_json::json!(true));
}

#[then(expr = "the refusal has status {int} and the typed code {string}")]
fn api_refused(agent: &mut Agent, status: u16, code: String) {
    let (actual, json) = api_answer(agent);
    assert_eq!(actual, status, "the refusal: {json}");
    assert_eq!(json["code"], serde_json::json!(code), "the refusal: {json}");
}

#[then("the refusal carries the engine's detail")]
fn api_refusal_has_detail(agent: &mut Agent) {
    let (_, json) = api_answer(agent);
    assert!(
        json["detail"]
            .as_str()
            .is_some_and(|detail| !detail.trim().is_empty()),
        "the refusal has no detail: {json}"
    );
}

#[then("no answer holds the password")]
fn api_no_answer_holds_password(agent: &mut Agent) {
    let secret = api_secret(agent);
    for answer in &agent.repositories.api_answers {
        assert!(
            !answer.contains(secret),
            "an answer holds the password: {answer}"
        );
    }
}

#[then("the daemon lists no repositories")]
fn api_lists_none(agent: &mut Agent) {
    assert!(api_list(agent).is_empty(), "the daemon lists a repository");
}

#[then(expr = "the daemon lists {int} repository")]
#[then(expr = "the daemon lists {int} repositories")]
fn api_lists_count(agent: &mut Agent, count: usize) {
    assert_eq!(api_list(agent).len(), count);
}

/// The listed row of `alias`, which must be there.
fn api_row(agent: &mut Agent, alias: &str) -> serde_json::Value {
    let rows = api_list(agent);
    rows.iter()
        .find(|row| row["alias"].as_str() == Some(alias))
        .cloned()
        .unwrap_or_else(|| panic!("the list has no repository {alias}: {rows:?}"))
}

#[then(
    expr = "the daemon lists the repository {string} at the location, its password stored inline"
)]
fn api_lists_inline(agent: &mut Agent, alias: String) {
    let location = text(agent.repositories.cold.as_ref().expect("the location"));
    let row = api_row(agent, &alias);
    assert_eq!(row["repository"], serde_json::json!(location));
    assert_eq!(row["password"], serde_json::json!({"source": "inline"}));
}

#[then(
    expr = "the daemon lists the repository {string} with a warning that its password is written in the config"
)]
fn api_lists_inline_warning(agent: &mut Agent, alias: String) {
    let row = api_row(agent, &alias);
    assert_eq!(
        row["warnings"],
        serde_json::json!([{
            "code": "config-inline-secret",
            "args": {"field": {"type": "text", "value": "password"}}
        }])
    );
}

#[then(expr = "the daemon lists the repository {string} at the location, its password in the file")]
fn api_lists_file(agent: &mut Agent, alias: String) {
    let location = text(agent.repositories.cold.as_ref().expect("the location"));
    let file = text(
        agent
            .repositories
            .api_password_file
            .as_ref()
            .expect("the password file"),
    );
    let row = api_row(agent, &alias);
    assert_eq!(row["repository"], serde_json::json!(location));
    assert_eq!(
        row["password"],
        serde_json::json!({"source": "file", "path": file})
    );
}

#[then(expr = "the daemon's config file doesn't hold {string}")]
fn api_config_file_lacks(agent: &mut Agent, secret: String) {
    let file = super::events_steps::Events::config_dir(agent).join("config.toml");
    let text = fs::read_to_string(&file).expect("the daemon saved its config file");
    assert!(!text.contains(&secret), "the config file holds {secret:?}");
}

#[then("the hot and the cold half each hold the repository's config")]
fn api_both_halves_hold_config(agent: &mut Agent) {
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
