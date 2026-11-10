//! What a job's secrets do not reach (invariant I4), through the real worker:
//! a backend that fails on a connection error quotes the URL it called, and
//! that URL holds what the settings held. Each job fails fast against
//! `127.0.0.1:9`, where nothing listens.

mod support;

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use app::ports::{Engine, EngineEvent, Job, RepositoryAccess};
use domain::execution::FailureReason;
use domain::secrets::SecretValue;
use futures::StreamExt;
use support::{engine, op};

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
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = log::set_logger(&CapturingLogger);
        log::set_max_level(log::LevelFilter::Debug);
    });
}

fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// The canary as written and as a URL parser prints a space, which is how a
/// backend quotes it.
fn forms(canary: &str) -> [String; 2] {
    [canary.to_owned(), canary.replace(' ', "%20")]
}

/// What a job is given for a repository in one place: nothing hot or cold.
/// `secrets` are what its references resolved to.
fn access(
    repository: &str,
    options: BTreeMap<String, String>,
    secrets: &[&str],
) -> RepositoryAccess {
    RepositoryAccess {
        repository: repository.to_owned(),
        repo_hot: None,
        password: SecretValue::new("demo-password"),
        options,
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
        secrets: secrets.iter().map(|s| SecretValue::new(*s)).collect(),
        id: None,
    }
}

/// What a job that failed said: the failure the daemon reports, and
/// everything it logged for the worker.
struct Said {
    detail: String,
    logged: Vec<String>,
}

impl Said {
    /// `canary` shows nowhere in it.
    fn assert_hides(&self, canary: &str) {
        for form in forms(canary) {
            assert!(
                !self.detail.contains(&form),
                "{form} leaked in: {}",
                self.detail
            );
            for line in &self.logged {
                assert!(!line.contains(&form), "{form} leaked in the log: {line}");
            }
        }
    }
}

/// A check of a log that captured nothing has looked at nothing, so it must
/// fail, not pass: capture that is broken (another logger was installed
/// first) would otherwise leave every check of the log green.
#[test]
fn a_check_of_a_log_that_captured_nothing_fails() {
    let said = Said {
        detail: "the connection to 127.0.0.1:9 was refused".to_owned(),
        logged: Vec::new(),
    };
    let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        said.assert_hides("CANARY");
    }));
    assert!(
        checked.is_err(),
        "a log with nothing in it passed the check"
    );
}

/// Runs an Identify job that can only fail.
async fn fails(access: RepositoryAccess) -> Said {
    capture_the_worker_log();
    let running = engine().run(op(), Job::Identify(access)).await.unwrap();
    let events = tokio::time::timeout(Duration::from_secs(30), running.events.collect::<Vec<_>>())
        .await
        .expect("the job hung instead of failing");
    let Some(EngineEvent::Failed(FailureReason::EngineFailed { detail })) = events.last() else {
        panic!("the job did not fail as an engine failure: {events:?}");
    };
    // What the worker's stderr holds is read beside the events.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let logged = WORKER_LOG
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    Said {
        detail: detail.clone(),
        logged,
    }
}

/// Runs an Identify job that can only fail, with an error that quotes
/// `address`, and checks that `canary` shows nowhere: not in the failure the
/// daemon reports, and not in anything it logged for the worker.
async fn fails_without_showing(access: RepositoryAccess, address: &str, canary: &str) {
    let said = fails(access).await;
    assert!(
        said.detail.contains(address),
        "the failure is not about {address}: {}",
        said.detail
    );
    said.assert_hides(canary);
    assert!(
        said.detail.contains("***"),
        "the secret was not masked in the failure: {}",
        said.detail
    );
}

/// Azure Blob on a port where nothing listens, with no credentials: the
/// connection is what fails, and its error quotes the URL it called, a SAS
/// token in it.
fn azure() -> BTreeMap<String, String> {
    options(&[
        ("endpoint", "http://127.0.0.1:9"),
        ("container", "demo"),
        ("account_name", "demo"),
        ("retry", "0"),
    ])
}

#[tokio::test]
async fn an_inline_azure_sas_token_is_masked_in_what_the_backend_says() {
    let mut settings = azure();
    settings.insert("sas_token".to_owned(), "sv=CANARY-sas-token".to_owned());
    fails_without_showing(
        access("opendal:azblob", settings, &[]),
        "127.0.0.1:9",
        "CANARY-sas-token",
    )
    .await;
}

/// S3 without credentials looks for them in the environment and on the
/// network; these options say not to, so the connection is the first thing
/// that fails.
fn s3(endpoint: &str) -> BTreeMap<String, String> {
    options(&[
        ("endpoint", endpoint),
        ("bucket", "demo"),
        ("region", "us-east-1"),
        ("retry", "0"),
        ("disable_config_load", "true"),
        ("disable_ec2_metadata", "true"),
        ("allow_anonymous", "true"),
    ])
}

#[tokio::test]
async fn the_password_in_an_s3_endpoint_url_is_masked_in_what_the_backend_says() {
    fails_without_showing(
        access(
            "opendal:s3",
            s3("http://demo:CANARY-endpoint-password@127.0.0.1:9"),
            &[],
        ),
        "127.0.0.1:9",
        "CANARY-endpoint-password",
    )
    .await;
}

#[tokio::test]
async fn a_resolved_secret_is_masked_in_an_option_that_is_not_named_like_a_secret() {
    fails_without_showing(
        access(
            "opendal:s3",
            s3("http://127.0.0.1:9/CANARY-resolved"),
            &["CANARY-resolved"],
        ),
        "127.0.0.1:9",
        "CANARY-resolved",
    )
    .await;
}

#[tokio::test]
async fn a_resolved_secret_the_url_parser_rewrites_is_masked_as_the_backend_prints_it() {
    fails_without_showing(
        access(
            "opendal:s3",
            s3("http://127.0.0.1:9/CANARY-a b:c"),
            &["CANARY-a b:c"],
        ),
        "127.0.0.1:9",
        "CANARY-a b:c",
    )
    .await;
}

/// The cold half is a folder that isn't there, so what fails is the hot
/// half: its URL has no valid port, rustic can't parse it, and its error
/// quotes the URL, userinfo and all.
#[tokio::test]
async fn the_password_in_the_url_of_the_hot_repository_is_masked_in_what_rustic_says() {
    fails_without_showing(
        RepositoryAccess {
            repo_hot: Some("rest:http://demo:CANARY-hot-password@127.0.0.1:99999/hot".to_owned()),
            ..access("/nonexistent/demo/cold", BTreeMap::new(), &[])
        },
        "127.0.0.1:99999",
        "CANARY-hot-password",
    )
    .await;
}

/// The cold half is a folder that isn't there, so the refused connection is
/// the hot half's, and its error quotes the endpoint URL it called.
#[tokio::test]
async fn the_password_in_a_hot_options_endpoint_is_masked_in_what_the_hot_backend_says() {
    fails_without_showing(
        RepositoryAccess {
            repo_hot: Some("opendal:s3".to_owned()),
            options_hot: s3("http://demo:CANARY-hot-endpoint-password@127.0.0.1:9"),
            ..access("/nonexistent/demo/cold", BTreeMap::new(), &[])
        },
        "127.0.0.1:9",
        "CANARY-hot-endpoint-password",
    )
    .await;
}

/// The hot half is a real, initialized folder, which rustic reads first. The
/// cold half is only listed, and the OpenDAL backend reports a refused
/// connection there in its log, quoting the URL it called with the SAS token
/// of the cold half's own options in it. The failure rustic reports instead
/// says that the keys of the halves differ, and quotes nothing.
#[tokio::test]
async fn a_credential_option_of_the_cold_half_is_masked_in_what_the_cold_backend_says() {
    let scratch = support::Scratch::new();
    let initialized = scratch.hot_cold();
    initialized.init();
    let mut cold = azure();
    cold.insert("container".to_owned(), "demo-cold".to_owned());
    cold.insert(
        "sas_token".to_owned(),
        "sv=CANARY-cold-sas-token".to_owned(),
    );
    let said = fails(RepositoryAccess {
        repo_hot: initialized.repo_hot.clone(),
        options_cold: cold,
        ..access("opendal:azblob", BTreeMap::new(), &[])
    })
    .await;
    assert!(
        said.logged
            .iter()
            .any(|line| line.contains("127.0.0.1:9/demo-cold") && line.contains("***")),
        "what the cold backend said is not in the log, or its token is not masked: {:#?}",
        said.logged
    );
    said.assert_hides("CANARY-cold-sas-token");
}
