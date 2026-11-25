//! The worker's standard input and output belong to the daemon alone.
//!
//! A program the worker starts inherits its file descriptors. rustic's
//! rclone backend starts `rclone`, which writes to stderr only, but still
//! holds stdout: while it runs, the daemon's reader never sees the pipe close,
//! and anything such a program printed there would corrupt the stream. So the
//! worker keeps the event pipe under a descriptor of its own, marked
//! close-on-exec, and points descriptor 1 at `/dev/null`. A prune's stdin
//! stays open for the daemon's answer (§16, decision 161), and a program
//! that read it could take the answer or wait on it, so stdin is kept the
//! same way, with descriptor 0 at `/dev/null`.

use std::fs::File;
use std::io;
#[cfg(unix)]
use std::os::fd::AsFd;

/// The process's stdout as a file for the event stream. Afterwards
/// descriptor 1 is `/dev/null`: whatever else writes to stdout is dropped,
/// and no program the worker starts inherits the pipe.
#[cfg(unix)]
pub(crate) fn detach_stdout() -> io::Result<File> {
    let events = File::from(io::stdout().as_fd().try_clone_to_owned()?);
    let null = File::options().write(true).open("/dev/null")?;
    nix::unistd::dup2_stdout(&null)?;
    Ok(events)
}

/// The process's stdin as a file for the job spec and a prune's answer,
/// marked close-on-exec. Afterwards descriptor 0 is `/dev/null`: a program
/// the worker starts reads the end of its input at once, never the daemon's
/// pipe.
#[cfg(unix)]
pub(crate) fn detach_stdin() -> io::Result<File> {
    let answers = File::from(io::stdin().as_fd().try_clone_to_owned()?);
    let null = File::open("/dev/null")?;
    nix::unistd::dup2_stdin(&null)?;
    Ok(answers)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;

    /// Set in the process the test starts, which then runs [`detached`]
    /// instead of starting one more: redirecting descriptor 1 inside the
    /// test run itself would swallow the harness's own output.
    const CHILD: &str = "LADDERBACK_TEST_DETACH_STDOUT";

    const TEST: &str = "worker::stdio::tests::what_is_written_to_stdout_after_detaching_never_reaches_the_event_pipe";

    /// The body of the test, in the process whose stdout is the pipe.
    fn detached() {
        let mut events = detach_stdout().unwrap();
        events.write_all(b"event line\n").unwrap();
        let mut stdout = io::stdout();
        stdout.write_all(b"stray line\n").unwrap();
        stdout.flush().unwrap();
        // A program the worker starts inherits descriptor 1 as well.
        let status = Command::new("sh")
            .args(["-c", "echo inherited line"])
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn what_is_written_to_stdout_after_detaching_never_reaches_the_event_pipe() {
        if std::env::var_os(CHILD).is_some() {
            detached();
            return;
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = spawn_gate::hold(|| command.spawn())
            .unwrap()
            .wait_with_output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "the child failed: {stdout}\n{stderr}"
        );
        assert!(stdout.contains("event line"), "{stdout}");
        assert!(!stdout.contains("stray line"), "{stdout}");
        assert!(!stdout.contains("inherited line"), "{stdout}");
    }

    /// Set in the process the stdin test starts, which then runs
    /// [`stdin_detached`].
    const STDIN_CHILD: &str = "LADDERBACK_TEST_DETACH_STDIN";

    const STDIN_TEST: &str =
        "worker::stdio::tests::a_program_the_worker_starts_never_reads_the_daemons_pipe";

    /// The body of the stdin test, in the process whose stdin is the pipe
    /// with one answer line in it.
    fn stdin_detached() {
        use std::io::BufRead;

        let answers = detach_stdin().unwrap();
        let flags = nix::fcntl::fcntl(&answers, nix::fcntl::FcntlArg::F_GETFD).unwrap();
        assert!(
            nix::fcntl::FdFlag::from_bits_truncate(flags).contains(nix::fcntl::FdFlag::FD_CLOEXEC),
            "the answers' descriptor would be inherited"
        );
        // A program the worker starts inherits descriptor 0.
        let inherited = Command::new("cat")
            .stdin(Stdio::inherit())
            .output()
            .unwrap();
        assert!(inherited.status.success());
        assert_eq!(
            String::from_utf8_lossy(&inherited.stdout),
            "",
            "a program the worker started read the daemon's pipe"
        );
        let mut line = String::new();
        io::BufReader::new(answers).read_line(&mut line).unwrap();
        assert_eq!(line, "answer line\n");
    }

    #[test]
    fn a_program_the_worker_starts_never_reads_the_daemons_pipe() {
        if std::env::var_os(STDIN_CHILD).is_some() {
            stdin_detached();
            return;
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", STDIN_TEST, "--nocapture", "--test-threads=1"])
            .env(STDIN_CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn_gate::hold(|| command.spawn()).unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"answer line\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "the child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
