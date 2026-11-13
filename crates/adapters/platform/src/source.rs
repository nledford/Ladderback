//! Reading what the operating system reports, as hostile input: a folder of
//! the kernel's (sysfs) with bounded reads and a cap on its entries, or a
//! program's output, bounded in size and time. Every surprise is a typed
//! `PlatformError`, never a guess and never a panic.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use app::ports::PlatformError;
use tokio::io::AsyncReadExt;
use tokio::process::ChildStdout;

/// How long a source may take to answer before it is given up.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(5);

/// The most entries read of one sysfs class folder.
const MAX_ENTRIES: usize = 1024;

/// The most bytes read of one attribute: every value read is a word or a
/// number.
const MAX_ATTRIBUTE_BYTES: u64 = 256;

/// The most bytes read of a program's output.
const MAX_OUTPUT_BYTES: u64 = 64 * 1024;

/// The most characters of what was read that an error repeats.
const MAX_DETAIL_CHARS: usize = 120;

/// What an error says it read: at most [`MAX_DETAIL_CHARS`] characters, a
/// control character among them (a terminal escape, a line break) written
/// out as an escape, so the agent's log shows it rather than acts on it.
pub(crate) fn detail(text: &str) -> String {
    let mut shown = String::new();
    for character in text.chars().take(MAX_DETAIL_CHARS) {
        if character.is_control() {
            shown.extend(character.escape_default());
        } else {
            shown.push(character);
        }
    }
    shown
}

fn unreadable(from: &Path, error: impl ToString) -> PlatformError {
    PlatformError::Unreadable {
        from: from.display().to_string(),
        detail: error.to_string(),
    }
}

pub(crate) fn unrecognized(from: &Path, what: &str) -> PlatformError {
    PlatformError::Unrecognized {
        from: from.display().to_string(),
        detail: detail(what),
    }
}

/// Runs `read` on the blocking pool, giving it up after `timeout`. A read
/// given up keeps its thread until the read returns; the caller has its
/// answer.
pub(crate) async fn blocking<T: Send + 'static>(
    root: &Path,
    timeout: Duration,
    read: impl FnOnce() -> Result<T, PlatformError> + Send + 'static,
) -> Result<T, PlatformError> {
    match tokio::time::timeout(timeout, tokio::task::spawn_blocking(read)).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(joined)) => Err(unreadable(root, joined)),
        Err(_) => Err(PlatformError::TimedOut {
            from: root.display().to_string(),
        }),
    }
}

/// The entries of a sysfs class folder, at most [`MAX_ENTRIES`]; `None`
/// when there is no such folder.
pub(crate) fn entries(root: &Path) -> Result<Option<Vec<PathBuf>>, PlatformError> {
    let listing = match std::fs::read_dir(root) {
        Ok(listing) => listing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(root, error)),
    };
    let mut entries = Vec::new();
    for entry in listing {
        let entry = entry.map_err(|error| unreadable(root, error))?;
        if entries.len() == MAX_ENTRIES {
            return Err(unrecognized(
                root,
                &format!("more than {MAX_ENTRIES} entries"),
            ));
        }
        entries.push(entry.path());
    }
    entries.sort();
    Ok(Some(entries))
}

/// One attribute of a sysfs entry, without the white space around it;
/// `None` when the entry has no such attribute.
pub(crate) fn attribute(entry: &Path, name: &str) -> Result<Option<String>, PlatformError> {
    let path = entry.join(name);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(&path, error)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_ATTRIBUTE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| unreadable(&path, error))?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_ATTRIBUTE_BYTES) {
        return Err(unrecognized(
            &path,
            &format!("longer than {MAX_ATTRIBUTE_BYTES} bytes"),
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| unrecognized(&path, "not UTF-8"))?;
    Ok(Some(text.trim().to_owned()))
}

/// The most bytes read of one table of the kernel's (`/proc/net/route`):
/// a few lines per route.
const MAX_TABLE_BYTES: u64 = 64 * 1024;

/// A table of the kernel's, whole; `None` when there is no such file.
pub(crate) fn table(path: &Path) -> Result<Option<String>, PlatformError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(path, error)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_TABLE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| unreadable(path, error))?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_TABLE_BYTES) {
        return Err(unrecognized(
            path,
            &format!("longer than {MAX_TABLE_BYTES} bytes"),
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| unrecognized(path, "not UTF-8"))
}

/// A program the adapter runs, with fixed arguments.
#[derive(Clone, Debug)]
pub(crate) struct Program {
    pub(crate) path: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) timeout: Duration,
    /// The most bytes of its output read.
    pub(crate) max_output: u64,
}

impl Program {
    pub(crate) fn new<A: Into<OsString>>(
        path: impl Into<PathBuf>,
        args: impl IntoIterator<Item = A>,
    ) -> Self {
        Self {
            path: path.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: TIMEOUT,
            max_output: MAX_OUTPUT_BYTES,
        }
    }

    /// This program given `timeout` and `max_output` bytes of output in
    /// place of the usual 5 s and 64 KiB, for one that is slow or says much.
    pub(crate) fn with_limits(self, timeout: Duration, max_output: u64) -> Self {
        Self {
            timeout,
            max_output,
            ..self
        }
    }

    /// Runs the program with no environment and nothing on stdin, through
    /// the spawn gate (decision 58), and returns what it printed. A program
    /// that fails, prints more than its `max_output` bytes or isn't done
    /// within its timeout gives no answer; it is killed when this returns.
    /// The timeout covers the whole call, the wait for the gate too.
    pub(crate) async fn output(&self) -> Result<String, PlatformError> {
        tokio::time::timeout(self.timeout, self.run())
            .await
            .unwrap_or_else(|_| {
                Err(PlatformError::TimedOut {
                    from: self.path.display().to_string(),
                })
            })
    }

    /// [`output`](Self::output), without its timeout.
    async fn run(&self) -> Result<String, PlatformError> {
        let from = self.path.as_path();
        let mut command = tokio::process::Command::new(&self.path);
        command
            .args(&self.args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        // The pipe goes into its guard inside the gate, so it is let go of
        // through the gate even when this call is no longer waiting.
        let (mut child, mut stdout) = spawn_gate::hold_blocking(move || {
            command.spawn().map(|mut child| {
                let stdout = Pipe(child.stdout.take());
                (child, stdout)
            })
        })
        .await
        .and_then(std::convert::identity)
        .map_err(|error| unreadable(from, error))?;
        let Some(pipe) = stdout.0.as_mut() else {
            return Err(unreadable(from, "its output is not piped"));
        };
        let max_output = self.max_output;
        let mut bytes = Vec::new();
        pipe.take(max_output.saturating_add(1))
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| unreadable(from, error))?;
        if u64::try_from(bytes.len()).map_or(true, |len| len > max_output) {
            return Err(unrecognized(
                from,
                &format!("more than {max_output} bytes of output"),
            ));
        }
        let status = child
            .wait()
            .await
            .map_err(|error| unreadable(from, error))?;
        if !status.success() {
            return Err(unreadable(from, status));
        }
        String::from_utf8(bytes).map_err(|_| unrecognized(from, "output not UTF-8"))
    }
}

/// The read end of a child's output. However it is let go of (read to its
/// end, given up, or with the call dropped), it is dropped through the spawn
/// gate, on the blocking pool: a start in flight may hold a copy until its
/// `exec`, and holds the gate as long (decision 58), so no caller waits for
/// that.
struct Pipe(Option<ChildStdout>);

impl Drop for Pipe {
    fn drop(&mut self) {
        let Some(pipe) = self.0.take() else {
            return;
        };
        let let_go = move || spawn_gate::hold(|| drop(pipe));
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(let_go)),
            // No runtime, so no task this could hold up.
            Err(_) => let_go(),
        }
    }
}
