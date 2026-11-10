//! A secret from a command: started without a shell, in a process group of
//! its own, under a time limit and a size limit.

use std::io;
use std::pin::pin;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use domain::secrets::{
    CommandLine, MAX_SECRET_BYTES, Problem, SecretError, SecretValue, SourceName, command_failure,
    command_secret,
};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::time::timeout;

/// Of what a command writes to stderr only the first line is used, and a
/// line this long is cut short anyway.
const STDERR_KEPT: usize = 8 * 1024;

/// How long a pipe may stay open after the command has exited. A daemon the
/// command started (gpg-agent) can hold it open for good.
const PIPE_GRACE: Duration = Duration::from_secs(1);

const READ_CHUNK: usize = 8 * 1024;

/// How long a command that was killed is waited for: one in uninterruptible
/// sleep (a hung mount) may never be reaped.
const REAP_LIMIT: Duration = Duration::from_secs(5);

/// What stops a command that outlives its limit; tests replace both parts.
pub(crate) struct Stop<'a> {
    /// How long a command that was signalled is waited for.
    pub(crate) reap_limit: Duration,
    /// Signals the process group with this id.
    pub(crate) kill: &'a (dyn Fn(Pid) + Send + Sync),
}

impl Stop<'static> {
    /// SIGKILL to the group, and five seconds for the command to be reaped.
    const SIGKILL: Self = Self {
        reap_limit: REAP_LIMIT,
        kill: &kill_group,
    };
}

/// Runs `command` and gives the first line of its output.
pub(crate) async fn run(
    command: &CommandLine,
    limit: Duration,
) -> Result<SecretValue, SecretError> {
    run_with(command, limit, &Stop::SIGKILL).await
}

pub(crate) async fn run_with(
    command: &CommandLine,
    limit: Duration,
    stop: &Stop<'_>,
) -> Result<SecretValue, SecretError> {
    // The program only: the arguments may hold the secret itself.
    let source = SourceName::Command {
        program: command.program().to_owned(),
    };
    let fail = |problem| SecretError {
        source: source.clone(),
        problem,
    };
    let unreachable = |detail: String| fail(Problem::Unreachable { detail });

    let child = Command::new(command.program())
        .args(command.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0)
        .spawn()
        .map_err(|error| unreachable(error.to_string()))?;
    // From here on, whatever ends this call early (a limit, a size, the
    // caller giving up) takes the group with it.
    let mut running = Running::new(child, stop);
    let (Some(stdout), Some(stderr)) = (running.child.stdout.take(), running.child.stderr.take())
    else {
        return Err(unreachable("the command's output is not piped".to_owned()));
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let outcome = collect(
        &mut running.child,
        stdout,
        stderr,
        limit,
        &mut out,
        &mut err,
    )
    .await;
    let status = match outcome {
        Ok(Outcome::Exited(status)) => status,
        Ok(Outcome::TooLarge) => {
            running.kill_and_reap().await;
            return Err(fail(Problem::TooLarge {
                limit: MAX_SECRET_BYTES,
            }));
        }
        Err(error) => {
            running.kill_and_reap().await;
            return Err(unreachable(error.to_string()));
        }
        Ok(Outcome::TimedOut) => {
            running.kill_and_reap().await;
            return Err(fail(Problem::TimedOut {
                seconds: whole_seconds_up(limit),
            }));
        }
    };
    // It ran to its end and was reaped: what it left running is its own
    // business, and the group is not signalled any more.
    if status.success() {
        command_secret(&source, &out)
    } else {
        Err(command_failure(&source, status.code(), &out, &err))
    }
}

/// The limit in whole seconds, rounded up, and at least one.
pub(crate) fn whole_seconds_up(limit: Duration) -> u64 {
    limit
        .as_secs()
        .saturating_add(u64::from(limit.subsec_nanos() > 0))
        .max(1)
}

/// The command and its process group, which is killed when this is dropped
/// while the command is still unreaped: a future that is dropped mid-run must
/// not leave a command behind, and `kill_on_drop` reaches only the command
/// itself.
///
/// A group is named by the process id of its leader, and that id is free for
/// an unrelated process once the leader has been reaped, so the group is
/// signalled only while the leader is not, and at most once.
struct Running<'a> {
    child: Child,
    group: Option<Pid>,
    stop: &'a Stop<'a>,
    signalled: bool,
}

impl<'a> Running<'a> {
    fn new(child: Child, stop: &'a Stop<'a>) -> Self {
        let group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .map(Pid::from_raw);
        Self {
            child,
            group,
            stop,
            signalled: false,
        }
    }

    fn kill(&mut self) {
        // Tokio gives no id once the child has been waited for.
        if self.signalled || self.child.id().is_none() {
            return;
        }
        if let Some(group) = self.group {
            self.signalled = true;
            (self.stop.kill)(group);
        }
    }

    /// Signals the group and waits for the command, for as long as the stop
    /// allows: a command no signal reaches does not hold the caller up.
    async fn kill_and_reap(&mut self) {
        self.kill();
        let _ = timeout(self.stop.reap_limit, self.child.wait()).await;
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Kills the whole process group: nothing left in it is not an error.
fn kill_group(group: Pid) {
    let _ = killpg(group, Signal::SIGKILL);
}

enum Outcome {
    Exited(ExitStatus),
    /// Stdout went past the limit.
    TooLarge,
    /// The command was still running at its limit.
    TimedOut,
}

enum End {
    Eof,
    Full,
}

/// Reads stdout and stderr while the command runs, under `limit`, then gives
/// the pipes [`PIPE_GRACE`] to end. The limit is the command's: once it has
/// exited and been reaped, what is left is a pipe a daemon may hold open,
/// which the grace bounds, so a command that ended in time is never timed
/// out, and the group is signalled only while its leader is not reaped.
async fn collect(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    limit: Duration,
    out: &mut Vec<u8>,
    err: &mut Vec<u8>,
) -> io::Result<Outcome> {
    // One byte more than the limit: that is already too much.
    let mut out_read = pin!(read_capped(stdout, out, MAX_SECRET_BYTES + 1, true));
    let mut err_read = pin!(read_capped(stderr, err, STDERR_KEPT, false));
    let mut out_done = false;
    let mut err_done = false;
    let running = async {
        loop {
            tokio::select! {
                result = &mut out_read, if !out_done => match result? {
                    End::Full => return Ok(None),
                    End::Eof => out_done = true,
                },
                // A stderr that can't be read is a stderr with nothing more in it.
                _ = &mut err_read, if !err_done => err_done = true,
                status = child.wait() => return status.map(Some),
            }
        }
    };
    let status = match timeout(limit, running).await {
        Err(_elapsed) => return Ok(Outcome::TimedOut),
        Ok(Err(error)) => return Err(error),
        Ok(Ok(None)) => return Ok(Outcome::TooLarge),
        Ok(Ok(Some(status))) => status,
    };

    let drained = async {
        let (out_end, ()) = tokio::join!(
            async {
                if out_done {
                    Ok(End::Eof)
                } else {
                    (&mut out_read).await
                }
            },
            async {
                if !err_done {
                    let _ = (&mut err_read).await;
                }
            },
        );
        out_end
    };
    match timeout(PIPE_GRACE, drained).await {
        Ok(Ok(End::Full)) => Ok(Outcome::TooLarge),
        Ok(Err(error)) => Err(error),
        // Ended, or kept open by a daemon: go on with what was read.
        Ok(Ok(End::Eof)) | Err(_) => Ok(Outcome::Exited(status)),
    }
}

/// Reads `reader` into `kept`, which keeps at most `keep` bytes. With
/// `stop_when_full` it stops as soon as `keep` bytes are there; without, it
/// reads on to the end and drops the rest, so the writer is never blocked.
async fn read_capped(
    mut reader: impl AsyncRead + Unpin,
    kept: &mut Vec<u8>,
    keep: usize,
    stop_when_full: bool,
) -> io::Result<End> {
    let mut chunk = [0_u8; READ_CHUNK];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(End::Eof);
        }
        let room = keep.saturating_sub(kept.len());
        if let Some(bytes) = chunk.get(..read.min(room)) {
            kept.extend_from_slice(bytes);
        }
        if stop_when_full && kept.len() >= keep {
            return Ok(End::Full);
        }
    }
}
