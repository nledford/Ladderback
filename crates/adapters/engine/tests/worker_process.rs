//! The worker as a process of its own: what it does when the daemon that
//! started it goes away.

#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use domain::{OperationId, Uuid};
use engine::job::{FakeEndSpec, FakeJobSpec, JobKind, JobSpec};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Whether the kernel tells a process when its parent dies. Only Linux's
/// `PR_SET_PDEATHSIG` does; elsewhere a worker whose daemon is gone ends at
/// its next event, when the pipe it writes to is broken.
fn parent_death_signal_exists() -> bool {
    cfg!(target_os = "linux")
}

/// A fake job that runs for half a minute unless something stops it.
fn long_job(spec_file: &Path) {
    let spec = JobSpec {
        op_id: OperationId::from_uuid(Uuid::from_u128(1)),
        cache_dir: None,
        watchdog_interval_ms: 50,
        job: JobKind::Fake(FakeJobSpec {
            steps: 600,
            step_delay_ms: 50,
            warnings: Vec::new(),
            end: FakeEndSpec::Succeed,
            deadline_budget: None,
        }),
    };
    std::fs::write(spec_file, serde_json::to_vec(&spec).unwrap()).unwrap();
}

/// A zombie isn't running: it ended, and waits for a reaper that may never
/// come (an orphan, in a container whose PID 1 doesn't reap).
fn is_running(pid: Pid) -> bool {
    kill(pid, None).is_ok() && !is_zombie(pid)
}

/// The state in `/proc/<pid>/stat` follows the `)` that ends the name.
fn is_zombie(pid: Pid) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
    })
}

/// A worker is killed with its parent, even when the pipe it writes to stays
/// open and writable.
#[test]
fn a_worker_whose_parent_dies_ends_instead_of_running_on() {
    if !parent_death_signal_exists() {
        testkit::skip!("the parent-death signal is a Linux feature");
    }
    let dir = tempfile::tempdir().unwrap();
    let spec_file = dir.path().join("spec.json");
    let events_file = dir.path().join("events");
    long_job(&spec_file);

    // The parent: a shell that starts the worker, waits until its first
    // event shows it is running, and exits. The events go to a file, so
    // nothing but the parent's death can stop the worker. The worker holds
    // none of the shell's pipes, or `output` would wait for it.
    let script = r#"
        "$0" < "$1" > "$2" 2>/dev/null &
        pid=$!
        n=0
        until grep -q started "$2" 2>/dev/null || [ "$n" -ge 200 ]; do
            sleep 0.05
            n=$((n + 1))
        done
        echo "$pid"
    "#;
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_engine-worker"))
        .arg(&spec_file)
        .arg(&events_file)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let pid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i32>()
        .map(Pid::from_raw)
        .unwrap_or_else(|_| panic!("no worker started: {output:?}"));

    let deadline = Instant::now() + Duration::from_secs(10);
    while is_running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let survived = is_running(pid);
    if survived {
        // Don't leave it running for the rest of its half minute.
        let _ = kill(pid, Signal::SIGKILL);
    }
    assert!(!survived, "the worker outlived its parent");
}
