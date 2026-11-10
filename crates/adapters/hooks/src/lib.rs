//! The hooks adapter: a [`HookRunner`] that spawns each hook's command as a
//! process, without a shell, in a process group of its own, with the
//! worker's scrubbed environment (components.md §4.1, security.md §10.2).
//! The spawning, timeout and kill sequence mirror the password-command
//! runner (`secrets::command`).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::ffi::OsString;
use std::pin::pin;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use app::ports::{
    HOOK_TIMEOUT, HookContext, HookOutcome, HookOutput, HookRunner, HookStatus,
    MAX_HOOK_OUTPUT_BYTES,
};
use async_trait::async_trait;
use domain::planning::HookEntry;
use domain::secrets::{Redactor, SecretValue};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

/// How long a hook's process group is given to end after SIGTERM before it
/// is killed, as the password-command runner gives one.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// How much is read from a pipe at a time.
const READ_CHUNK: usize = 8 * 1024;

/// The environment variables of the daemon a hook keeps, by exact name:
/// the worker's allow-list (components.md §4.1). A hook gets what a worker
/// gets, and nothing more of the daemon's own.
const KEPT: [&str; 24] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TZ",
    "LANG",
    "LC_ALL",
    "SSH_AUTH_SOCK",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "RCLONE_CONFIG",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

/// The environment one hook starts with: the allow-list, plus the context
/// variables of the operation it runs for, and nothing else: no secret, no
/// repository URL (security.md §10.2).
fn environment(context: &HookContext) -> Vec<(OsString, OsString)> {
    let pair = |name: &str, value: &str| (OsString::from(name), OsString::from(value));
    let mut vars = KEPT
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(*name), value)))
        .collect::<Vec<_>>();
    vars.push(pair("LADDERBACK_HOOK", context.phase.as_env()));
    vars.push(pair("LADDERBACK_PLAN", context.plan.as_str()));
    vars.push(pair("LADDERBACK_REPOSITORY", context.repository.as_str()));
    vars.push(pair(
        "LADDERBACK_OPERATION_ID",
        &context.operation.as_uuid().to_string(),
    ));
    if let Some(snapshot) = &context.snapshot {
        vars.push(pair("LADDERBACK_SNAPSHOT_ID", snapshot.as_str()));
    }
    if let Some(error) = &context.error {
        vars.push(pair("LADDERBACK_ERROR", error));
    }
    vars
}

/// Runs hook commands as processes.
#[derive(Clone, Debug, Default)]
pub struct ProcessHookRunner {
    /// How long one hook may run before it is stopped: SIGTERM to its
    /// process group, then SIGKILL after [`KILL_GRACE`].
    timeout: Duration,
    /// Masks the secrets of the run in everything a hook wrote, before it is
    /// stored or logged.
    redactor: Redactor,
}

impl ProcessHookRunner {
    /// A runner with the default timeout, redacting against nothing.
    pub fn new() -> Self {
        Self {
            timeout: HOOK_TIMEOUT,
            redactor: Redactor::new([]),
        }
    }

    /// A runner with a timeout of its own, for tests.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::new()
        }
    }

    /// Adds a secret the captured output is redacted against.
    pub fn with_secret(mut self, secret: &str) -> Self {
        let value = SecretValue::new(secret);
        self.redactor = Redactor::new([&value]);
        self
    }

    /// Runs one hook to its end, or stops it at `self.timeout`. What was
    /// read until then is kept either way.
    async fn spawn_and_wait(
        &self,
        entry: &HookEntry,
        context: &HookContext,
    ) -> (HookStatus, Captured) {
        self.spawn_and_wait_with(entry, context, &Stop::REAL).await
    }

    async fn spawn_and_wait_with(
        &self,
        entry: &HookEntry,
        context: &HookContext,
        stop: &Stop<'_>,
    ) -> (HookStatus, Captured) {
        let mut command = tokio::process::Command::new(entry.command.program());
        command
            .args(entry.command.args())
            .env_clear()
            .envs(environment(context))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            // The platform's message names the reason; the command line
            // itself may hold a secret and never appears.
            Err(error) => {
                return (
                    HookStatus::NotStarted {
                        detail: error.to_string(),
                    },
                    Captured::default(),
                );
            }
        };
        // From here on, whatever ends this call early (the timeout, the
        // caller giving up) takes the group with it.
        let group = Group::of(&child, stop);
        let Some(pipes) = Pipes::take(&mut child) else {
            group.kill_and_reap(&mut child).await;
            return (
                HookStatus::NotStarted {
                    detail: "the hook's output is not piped".to_owned(),
                },
                Captured::default(),
            );
        };
        let mut captured = Captured::default();
        let outcome = {
            let reading = pin!(captured.read(&mut child, pipes));
            tokio::time::timeout(self.timeout, reading).await
        };
        match outcome {
            // The hook ran to its end: what it left running is its business.
            Ok(Ok(status)) => {
                group.release();
                (exit_status(status), captured)
            }
            Ok(Err(error)) => {
                group.kill_and_reap(&mut child).await;
                (
                    HookStatus::NotStarted {
                        detail: error.to_string(),
                    },
                    captured,
                )
            }
            Err(_elapsed) => {
                group.stop_and_reap(&mut child).await;
                (HookStatus::TimedOut, captured)
            }
        }
    }
}

#[async_trait]
impl HookRunner for ProcessHookRunner {
    async fn run(&self, entry: &HookEntry, context: &HookContext) -> HookOutcome {
        let (status, captured) = self.spawn_and_wait(entry, context).await;
        HookOutcome {
            stdout: captured.stream(&captured.stdout, captured.stdout_truncated, &self.redactor),
            stderr: captured.stream(&captured.stderr, captured.stderr_truncated, &self.redactor),
            status,
        }
    }
}

/// What a hook wrote, as it was kept.
#[derive(Default)]
struct Captured {
    stdout: Vec<u8>,
    stdout_truncated: bool,
    stderr: Vec<u8>,
    stderr_truncated: bool,
}

impl Captured {
    /// Reads both streams to the end of the hook, keeping at most
    /// [`MAX_HOOK_OUTPUT_BYTES`] of each: the rest is read on and dropped,
    /// so a hook that talks past the cap is never blocked on it. Both
    /// streams are read together, so a hook that fills one pipe while
    /// writing the other cannot block itself.
    async fn read(&mut self, child: &mut Child, pipes: Pipes) -> std::io::Result<ExitStatus> {
        let Pipes { stdout, stderr } = pipes;
        let (out, err) = (pin!(stdout), pin!(stderr));
        let (kept_out, kept_err) = (&mut self.stdout, &mut self.stderr);
        let (cut_out, cut_err) = (&mut self.stdout_truncated, &mut self.stderr_truncated);
        let read_out = read_capped(out, kept_out, MAX_HOOK_OUTPUT_BYTES, cut_out);
        let read_err = read_capped(err, kept_err, MAX_HOOK_OUTPUT_BYTES, cut_err);
        let (out, err) = tokio::join!(read_out, read_err);
        out?;
        err?;
        child.wait().await
    }

    /// The kept, redacted text of one stream. A stream that was cut first
    /// loses as many bytes of its end as the longest secret has, less one:
    /// a secret straddling the cut can't leave its first characters
    /// behind, for the mask only finds what is whole. The engine client's
    /// log lines are cut the same way.
    fn stream(&self, text: &[u8], truncated: bool, redactor: &Redactor) -> HookOutput {
        let kept = if truncated {
            text.len()
                .saturating_sub(redactor.longest_match().saturating_sub(1))
        } else {
            text.len()
        };
        let read = String::from_utf8_lossy(text.get(..kept).unwrap_or(text));
        HookOutput {
            text: redactor.redact(&read),
            truncated,
        }
    }
}

/// The hook's output pipes.
struct Pipes {
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
}

impl Pipes {
    /// Takes both pipes, if they were piped.
    fn take(child: &mut Child) -> Option<Self> {
        let stdout = child.stdout.take()?;
        let stderr = child.stderr.take()?;
        Some(Self { stdout, stderr })
    }
}

/// Reads `reader` into `kept`, which keeps at most `keep` bytes and flags
/// `truncated` when some was dropped; the rest is read on and dropped, so
/// the writer is never blocked.
async fn read_capped(
    mut reader: impl AsyncRead + Unpin,
    kept: &mut Vec<u8>,
    keep: usize,
    truncated: &mut bool,
) -> std::io::Result<()> {
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        let room = keep.saturating_sub(kept.len());
        if read > room {
            *truncated = true;
        }
        kept.extend_from_slice(&chunk[..read.min(room)]);
    }
}

/// The status of a hook that exited.
fn exit_status(status: ExitStatus) -> HookStatus {
    if status.success() {
        HookStatus::Succeeded
    } else {
        HookStatus::Failed {
            exit_code: status.code(),
        }
    }
}

/// What stops a hook that outlives its timeout; tests replace the parts.
struct Stop<'a> {
    /// How long a hook that got SIGTERM is given to end before SIGKILL.
    grace: Duration,
    /// How long a hook that got SIGKILL is waited for.
    reap_limit: Duration,
    /// Sends a signal to the process group with this id.
    signal: &'a (dyn Fn(Pid, Signal) + Send + Sync),
}

impl Stop<'static> {
    /// The real stop: SIGTERM to the group, SIGKILL after [`KILL_GRACE`].
    const REAL: Self = Self {
        grace: KILL_GRACE,
        reap_limit: KILL_GRACE,
        signal: &signal_group,
    };
}

/// Signals the whole process group: nothing left in it is not an error.
fn signal_group(group: Pid, signal: Signal) {
    let _ = killpg(group, signal);
}

/// The process group of a hook, killed when this is dropped unless it was
/// released: a future that is dropped mid-run must not leave a command
/// behind, and `kill_on_drop` reaches only the command itself.
struct Group<'a>(Option<Pid>, &'a Stop<'a>);

impl<'a> Group<'a> {
    /// The group of `child`, which was started in a group of its own.
    fn of(child: &Child, stop: &'a Stop<'a>) -> Self {
        Self(
            child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .map(Pid::from_raw),
            stop,
        )
    }

    fn kill(&self) {
        if let Some(group) = self.0 {
            (self.1.signal)(group, Signal::SIGKILL);
        }
    }

    /// Stops a hook that outlived its timeout: SIGTERM to its process
    /// group, then SIGKILL after the grace if that was not enough.
    async fn stop_and_reap(&self, child: &mut Child) {
        if let Some(group) = self.0 {
            (self.1.signal)(group, Signal::SIGTERM);
        }
        if tokio::time::timeout(self.1.grace, child.wait())
            .await
            .is_err()
        {
            self.kill();
            let _reap_limit = self.1.reap_limit;
            let _ = child.wait().await;
        }
    }

    async fn kill_and_reap(&self, child: &mut Child) {
        self.kill();
        let _ = child.wait().await;
    }

    /// The hook ended on its own: what it left running is its business.
    fn release(mut self) {
        self.0 = None;
    }
}

impl Drop for Group<'_> {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests;
