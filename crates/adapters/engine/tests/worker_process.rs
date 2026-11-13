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

/// A fake job of `steps` steps 50 ms apart that ends well unless something
/// stops it.
fn fake_job(spec_file: &Path, steps: u32) {
    let spec = JobSpec {
        op_id: OperationId::from_uuid(Uuid::from_u128(1)),
        cache_dir: None,
        watchdog_interval_ms: 50,
        job: JobKind::Fake(FakeJobSpec {
            steps,
            step_delay_ms: 50,
            warnings: Vec::new(),
            end: FakeEndSpec::Succeed,
            deadline_budget: None,
        }),
    };
    std::fs::write(spec_file, serde_json::to_vec(&spec).unwrap()).unwrap();
}

/// A fake job that runs for half a minute unless something stops it.
fn long_job(spec_file: &Path) {
    fake_job(spec_file, 600);
}

/// Waits until the worker writing `events_file` has said that its job
/// started, which it says only once it has asked to be told of its parent's
/// death.
fn until_started(events_file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::fs::read_to_string(events_file).is_ok_and(|events| events.contains("started")) {
        assert!(
            Instant::now() < deadline,
            "the worker never said its job started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
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

/// The kernel tells a worker when the *thread* that started it ends, not
/// only its process: the daemon starts workers from tokio's blocking pool,
/// whose threads end after 10 s without work while the daemon lives on. A
/// backup that outlived that thread was stopped mid-run and failed with the
/// exit code of a SIGTERM, 143 (#448). Only the daemon's death ends a worker.
#[test]
fn a_worker_runs_on_when_the_thread_that_started_it_ends() {
    if !parent_death_signal_exists() {
        testkit::skip!("the parent-death signal is a Linux feature");
    }
    let dir = tempfile::tempdir().unwrap();
    let spec_file = dir.path().join("spec.json");
    let events_file = dir.path().join("events");
    // Two seconds of work: long enough to still be running when the thread
    // that started it has ended.
    fake_job(&spec_file, 40);

    // A thread of this process starts the worker, waits until it runs, and
    // ends; the process, the worker's parent, lives on.
    let starter = std::thread::spawn({
        let events_file = events_file.clone();
        move || {
            let child = Command::new(env!("CARGO_BIN_EXE_engine-worker"))
                .stdin(std::fs::File::open(&spec_file).unwrap())
                .stdout(std::fs::File::create(&events_file).unwrap())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            until_started(&events_file);
            child
        }
    });
    let mut child = starter.join().unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("the worker never ended its two-second job");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        status.code(),
        Some(0),
        "the worker ended ({status}) when the thread that started it ended, while its parent lived on"
    );
}

/// Set in the copy of this test binary that plays the daemon in
/// [`a_worker_ends_with_its_parent_after_the_thread_that_started_it_ended`]:
/// the folder that holds the job, the events and the copy's report.
const PLAY_THE_DAEMON: &str = "ENGINE_WORKER_PROCESS_PLAY_THE_DAEMON";

/// A worker that ran on when the thread that started it ended still ends
/// with the process that started it: the daemon's death, not the first
/// thread's end, is what a dead daemon's workers are stopped by.
#[test]
fn a_worker_ends_with_its_parent_after_the_thread_that_started_it_ended() {
    if !parent_death_signal_exists() {
        testkit::skip!("the parent-death signal is a Linux feature");
    }
    if let Some(dir) = std::env::var_os(PLAY_THE_DAEMON) {
        play_the_daemon(Path::new(&dir));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    long_job(&dir.path().join("spec.json"));

    // The parent: a copy of this test binary that runs only this test. A
    // thread of it starts the worker and ends; the copy reports whether the
    // worker ran on, and exits.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "a_worker_ends_with_its_parent_after_the_thread_that_started_it_ended",
            "--nocapture",
        ])
        .env(PLAY_THE_DAEMON, dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "the parent failed: {output:?}");
    let report = std::fs::read_to_string(dir.path().join("report"))
        .unwrap_or_else(|_| panic!("the parent wrote no report: {output:?}"));
    let pid = match report.strip_prefix("running ") {
        Some(pid) => Pid::from_raw(pid.parse().unwrap()),
        None => panic!(
            "the worker ended ({report}) when the thread that started it ended, while its parent lived on"
        ),
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    while is_running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let survived = is_running(pid);
    if survived {
        // Don't leave it running for the rest of its half minute.
        let _ = kill(pid, Signal::SIGKILL);
    }
    assert!(
        !survived,
        "the worker outlived its parent once the thread that started it had ended"
    );
}

/// The parent in
/// [`a_worker_ends_with_its_parent_after_the_thread_that_started_it_ended`]:
/// a thread starts the worker on the job in `dir` and ends once the job has
/// started. Two seconds later the report says `running <pid>`, or the
/// worker's exit status.
fn play_the_daemon(dir: &Path) {
    let starter = std::thread::spawn({
        let dir = dir.to_owned();
        move || {
            let events_file = dir.join("events");
            let child = Command::new(env!("CARGO_BIN_EXE_engine-worker"))
                .stdin(std::fs::File::open(dir.join("spec.json")).unwrap())
                .stdout(std::fs::File::create(&events_file).unwrap())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            until_started(&events_file);
            child
        }
    });
    let mut child = starter.join().unwrap();

    // The kernel signals the worker as the thread ends, which can come a
    // moment after `join` returns; give the worker time to act on it.
    let deadline = Instant::now() + Duration::from_secs(2);
    let report = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.to_string();
        }
        if Instant::now() >= deadline {
            break format!("running {}", child.id());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    std::fs::write(dir.join("report"), report).unwrap();
}
