//! A secret from a command: started without a shell, in a process group of
//! its own, under a time limit and a size limit.

use std::io;
use std::pin::pin;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use domain::secrets::{
    CommandLine, MAX_SECRET_BYTES, Problem, Redactor, SecretError, SecretValue, SourceName, decode,
    first_line,
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

/// Runs `command` and gives the first line of its output.
pub(crate) async fn run(
    command: &CommandLine,
    limit: Duration,
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

    let mut child = Command::new(command.program())
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
    let group = Group(
        child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .map(Pid::from_raw),
    );
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(unreachable("the command's output is not piped".to_owned()));
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let outcome = timeout(
        limit,
        collect(&mut child, stdout, stderr, &mut out, &mut err),
    )
    .await;
    let status = match outcome {
        Ok(Ok(Outcome::Exited(status))) => status,
        Ok(Ok(Outcome::TooLarge)) => {
            group.kill_and_reap(&mut child).await;
            return Err(fail(Problem::TooLarge {
                limit: MAX_SECRET_BYTES,
            }));
        }
        Ok(Err(error)) => {
            group.kill_and_reap(&mut child).await;
            return Err(unreachable(error.to_string()));
        }
        Err(_elapsed) => {
            group.kill_and_reap(&mut child).await;
            return Err(fail(Problem::TimedOut {
                seconds: whole_seconds_up(limit),
            }));
        }
    };
    // It ran to its end: what it left running is its own business.
    group.release();

    if status.success() {
        decode(&source, &out, first_line)
    } else {
        // A helper that rejects a value may quote it on stderr: what it
        // printed, and its first line, which is what it would have given as
        // its secret, are masked there.
        let output = String::from_utf8_lossy(&out);
        let printed = SecretValue::new(output.as_ref());
        let first = SecretValue::new(first_line(&output));
        let said = Redactor::new([&printed, &first]).redact(&String::from_utf8_lossy(&err));
        Err(fail(Problem::command_failed(
            status.code(),
            said.as_bytes(),
        )))
    }
}

/// The limit in whole seconds, rounded up, and at least one.
pub(crate) fn whole_seconds_up(limit: Duration) -> u64 {
    limit
        .as_secs()
        .saturating_add(u64::from(limit.subsec_nanos() > 0))
        .max(1)
}

/// The process group of a command, killed when this is dropped unless it was
/// released: a future that is dropped mid-run must not leave a command
/// behind, and `kill_on_drop` reaches only the command itself.
struct Group(Option<Pid>);

impl Group {
    fn kill(&self) {
        if let Some(group) = self.0 {
            // Nothing left in the group is not an error.
            let _ = killpg(group, Signal::SIGKILL);
        }
    }

    async fn kill_and_reap(&self, child: &mut Child) {
        self.kill();
        let _ = child.wait().await;
    }

    fn release(mut self) {
        self.0 = None;
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

enum Outcome {
    Exited(ExitStatus),
    /// Stdout went past the limit.
    TooLarge,
}

enum End {
    Eof,
    Full,
}

/// Reads stdout and stderr while the command runs, then gives the pipes
/// [`PIPE_GRACE`] to end. What was read is in `out` and `err` even when this
/// is cancelled.
async fn collect(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    out: &mut Vec<u8>,
    err: &mut Vec<u8>,
) -> io::Result<Outcome> {
    // One byte more than the limit: that is already too much.
    let mut out_read = pin!(read_capped(stdout, out, MAX_SECRET_BYTES + 1, true));
    let mut err_read = pin!(read_capped(stderr, err, STDERR_KEPT, false));
    let mut out_done = false;
    let mut err_done = false;
    let status = loop {
        tokio::select! {
            result = &mut out_read, if !out_done => match result? {
                End::Full => return Ok(Outcome::TooLarge),
                End::Eof => out_done = true,
            },
            // A stderr that can't be read is a stderr with nothing more in it.
            _ = &mut err_read, if !err_done => err_done = true,
            status = child.wait() => break status?,
        }
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
