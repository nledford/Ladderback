//! What the tests of the engine client's modules share: stand-in workers,
//! jobs, and a look at the worker log.

#[cfg(unix)]
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use api::ErrorCode;
use app::ports::{
    Engine, EngineError, EngineEvent, FakeEnd, FakeJob, Job, RepositoryAccess, RunningJob,
};
use domain::execution::{Warning, WarningKind};
use domain::secrets::{Redactor, SecretValue};
use domain::{OperationId, RepositoryId, Uuid};
use futures::stream::StreamExt;

use super::WorkerEngine;
use crate::job::{OpResult, WorkerEvent};

pub(super) fn redactor_of(secrets: &[&str]) -> Redactor {
    Redactor::new(
        &secrets
            .iter()
            .map(|secret| SecretValue::new(*secret))
            .collect::<Vec<_>>(),
    )
}

pub(super) fn failed_saying(code: ErrorCode, detail: &str) -> WorkerEvent {
    WorkerEvent::Failed {
        code,
        args: Vec::new(),
        detail: Some(detail.to_owned()),
    }
}

pub(super) fn repository(byte: &str) -> RepositoryId {
    RepositoryId::parse(&byte.repeat(32)).unwrap()
}

/// A slot's folder: straight under the root, which is Ladderback's own
/// cache folder (`~/.cache/ladderback`), not the platform's.
pub(super) fn cache_dir(root: &Path, id: &RepositoryId, slot: u32) -> PathBuf {
    root.join(id.as_str()).join(slot.to_string())
}

pub(super) fn fake_job() -> Job {
    Job::Fake(FakeJob {
        steps: 0,
        step_delay: Duration::ZERO,
        warnings: Vec::new(),
        end: FakeEnd::Succeed,
        deadline_budget: None,
    })
}

pub(super) async fn run(engine: &WorkerEngine) -> RunningJob {
    engine
        .run(OperationId::from_uuid(Uuid::from_u128(1)), fake_job())
        .await
        .unwrap()
}

/// The process id a script wrote to `file`.
#[cfg(unix)]
pub(super) async fn pid_from(file: &Path) -> i32 {
    for _ in 0..200 {
        let written = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        if let Some(pid) = written {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the script never wrote a process id to {}", file.display());
}

/// Whether `pid` is gone within five seconds. A zombie is gone: it ran
/// to its end, and only waits for a parent that may never reap it (an
/// orphan, in a container whose PID 1 doesn't reap).
#[cfg(unix)]
pub(super) async fn gone_soon(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    for _ in 0..200 {
        if kill(Pid::from_raw(pid), None).is_err() || is_zombie(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

/// The state in `/proc/<pid>/stat` follows the `)` that ends the name.
#[cfg(target_os = "linux")]
fn is_zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
    })
}

/// No `/proc` to tell: `kill` alone decides.
#[cfg(all(unix, not(target_os = "linux")))]
fn is_zombie(_pid: i32) -> bool {
    false
}

/// What the capturing logger was given for `ladderback::worker`.
static WORKER_LOG: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

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
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

pub(super) fn capture_the_worker_log() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = log::set_logger(&CapturingLogger);
        log::set_max_level(log::LevelFilter::Debug);
    });
}

/// The first record of `level` that `wanted` accepts, looked for five
/// seconds.
pub(super) async fn logged_soon(
    level: log::Level,
    wanted: impl Fn(&str) -> bool,
) -> Option<String> {
    for _ in 0..200 {
        let found = WORKER_LOG
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(logged, text)| *logged == level && wanted(text))
            .map(|(_, text)| text.clone());
        if found.is_some() {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    None
}

pub(super) fn result_line() -> String {
    serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 })).unwrap()
}

pub(super) fn identify(password: &str, secrets: &[&str]) -> Job {
    Job::Identify(RepositoryAccess {
        repository: "/backups/demo".to_owned(),
        repo_hot: None,
        password: domain::secrets::SecretValue::new(password),
        options: std::collections::BTreeMap::new(),
        options_hot: std::collections::BTreeMap::new(),
        options_cold: std::collections::BTreeMap::new(),
        secrets: secrets
            .iter()
            .map(|secret| domain::secrets::SecretValue::new(*secret))
            .collect(),
        id: None,
        entry: None,
    })
}

pub(super) fn line_of(event: &WorkerEvent) -> String {
    serde_json::to_string(event).unwrap()
}

/// The events of `job` when a stand-in worker, which started with
/// `environment` (the real one when `None`), writes `stderr` and then
/// `lines`.
#[cfg(unix)]
pub(super) async fn answered(
    job: Job,
    environment: Option<Vec<(OsString, OsString)>>,
    stderr: &str,
    lines: &[String],
) -> Vec<EngineEvent> {
    let lines: String = lines.iter().map(|line| format!(" '{line}'")).collect();
    let script = format!("cat >/dev/null; echo '{stderr}' >&2; printf '%s\\n'{lines}");
    let mut engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
    if let Some(environment) = environment {
        engine = engine.with_environment(environment);
    }
    let op = OperationId::from_uuid(Uuid::from_u128(1));
    let events = engine.run(op, job).await.unwrap().events;
    tokio::time::timeout(Duration::from_secs(10), events.collect::<Vec<_>>())
        .await
        .expect("the worker's events ended")
}

/// The environment of a daemon behind a proxy that wants credentials.
#[cfg(unix)]
pub(super) fn behind_a_proxy() -> Option<Vec<(OsString, OsString)>> {
    Some(vec![
        (
            OsString::from("PATH"),
            std::env::var_os("PATH").unwrap_or_default(),
        ),
        (
            OsString::from("HTTPS_PROXY"),
            OsString::from("http://demo:CANARY-proxy@proxy.demo:3128"),
        ),
    ])
}

pub(super) fn access() -> app::ports::RepositoryAccess {
    app::ports::RepositoryAccess {
        repository: "/srv/demo/repo".to_owned(),
        repo_hot: None,
        password: domain::secrets::SecretValue::new("demo-password"),
        options: std::collections::BTreeMap::new(),
        options_hot: std::collections::BTreeMap::new(),
        options_cold: std::collections::BTreeMap::new(),
        secrets: Vec::new(),
        id: None,
        entry: None,
    }
}

// The start and exit timeouts: a worker that hangs must not hold the
// daemon. Every wait on a worker is guarded, so that a regression fails
// at once instead of hanging the suite.

/// A job whose spec is about 900 KB: under the protocol's cap, and more
/// than any pipe holds, so a worker that never reads its stdin leaves
/// the daemon's write of the job blocked.
pub(super) fn job_larger_than_a_pipe() -> Job {
    Job::Fake(FakeJob {
        steps: 0,
        step_delay: Duration::ZERO,
        warnings: vec![Warning {
            kind: WarningKind::Other,
            item: None,
            during: "backup".to_owned(),
            detail: "x".repeat(900_000),
        }],
        end: FakeEnd::Succeed,
        deadline_budget: None,
    })
}

/// The engine's answer to `job`, which it has `guard` to give.
pub(super) async fn run_within(
    guard: Duration,
    engine: &WorkerEngine,
    job: Job,
) -> Result<RunningJob, EngineError> {
    let op = OperationId::from_uuid(Uuid::from_u128(1));
    tokio::time::timeout(guard, engine.run(op, job))
        .await
        .expect("`run` never returned: a worker that never read its job held it")
}

/// Everything `events` yields, which has `guard` to end.
pub(super) async fn collected_within(
    guard: Duration,
    events: impl futures::Stream<Item = EngineEvent>,
) -> Vec<EngineEvent> {
    tokio::time::timeout(guard, events.collect::<Vec<_>>())
        .await
        .expect("the events never ended")
}
