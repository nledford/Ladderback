//! Steps for `features/secrets/core_dumps.feature`: the core-dump limit and
//! the dumpable flag of the real daemon and of a real worker, read from
//! `/proc` as another process of the same user sees them.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{ChildStdout, Command, Stdio};

use cucumber::{given, then};
use domain::{OperationId, Uuid};
use engine::job::{FakeEndSpec, FakeJobSpec, JobKind, JobSpec, WorkerEvent};
use testkit::children::Owned;

use super::{Agent, LADDERBACK, blocking};

/// The worker the scenario started, and the events it writes, kept open so
/// that it isn't stopped by a broken pipe.
#[derive(Debug, Default)]
pub struct CoreDumpsE2e {
    worker: Option<(Owned, BufReader<ChildStdout>)>,
}

/// How long the worker's fake job waits between its steps: longer than any
/// scenario runs, so it is still running its job when it is looked at, and
/// ended with the scenario.
const STEP_DELAY_MS: u64 = 600_000;

#[given("a worker is running its job")]
async fn a_worker_running_its_job(agent: &mut Agent) {
    let spec = agent.scratch().join("core-dumps-job.json");
    let worker = blocking(move || start_worker(&spec)).await;
    agent.core_dumps.worker = Some(worker);
}

/// Starts `ladderback worker` as the daemon does, on a fake job read from
/// `spec`, and returns once the worker has said the job started: by then it
/// has done everything it does before it reads a job.
fn start_worker(spec: &Path) -> (Owned, BufReader<ChildStdout>) {
    let job = JobSpec {
        op_id: OperationId::from_uuid(Uuid::from_u128(0xc0de_d0e5)),
        cache_dir: None,
        watchdog_interval_ms: 1_000,
        job: JobKind::Fake(FakeJobSpec {
            steps: 2,
            step_delay_ms: STEP_DELAY_MS,
            warnings: Vec::new(),
            end: FakeEndSpec::Succeed,
            deadline_budget: None,
        }),
    };
    let json = serde_json::to_vec(&job).expect("the job as JSON");
    std::fs::write(spec, json).expect("the job's file");
    let mut command = Command::new(LADDERBACK);
    command
        .arg("worker")
        .stdin(std::fs::File::open(spec).expect("the job's file"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let worker = Owned::spawn(&mut command).expect("the worker starts");
    let mut events = BufReader::new(worker.take_stdout().expect("the worker's events"));
    // A log line may come first.
    loop {
        let mut line = String::new();
        let read = events.read_line(&mut line).expect("the worker's events");
        assert_ne!(read, 0, "the worker ended before it started its job");
        if let Ok(WorkerEvent::Started { .. }) = serde_json::from_str(&line) {
            return (worker, events);
        }
    }
}

fn daemon(agent: &Agent) -> u32 {
    agent
        .web
        .server
        .as_ref()
        .expect("Ladderback is serving")
        .pid()
}

fn worker(agent: &Agent) -> u32 {
    let (worker, _) = agent
        .core_dumps
        .worker
        .as_ref()
        .expect("a worker is running");
    worker.id()
}

#[then("the daemon's core-dump limit is 0")]
fn daemon_core_limit(agent: &mut Agent) {
    core_limit_is_0(daemon(agent));
}

#[then("the worker's core-dump limit is 0")]
fn worker_core_limit(agent: &mut Agent) {
    core_limit_is_0(worker(agent));
}

#[then("other processes can't read the daemon's memory")]
fn daemon_memory(agent: &mut Agent) {
    memory_unreadable(daemon(agent));
}

#[then("other processes can't read the worker's memory")]
fn worker_memory(agent: &mut Agent) {
    memory_unreadable(worker(agent));
}

/// Both the soft and the hard limit are 0, so nothing the process starts can
/// raise it again either.
fn core_limit_is_0(pid: u32) {
    if !cfg!(target_os = "linux") {
        testkit::skip!(
            "only Linux shows another process's limits (`/proc/<pid>/limits`); the platform adapter's tests check the limit of their own process"
        );
    }
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).expect("its limits");
    let line = limits
        .lines()
        .find_map(|line| line.strip_prefix("Max core file size"))
        .expect("a core-file line");
    let values: Vec<&str> = line.split_whitespace().take(2).collect();
    assert_eq!(
        values,
        ["0", "0"],
        "the soft and hard core-dump limits: {line}"
    );
}

/// The process runs as this test's user, and yet its memory belongs to root
/// and can't be opened: it is not dumpable, which also refuses `ptrace`. The
/// owner is what proves the flag: Yama's `ptrace_scope` 2 or 3 refuses the
/// open for every process, dumpable or not (proc(5)).
fn memory_unreadable(pid: u32) {
    if !cfg!(target_os = "linux") {
        testkit::skip!(
            "macOS has no dumpable flag and no `/proc`; decision 98 leaves its debugger refusal out"
        );
    }
    let ours = uids("self");
    if ours.split_whitespace().nth(1) == Some("0") {
        testkit::skip!("root reads any process's memory, dumpable or not");
    }
    assert_eq!(uids(&pid.to_string()), ours, "it runs as this test's user");
    let memory = std::fs::metadata(format!("/proc/{pid}/mem")).expect("its memory file");
    assert_eq!(
        std::os::unix::fs::MetadataExt::uid(&memory),
        0,
        "a process that isn't dumpable has its memory file owned by root"
    );
    match std::fs::File::open(format!("/proc/{pid}/mem")) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        other => panic!("its memory was opened, or failed otherwise: {other:?}"),
    }
}

/// The `Uid:` line of `/proc/<pid>/status`: real, effective, saved and
/// file-system user IDs.
fn uids(pid: &str) -> String {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("its status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .expect("a Uid line")
        .to_owned()
}
