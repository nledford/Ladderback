//! Steps for the `@e2e` secrets scenarios: the settings go through the use
//! case and a real resolver that reads real files, and a real worker runs the
//! job, so the scenarios follow the whole way a secret takes (invariant I4).
//! The scratch backends never connect: the first thing that fails is a
//! refused connection to `127.0.0.1:9`, where nothing listens, and an error
//! that quotes the URL it called is what could show a secret.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::Duration;

use app::ports::Job;
use app::secrets::ResolveRepository;
use cucumber::{given, then, when};
use domain::execution::{FailureReason, OperationStatus};
use domain::planning::RepositorySettings;
use domain::secrets::{SecretRef, SecretValue};
use engine::WorkerEngine;
use secrets::SystemSecrets;

use super::Agent;

/// Not a real secret: the repository's password in these scenarios.
const PASSWORD: &str = "demo-password";

/// What the stand-in worker does: keeps its input, its environment and its
/// arguments in the directory it is given, then ends without a result.
const STAND_IN_WORKER: &str = "cat > \"$1/spec.json\"; env > \"$1/environment.txt\"; \
     printf '%s\\n' \"$0\" \"$@\" > \"$1/arguments.txt\"";

/// Everything the daemon logged for `ladderback::worker`.
static WORKER_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct CapturingLogger;

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "ladderback::worker"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            WORKER_LOG
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn capture_the_worker_log() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        log::set_logger(&CapturingLogger)
            .expect("another logger was installed first, so the worker's log can't be captured");
        log::set_max_level(log::LevelFilter::Debug);
    });
}

/// What a scenario sets up before the repository is identified.
#[derive(Debug, Default)]
pub struct SecretsSetup {
    repository: String,
    repo_hot: Option<String>,
    options: BTreeMap<String, String>,
    options_hot: BTreeMap<String, String>,
    /// The daemon's environment: what `${VAR}` reads, and what a worker is
    /// started from.
    environment: Vec<(String, String)>,
    /// Where a stand-in worker keeps what it was given, when one is used.
    stand_in: Option<PathBuf>,
    /// The debug form of the resolved repository.
    resolved: String,
}

fn options_of(options: &[(&str, &str)]) -> BTreeMap<String, String> {
    options
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

impl SecretsSetup {
    fn backend(&mut self, repository: &str, options: &[(&str, &str)]) {
        self.repository = repository.to_owned();
        self.options = options_of(options);
    }

    fn hot_backend(&mut self, repository: &str, options: &[(&str, &str)]) {
        self.repo_hot = Some(repository.to_owned());
        self.options_hot = options_of(options);
    }
}

/// `{scratch}` in a step's text stands for the scenario's scratch folder.
fn in_scratch(agent: &Agent, text: &str) -> String {
    text.replace("{scratch}", &agent.scratch().display().to_string())
}

#[given(expr = "the file {string} holds {string}")]
fn file_holds(agent: &mut Agent, path: String, contents: String) {
    let path = PathBuf::from(in_scratch(agent, &path));
    fs::create_dir_all(path.parent().expect("a path with a folder")).unwrap();
    fs::write(&path, contents).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[given(expr = "the daemon's environment has the variable {string} set to {string}")]
fn environment_has(agent: &mut Agent, name: String, value: String) {
    agent.secrets.environment.push((name, value));
}

/// Without credentials S3 looks for them in the environment and on the
/// network; these options say not to, so the connection is what fails first.
const S3_OPTIONS: [(&str, &str); 7] = [
    ("endpoint", "http://127.0.0.1:9"),
    ("bucket", "demo"),
    ("region", "us-east-1"),
    ("retry", "0"),
    ("disable_config_load", "true"),
    ("disable_ec2_metadata", "true"),
    ("allow_anonymous", "true"),
];

#[given("an S3 repository on a port where nothing listens")]
fn s3(agent: &mut Agent) {
    agent.secrets.backend("opendal:s3", &S3_OPTIONS);
}

#[given("a hot S3 repository on a port where nothing listens")]
fn hot_s3(agent: &mut Agent) {
    agent.secrets.hot_backend("opendal:s3", &S3_OPTIONS);
}

#[given("an Azure Blob repository on a port where nothing listens")]
fn azure(agent: &mut Agent) {
    agent.secrets.backend(
        "opendal:azblob",
        &[
            ("endpoint", "http://127.0.0.1:9"),
            ("container", "demo"),
            ("account_name", "demo"),
            ("retry", "0"),
        ],
    );
}

/// A cold half that never answers, so what fails first is the hot half.
#[given("a repository in a folder that isn't there")]
fn nowhere(agent: &mut Agent) {
    let folder = agent.scratch().join("no-such-repository");
    agent.secrets.repository = folder.display().to_string();
}

#[given(expr = "the repository's option {string} is {string}")]
fn option_is(agent: &mut Agent, name: String, value: String) {
    let value = in_scratch(agent, &value);
    agent.secrets.options.insert(name, value);
}

#[given(expr = "the hot repository is {string}")]
fn hot_repository_is(agent: &mut Agent, repository: String) {
    agent.secrets.repo_hot = Some(repository);
}

#[given(expr = "the hot repository's option {string} is {string}")]
fn hot_option_is(agent: &mut Agent, name: String, value: String) {
    let value = in_scratch(agent, &value);
    agent.secrets.options_hot.insert(name, value);
}

/// The worker is a shell script that writes down what it was given; it is
/// started from the daemon's environment as the scenario describes it.
#[given("a worker that writes down what it was given")]
fn stand_in_worker(agent: &mut Agent) {
    let folder = agent.scratch().join("worker-saw");
    fs::create_dir_all(&folder).unwrap();
    agent.secrets.stand_in = Some(folder);
}

#[when("the repository is identified")]
async fn identified(agent: &mut Agent) {
    capture_the_worker_log();
    let mut environment: Vec<(OsString, OsString)> = agent
        .secrets
        .environment
        .iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect();
    if let Some(folder) = agent.secrets.stand_in.clone() {
        environment.push(("PATH".into(), std::env::var_os("PATH").unwrap_or_default()));
        environment.push(("HOME".into(), "/home/demo".into()));
        let arguments: Vec<OsString> = vec![
            "-c".into(),
            STAND_IN_WORKER.into(),
            "stand-in".into(),
            folder.into_os_string(),
        ];
        agent.engine = Arc::new(WorkerEngine::new("sh", arguments).with_environment(environment));
    }
    let resolver = SystemSecrets::new().with_environment(agent.secrets.environment.iter().cloned());
    let settings = RepositorySettings {
        repository: agent.secrets.repository.clone(),
        repo_hot: agent.secrets.repo_hot.clone(),
        password: SecretRef::Inline(SecretValue::new(PASSWORD)),
        options: agent.secrets.options.clone(),
        options_hot: agent.secrets.options_hot.clone(),
        options_cold: BTreeMap::new(),
    };
    let resolved = ResolveRepository::new(Arc::new(resolver))
        .resolve(&settings)
        .await
        .expect("the settings resolve");
    agent.secrets.resolved = format!("{resolved:?}");
    agent
        .run(Job::Identify(resolved.access), futures::future::pending())
        .await;
    // The daemon reads the worker's stderr beside its events.
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[then(expr = "the operation ends as failed because the connection to {string} was refused")]
fn refused(agent: &mut Agent, address: String) {
    let status = agent.operation().status();
    let OperationStatus::Failed {
        reason: FailureReason::EngineFailed { detail },
    } = status
    else {
        panic!("the operation did not fail as the engine reported: {status:?}");
    };
    assert!(
        detail.contains(&address),
        "the failure is not about {address}: {detail}"
    );
}

#[then(expr = "the failure shows {string}")]
fn failure_shows(agent: &mut Agent, text: String) {
    let status = agent.operation().status();
    let OperationStatus::Failed {
        reason: FailureReason::EngineFailed { detail },
    } = status
    else {
        panic!("the operation did not fail as the engine reported: {status:?}");
    };
    assert!(detail.contains(&text), "{text} is not in: {detail}");
}

/// The operation, the debug form of what was resolved and everything the
/// daemon logged for the worker.
#[then(expr = "nothing the engine reported or logged shows {string}")]
fn nothing_shows(agent: &mut Agent, secret: String) {
    let operation = format!("{:?}", agent.operation());
    assert!(!operation.contains(&secret), "{secret} is in: {operation}");
    assert!(
        !agent.secrets.resolved.contains(&secret),
        "{secret} is in: {}",
        agent.secrets.resolved
    );
    let logged = WORKER_LOG.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        !logged.is_empty(),
        "the log captured nothing, so it was not checked"
    );
    for line in logged.iter() {
        assert!(!line.contains(&secret), "{secret} is in the log: {line}");
    }
}

fn worker_saw(agent: &Agent, file: &str) -> String {
    let folder = agent.secrets.stand_in.as_ref().expect("a stand-in worker");
    fs::read_to_string(folder.join(file)).unwrap_or_else(|error| panic!("{file}: {error}"))
}

#[then(expr = "the worker was given the job with {string}")]
fn given_the_job(agent: &mut Agent, secret: String) {
    let spec = worker_saw(agent, "spec.json");
    assert!(spec.contains(&secret), "the job on stdin: {spec}");
}

#[then(expr = "the worker was given the variable {string}")]
fn given_the_variable(agent: &mut Agent, name: String) {
    let environment = worker_saw(agent, "environment.txt");
    let prefix = format!("{name}=");
    assert!(
        environment.lines().any(|line| line.starts_with(&prefix)),
        "{name} is not in: {environment}"
    );
}

#[then(expr = "the worker was given no variable {string}")]
fn given_no_variable(agent: &mut Agent, name: String) {
    let environment = worker_saw(agent, "environment.txt");
    let prefix = format!("{name}=");
    assert!(
        !environment.lines().any(|line| line.starts_with(&prefix)),
        "{name} is in: {environment}"
    );
}

#[then(expr = "the worker was given no {string} in its arguments or its environment")]
fn given_nothing(agent: &mut Agent, secret: String) {
    for file in ["arguments.txt", "environment.txt"] {
        let seen = worker_saw(agent, file);
        assert!(!seen.contains(&secret), "{secret} is in {file}: {seen}");
    }
}
