//! Processes a test run starts and owns: a daemon above all, with the
//! workers it starts. Each is ended when its [`Owned`] is dropped (a scenario
//! that passes, fails or panics drops its world), and every one still running
//! is ended by [`end_all`], which the scenario watch calls before it stops a
//! run at its limit (#499): `std::process::exit` runs no destructor, and a
//! daemon left running holds the runner's output pipe, so whatever waits for
//! that pipe to close (`just red-first`) waits for its own timeout.
//!
//! Ending one stops the process, so that it starts nothing more, kills the
//! process group each of its children leads (a daemon's workers each lead
//! one, with whatever they started), then kills and reaps the process.

use std::collections::BTreeMap;
use std::io;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// How often [`Owned::wait`] looks whether the process has ended.
const POLL: Duration = Duration::from_millis(10);

/// How long [`end_all`] waits for the killed processes to end before the
/// run exits anyway: a kill takes milliseconds, and one stuck where SIGKILL
/// can't reach it (a hung mount) must not hold the exit the watch exists
/// for. Its pipes are closed with it either way.
const REAP: Duration = Duration::from_secs(1);

/// Every process the run owns and has not ended yet.
static OWNED: Mutex<Owning> = Mutex::new(Owning {
    next: 0,
    children: BTreeMap::new(),
});

/// The processes the run owns, each by its owner's number: a number, not the
/// process ID, which the system may give another process once this one has
/// been reaped.
pub(crate) struct Owning {
    next: u64,
    children: BTreeMap<u64, Child>,
}

fn owned() -> MutexGuard<'static, Owning> {
    OWNED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A child process the run owns, ended with its children's process groups
/// when dropped.
#[derive(Debug)]
pub struct Owned {
    number: u64,
    pid: u32,
}

impl Owned {
    /// Starts `command` through the spawn gate. Call it outside
    /// `spawn_gate::hold`: it takes the gate itself.
    ///
    /// # Errors
    ///
    /// The process could not start.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        // Held across the start: when the scenario watch ends every process
        // the run owns, it sees this one, or this one never starts.
        let mut owning = owned();
        let child = spawn_gate::hold(|| command.spawn())?;
        let number = owning.next;
        owning.next = number.wrapping_add(1);
        let pid = child.id();
        owning.children.insert(number, child);
        Ok(Self { number, pid })
    }

    /// The process's ID.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Its stdout, if it was piped and not taken yet.
    #[must_use]
    pub fn take_stdout(&self) -> Option<ChildStdout> {
        self.with(|child| child.stdout.take())
    }

    /// Its stderr, if it was piped and not taken yet.
    #[must_use]
    pub fn take_stderr(&self) -> Option<ChildStderr> {
        self.with(|child| child.stderr.take())
    }

    /// How it ended, if it has.
    ///
    /// # Errors
    ///
    /// Its status could not be read.
    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        self.with(Child::try_wait)
    }

    /// Waits until it ends by itself.
    ///
    /// # Errors
    ///
    /// Its status could not be read.
    pub fn wait(&self) -> io::Result<ExitStatus> {
        // Looked at now and then, not waited for: the list stays free for
        // the scenario watch meanwhile.
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            thread::sleep(POLL);
        }
    }

    /// Runs `with` on the process.
    fn with<T>(&self, with: impl FnOnce(&mut Child) -> T) -> T {
        let mut owning = owned();
        let child = owning
            .children
            .get_mut(&self.number)
            .expect("a process is listed until its owner is dropped");
        with(child)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let mut owning = owned();
        let Some(mut child) = owning.children.remove(&self.number) else {
            return;
        };
        // Signalled while it is listed: the scenario watch, ending the run,
        // either sees it here or has ended it already.
        end(&mut child);
        drop(owning);
        let _ = child.wait();
    }
}

/// Ends and reaps every process the run owns, waiting up to [`REAP`] for
/// them, and returns holding the list, so that nothing starts another
/// before the process exits.
pub(crate) fn end_all() -> MutexGuard<'static, Owning> {
    let mut owning = owned();
    for child in owning.children.values_mut() {
        end(child);
    }
    let by = Instant::now() + REAP;
    for child in owning.children.values_mut() {
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < by {
            thread::sleep(POLL);
        }
    }
    owning
}

/// Kills `child`, unless it has ended, and the process group each of its
/// children leads first, without reaping it.
fn end(child: &mut Child) {
    // Reaped, its ID may be another process's by now.
    if !matches!(child.try_wait(), Ok(None)) {
        return;
    }
    #[cfg(unix)]
    end_children(child.id());
    let _ = child.kill();
}

/// Stops the process `parent`, so that it starts nothing more, and kills the
/// process group each of its children leads, or the child alone where it
/// leads none.
#[cfg(unix)]
fn end_children(parent: u32) {
    use nix::sys::signal::{Signal, kill, killpg};
    use nix::unistd::Pid;

    let Ok(raw) = i32::try_from(parent) else {
        return;
    };
    let _ = kill(Pid::from_raw(raw), Signal::SIGSTOP);
    match children_of(raw) {
        Ok(children) => {
            for child in children.into_iter().map(Pid::from_raw) {
                // A group with the child's ID is the child's: a process
                // group's ID is its leader's, never reused while it lives.
                if killpg(child, Signal::SIGKILL).is_err() {
                    let _ = kill(child, Signal::SIGKILL);
                }
            }
        }
        Err(error) => eprintln!(
            "testkit: the children of process {parent} could not be listed, so the process \
             groups they lead are left running: {error}"
        ),
    }
}

/// The IDs of the processes whose parent is `parent`, from `/proc`.
#[cfg(target_os = "linux")]
fn children_of(parent: i32) -> io::Result<Vec<i32>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        // A process that ended since the listing has no `stat` any more.
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // `pid (name) state ppid …`, where the name may hold spaces and
        // parentheses of its own.
        let ppid = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse::<i32>().ok());
        if ppid == Some(parent) {
            children.push(pid);
        }
    }
    Ok(children)
}

/// The IDs of the processes whose parent is `parent`, from `pgrep`.
#[cfg(all(unix, not(target_os = "linux")))]
fn children_of(parent: i32) -> io::Result<Vec<i32>> {
    use std::process::Stdio;

    let mut command = Command::new("/usr/bin/pgrep");
    command
        .args(["-P", &parent.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = spawn_gate::hold(|| command.spawn())?.wait_with_output()?;
    match output.status.code() {
        // 1: no process matched.
        Some(0 | 1) => Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect()),
        _ => Err(io::Error::other(format!(
            "pgrep {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}
