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

/// How long a command that outlived its limit has to end after SIGTERM before
/// it is killed.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// What stops a command that outlives its limit; tests replace the parts.
pub(crate) struct Stop<'a> {
    /// How long a command that got SIGTERM is given to end before SIGKILL.
    pub(crate) grace: Duration,
    /// How long a command that got SIGKILL is waited for.
    pub(crate) reap_limit: Duration,
    /// Sends a signal to the process group with this id.
    pub(crate) signal: &'a (dyn Fn(Pid, Signal) + Send + Sync),
}

impl Stop<'static> {
    /// The real stop: SIGTERM to the group, SIGKILL after [`KILL_GRACE`], and
    /// [`REAP_LIMIT`] for the command to be reaped.
    const REAL: Self = Self {
        grace: KILL_GRACE,
        reap_limit: REAP_LIMIT,
        signal: &signal_group,
    };
}

/// Runs `command` and gives the first line of its output.
pub(crate) async fn run(
    command: &CommandLine,
    limit: Duration,
) -> Result<SecretValue, SecretError> {
    run_with(command, limit, &Stop::REAL).await
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
    // Held until the command is reaped: a handler that writes to them when
    // it gets SIGTERM must find them open, or the SIGPIPE ends it before it
    // is done, and while it is being stopped they are read and dropped, so
    // it is not blocked on a pipe nobody reads either.
    let (Some(mut stdout), Some(mut stderr)) =
        (running.child.stdout.take(), running.child.stderr.take())
    else {
        return Err(unreachable("the command's output is not piped".to_owned()));
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let outcome = collect(
        &mut running.child,
        &mut stdout,
        &mut stderr,
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
            running.terminate_and_reap(&mut stdout, &mut stderr).await;
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
/// A group is named by the process id of its leader, and a leader that has
/// been reaped no longer holds that id: once the group has no live member
/// either, the id is free for an unrelated process, and a signal to it would
/// reach that one. What is left of the group after the leader is gone can't
/// be told from here, so the group is signalled only while the leader is
/// not reaped, and each signal at most once.
struct Running<'a> {
    child: Child,
    group: Option<Pid>,
    stop: &'a Stop<'a>,
    terminated: bool,
    killed: bool,
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
            terminated: false,
            killed: false,
        }
    }

    fn send(&mut self, signal: Signal) {
        let sent = match signal {
            Signal::SIGKILL => &mut self.killed,
            _ => &mut self.terminated,
        };
        // Tokio gives no id once the child has been waited for.
        if *sent || self.child.id().is_none() {
            return;
        }
        if let Some(group) = self.group {
            *sent = true;
            (self.stop.signal)(group, signal);
        }
    }

    /// Signals the group and waits for the command, for as long as the stop
    /// allows: a command no signal reaches does not hold the caller up.
    async fn kill_and_reap(&mut self) {
        self.send(Signal::SIGKILL);
        let _ = timeout(self.stop.reap_limit, self.child.wait()).await;
    }

    /// SIGTERM to the group, and SIGKILL once the grace has passed with the
    /// command still running. Whatever the command writes meanwhile is read
    /// and dropped. A command that ended on the SIGTERM is reaped
    /// and nothing more is sent: what it started and left running, if it
    /// ignored the SIGTERM, is not signalled, as the group may have no live
    /// member by now. The whole call takes at most the grace and the reap
    /// limit.
    async fn terminate_and_reap(&mut self, stdout: &mut ChildStdout, stderr: &mut ChildStderr) {
        self.send(Signal::SIGTERM);
        let ended = wait_draining(&mut self.child, stdout, stderr);
        if timeout(self.stop.grace, ended).await.is_err() {
            self.kill_and_reap().await;
        }
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.send(Signal::SIGKILL);
    }
}

/// Signals the whole process group: nothing left in it is not an error.
fn signal_group(group: Pid, signal: Signal) {
    let _ = killpg(group, signal);
}

/// Waits for the command while reading what it writes and dropping it. A
/// command that is being stopped may say more on its way out than a pipe
/// holds, and one that is blocked on a pipe nobody reads can't finish its
/// handler.
async fn wait_draining(
    child: &mut Child,
    stdout: &mut ChildStdout,
    stderr: &mut ChildStderr,
) -> io::Result<ExitStatus> {
    let mut drain = pin!(async {
        tokio::join!(discard(stdout), discard(stderr));
    });
    let mut drained = false;
    let mut exit = pin!(child.wait());
    loop {
        tokio::select! {
            status = &mut exit => return status,
            () = &mut drain, if !drained => drained = true,
        }
    }
}

/// Reads `reader` to its end, or its first error, and keeps nothing.
async fn discard(mut reader: impl AsyncRead + Unpin) {
    let mut chunk = [0_u8; READ_CHUNK];
    while matches!(reader.read(&mut chunk).await, Ok(read) if read > 0) {}
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
    stdout: &mut ChildStdout,
    stderr: &mut ChildStderr,
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
