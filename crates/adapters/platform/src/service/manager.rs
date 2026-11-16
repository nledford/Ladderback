//! Runs the manager's program: `systemctl`, `launchctl` or `loginctl`, found
//! on the `PATH` it is given, with argv only and an environment of its own.
//! The Full Disk Access pane's `open` runs through it too.

use std::ffi::{OsStr, OsString};
use std::process::Stdio;
use std::time::Duration;

use app::ports::ServiceProblem;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// The variables the manager's program is run with: what it needs to find
/// the user's manager (`XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`), to be
/// found itself (`PATH`), and to speak the user's language. No other
/// variable of the environment reaches it, so no secret does (I4).
#[derive(Clone, Debug, Default)]
pub struct Environment {
    kept: Vec<(OsString, OsString)>,
}

/// The names [`Environment`] keeps.
const KEPT: [&str; 8] = [
    "PATH",
    "HOME",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_MESSAGES",
];

/// The most of the manager's standard output, or of its standard error, a
/// call keeps.
const MAX_SAID: u64 = 4096;

impl Environment {
    /// The variables of `vars` the manager is run with.
    pub fn keeping(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let kept = vars
            .into_iter()
            .filter(|(name, _)| KEPT.iter().any(|kept| OsStr::new(kept) == name))
            .collect();
        Self { kept }
    }

    /// The variables kept.
    pub fn kept(&self) -> &[(OsString, OsString)] {
        &self.kept
    }
}

/// How one call of the manager ended.
pub(crate) enum Ended {
    /// It ended with 0, printing `out` on its standard output, perhaps cut
    /// short.
    Succeeded { out: String },
    /// It ended with an error, with exit code `code` (none when a signal
    /// ended it), saying `said`.
    Failed { code: Option<i32>, said: String },
}

/// Runs `program args`, with `environment` only, and waits at most `limit`.
///
/// # Errors
///
/// The program could not be started, or did not end within `limit`.
pub(crate) async fn run(
    program: &str,
    args: &[OsString],
    environment: &Environment,
    limit: Duration,
) -> Result<Ended, ServiceProblem> {
    let command_line = shown(program, args);
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .envs(environment.kept().iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Through the spawn gate: no other start inherits the pipe, and the
    // manager inherits none of theirs (decision 58).
    let mut child = spawn_gate::hold_blocking(move || command.spawn())
        .await
        .and_then(std::convert::identity)
        .map_err(|error| ServiceProblem::ManagerMissing {
            command: command_line.clone(),
            detail: error.to_string(),
        })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let finished = async {
        // Both at once: a program that fills one pipe while the other is
        // read would wait forever.
        let (out, said) = tokio::join!(kept(stdout), kept(stderr));
        (out, said, child.wait().await)
    };
    let silent = || ServiceProblem::ManagerSilent {
        command: command_line.clone(),
        seconds: limit.as_secs(),
    };
    // On a timeout the child is dropped, which kills it.
    let (out, said, status) = tokio::time::timeout(limit, finished)
        .await
        .map_err(|_| silent())?;
    let status = match status {
        Ok(status) => status,
        // The program ran, and how it ended is not known: a failure, in the
        // system's words.
        Err(error) => {
            return Ok(Ended::Failed {
                code: None,
                said: error.to_string(),
            });
        }
    };
    if status.success() {
        return Ok(Ended::Succeeded {
            out: String::from_utf8_lossy(&out).into_owned(),
        });
    }
    Ok(Ended::Failed {
        code: status.code(),
        said: String::from_utf8_lossy(&said).trim().to_owned(),
    })
}

/// The first [`MAX_SAID`] bytes of `pipe`. The rest is not kept, but read,
/// so the program never waits on a full pipe.
async fn kept(pipe: Option<impl AsyncRead + Unpin>) -> Vec<u8> {
    let mut kept = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = (&mut pipe).take(MAX_SAID).read_to_end(&mut kept).await;
        let _ = tokio::io::copy(&mut pipe, &mut tokio::io::sink()).await;
    }
    kept
}

/// The command line as a person would type it.
pub(crate) fn shown(program: &str, args: &[OsString]) -> String {
    let words: Vec<String> = std::iter::once(program.to_owned())
        .chain(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
        .collect();
    shell_words::join(words)
}
