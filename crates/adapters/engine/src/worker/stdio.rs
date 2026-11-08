//! The worker's standard output belongs to the event stream alone.
//!
//! A program the worker starts inherits its file descriptors. rustic's
//! rclone backend starts `rclone`, which writes to stderr only, but still
//! holds stdout: while it runs, the daemon's reader never sees the pipe close,
//! and anything such a program printed there would corrupt the stream. So the
//! worker keeps the event pipe under a descriptor of its own, marked
//! close-on-exec, and points descriptor 1 at `/dev/null`.

use std::fs::File;
use std::io;
#[cfg(unix)]
use std::os::fd::AsFd;

/// The process's stdout as a file for the event stream. Afterwards
/// descriptor 1 is `/dev/null`: whatever else writes to stdout is dropped,
/// and no program the worker starts inherits the pipe.
#[cfg(unix)]
pub(crate) fn detach_stdout() -> io::Result<File> {
    todo!()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;

    /// Set in the process the test starts, which then runs [`detached`]
    /// instead of starting one more: redirecting descriptor 1 inside the
    /// test run itself would swallow the harness's own output.
    const CHILD: &str = "STEADING_TEST_DETACH_STDOUT";

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
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .stdin(Stdio::null())
            .output()
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
}
