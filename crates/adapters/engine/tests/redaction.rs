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

/// Everything the daemon logged for `steading::worker`.
static WORKER_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct CapturingLogger;

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "steading::worker"
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

/// Runs an Identify job that can only fail, and checks that `canary` shows
/// nowhere: not in the failure the daemon reports, and not in anything it
/// logged for the worker.
async fn fails_without_showing(
    repository: &str,
    options: BTreeMap<String, String>,
    secrets: &[&str],
    canary: &str,
) {
    capture_the_worker_log();
    let job = Job::Identify(RepositoryAccess {
        repository: repository.to_owned(),
        password: SecretValue::new("demo-password"),
        options,
        secrets: secrets.iter().map(|s| SecretValue::new(*s)).collect(),
        id: None,
    });
    let running = engine().run(op(), job).await.unwrap();
    let events = tokio::time::timeout(Duration::from_secs(30), running.events.collect::<Vec<_>>())
        .await
        .expect("the job hung instead of failing on the refused connection");
    let Some(EngineEvent::Failed(FailureReason::EngineFailed { detail })) = events.last() else {
        panic!("the job did not fail as an engine failure: {events:?}");
    };
    assert!(
        detail.contains("127.0.0.1:9"),
        "the failure is not the refused connection: {detail}"
    );
    for form in forms(canary) {
        assert!(!detail.contains(&form), "{form} leaked in: {detail}");
    }
    assert!(
        detail.contains("***"),
        "the secret was not masked in the failure: {detail}"
    );
    // What the worker's stderr holds is read beside the events.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let logged = WORKER_LOG.lock().unwrap_or_else(PoisonError::into_inner);
    for line in logged.iter() {
        for form in forms(canary) {
            assert!(!line.contains(&form), "{form} leaked in the log: {line}");
        }
    }
}

#[tokio::test]
async fn an_inline_azure_sas_token_is_masked_in_what_the_backend_says() {
    fails_without_showing(
        "opendal:azblob",
        options(&[
            ("endpoint", "http://127.0.0.1:9"),
            ("container", "demo"),
            ("account_name", "demo"),
            ("sas_token", "sv=CANARY-sas-token"),
            ("retry", "0"),
        ]),
        &[],
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
        "opendal:s3",
        s3("http://demo:CANARY-endpoint-password@127.0.0.1:9"),
        &[],
        "CANARY-endpoint-password",
    )
    .await;
}

#[tokio::test]
async fn a_resolved_secret_is_masked_in_an_option_that_is_not_named_like_a_secret() {
    fails_without_showing(
        "opendal:s3",
        s3("http://127.0.0.1:9/CANARY-resolved"),
        &["CANARY-resolved"],
        "CANARY-resolved",
    )
    .await;
}

#[tokio::test]
async fn a_resolved_secret_the_url_parser_rewrites_is_masked_as_the_backend_prints_it() {
    fails_without_showing(
        "opendal:s3",
        s3("http://127.0.0.1:9/CANARY-a b:c"),
        &["CANARY-a b:c"],
        "CANARY-a b:c",
    )
    .await;
}
