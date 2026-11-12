//! The backends of a repository the daemon opens itself, with the `rclone`
//! server of an `rclone:` location started through the spawn gate one start
//! at a time, and waited for outside it (§16, decision 58; #319).
//!
//! rustic's `RcloneBackend::new` starts `rclone version` and `rclone serve`,
//! and reads the server's output until it says where it serves, all in one
//! call that hands back no child to stop: built inside the gate, it held
//! every other start in the daemon until `rclone` served, however long that
//! took, and built outside it, its starts could inherit another start's
//! pipes, or be inherited. So this starts the server as that call does, at
//! the pinned revision (`rclone.rs`), but holds the gate for each start
//! alone: each start makes its pipes inside the call that starts the child,
//! so they are close-on-exec once it lets go. Once the server says where it
//! serves, the repository is rustic's own REST backend at that address,
//! named `rclone:<path>`, as in rustic's.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;

use bytes::Bytes;
use domain::secrets::{Redactor, SecretValue};
use rustic_backend::choose::{BackendChoice, SupportedBackend};
use rustic_backend::util::location_to_type_and_path;
use rustic_backend::{BackendOptions, RestBackend};
use rustic_core::{
    BytesList, CommandInput, ErrorKind, FileType, Id, ReadBackend, RepositoryBackends, RusticError,
    RusticResult, WriteBackend,
};

/// What rustic runs when the options name no `rclone-command`.
const DEFAULT_COMMAND: &str = "rclone serve restic --addr localhost:0";

/// What the server writes before the address it serves on.
const SERVING: &str = "Serving restic REST API on ";

/// The most of one line of `rclone`'s output kept: the bytes are another
/// program's, so they never size an allocation unbounded (§13.5).
const MAX_LINE_BYTES: usize = 64 * 1024;

/// The oldest `rclone` that takes the server's user and password from its
/// environment; an older one would serve the repository to anyone on the
/// machine, so rustic refuses it, and so does this.
const OLDEST_VERSION: (u64, u64, u64) = (1, 52, 2);

/// The user and password an `rclone` server is started with, and the
/// repository read through it with: random, and secrets of their own, which
/// the evaluation's redactor masks (I4), since rustic may quote the address
/// they are written in.
pub(crate) struct ServerCredentials {
    user: SecretValue,
    password: SecretValue,
}

impl ServerCredentials {
    /// 256 random bits each, in hex.
    pub(crate) fn random() -> Self {
        let random = || SecretValue::new(Id::random().to_hex().as_str());
        Self {
            user: random(),
            password: random(),
        }
    }

    /// Both, for a redactor to mask.
    pub(crate) fn secrets(&self) -> [&SecretValue; 2] {
        [&self.user, &self.password]
    }
}

/// A repository's backends, and the `rclone` servers they reach it through.
/// Dropping it stops the servers.
pub(crate) struct Backends {
    pub(crate) backends: RepositoryBackends,
    _servers: Vec<Server>,
}

/// The backends of `of`, as its `to_backends` builds them: the cold half
/// reads `options` and `options-cold`, the hot half `options` and
/// `options-hot`, a half's own option replacing the shared one. A server
/// is started with `credentials`, and its output logged through `redactor`.
pub(crate) fn backends(
    of: &BackendOptions,
    credentials: &ServerCredentials,
    redactor: &Redactor,
) -> RusticResult<Backends> {
    let mut servers = Vec::new();
    let repository = of.repository.as_ref().ok_or_else(|| {
        RusticError::new(
            ErrorKind::Backend,
            "No repository given. Please make sure, that you have set the repository.",
        )
    })?;
    let mut serving = Serving {
        credentials,
        redactor,
        servers: &mut servers,
    };
    let mut options = of.options.clone();
    options.extend(of.options_cold.clone());
    let cold = serving.backend(repository, options)?;
    let hot = match &of.repo_hot {
        Some(location) => {
            let mut options = of.options.clone();
            options.extend(of.options_hot.clone());
            Some(serving.backend(location, options)?)
        }
        None => None,
    };
    Ok(Backends {
        backends: RepositoryBackends::new(cold, hot),
        _servers: servers,
    })
}

/// What the backends of one repository are built with, and the servers
/// started for them so far.
struct Serving<'a> {
    credentials: &'a ServerCredentials,
    redactor: &'a Redactor,
    servers: &'a mut Vec<Server>,
}

impl Serving<'_> {
    /// The backend at `location`, as rustic chooses it; an `rclone:` one is
    /// reached through a server this starts.
    fn backend(
        &mut self,
        location: &str,
        options: BTreeMap<String, String>,
    ) -> RusticResult<Arc<dyn WriteBackend>> {
        let (kind, path) = location_to_type_and_path(location)?;
        let built = if kind == SupportedBackend::Rclone {
            self.serve(&path, options)
        } else {
            kind.to_backend(path.clone(), Some(options))
        };
        built.map_err(|error| {
            error
                .prepend_guidance_line("Could not load the backend `{name}` at `{location}`. Please check the given backend and try again.")
                .attach_context("name", kind.to_string())
                .attach_context("location", path.to_string())
        })
    }

    /// Starts `rclone` serving `path` as rustic's backend starts it, and the
    /// REST backend that reaches the repository through it.
    fn serve(
        &mut self,
        path: &str,
        options: BTreeMap<String, String>,
    ) -> RusticResult<Arc<dyn WriteBackend>> {
        let rclone_command = options.get("rclone-command");
        let use_password = match options.get("use-password") {
            Some(value) => value.parse().map_err(|error| {
                RusticError::with_source(
                    ErrorKind::InvalidInput,
                    "Expected 'use-password' to be a boolean, but it was not. Please check the configuration file.",
                    error,
                )
            })?,
            None => true,
        };
        if use_password && rclone_command.is_none() {
            supported_version(Command::new("rclone"))?;
        }
        let mut rclone_command: CommandInput = rclone_command
            .map_or(DEFAULT_COMMAND, String::as_str)
            .parse()
            .map_err(|error| {
                RusticError::with_source(
                    ErrorKind::InvalidInput,
                    "Expected rclone command to be valid, but it was not. Please check the configuration file.",
                    error,
                )
            })?;
        rclone_command.append_arg(path.to_owned());
        let user = self.credentials.user.expose();
        let password = self.credentials.password.expose();
        let mut command = Command::new(rclone_command.command());
        if use_password {
            let _ = command
                .env("RCLONE_USER", user)
                .env("RCLONE_PASS", password);
        }
        log::debug!("starting rclone via {rclone_command:?}");
        let _ = command.args(rclone_command.args()).stderr(Stdio::piped());
        let mut server = Server {
            reader: None,
            child: spawn_gate::hold(|| command.spawn()).map_err(|error| {
                RusticError::with_source(
                    ErrorKind::ExternalCommand,
                    "Experienced an error while running rclone: `{rclone_command}`. Please check if rclone is installed and working correctly.",
                    error,
                )
                .attach_context("rclone_command", rclone_command.to_string())
            })?,
        };
        let mut output = BufReader::new(server.child.stderr.take().ok_or_else(|| {
            RusticError::new(
                ErrorKind::ExternalCommand,
                "Could not get stderr of rclone. Please check if rclone is installed and working correctly.",
            )
        })?);
        let address = match options.get("rest-url") {
            Some(address) => address.clone(),
            None => served_at(&mut server.child, &mut output, self.redactor)?,
        };
        let url = if use_password {
            let Some(host) = address.strip_prefix("http://") else {
                return Err(RusticError::new(
                    ErrorKind::InputOutput,
                    "Please make sure, the URL `{url}` starts with 'http://'!",
                )
                .attach_context("url", address));
            };
            format!("http://{user}:{password}@{host}")
        } else {
            address
        };
        let rest = RestBackend::new(url, options)?;
        server.reader = Some(keep_reading(output, self.redactor.clone())?);
        self.servers.push(server);
        Ok(Arc::new(Served {
            location: format!("rclone:{path}"),
            rest,
        }))
    }
}

/// rustic's REST backend at the address an `rclone` server serves on, named
/// as rustic's own `rclone:` backend is named: by the repository's path, as
/// the user wrote it, rather than by the server's port and user, which mean
/// nothing outside this evaluation.
struct Served {
    location: String,
    rest: RestBackend,
}

impl ReadBackend for Served {
    fn location(&self) -> String {
        self.location.clone()
    }

    fn list_with_size(&self, tpe: FileType) -> RusticResult<Vec<(Id, u32)>> {
        self.rest.list_with_size(tpe)
    }

    fn read_full(&self, tpe: FileType, id: &Id) -> RusticResult<Bytes> {
        self.rest.read_full(tpe, id)
    }

    fn read_partial(
        &self,
        tpe: FileType,
        id: &Id,
        cacheable: bool,
        offset: u32,
        length: u32,
    ) -> RusticResult<Bytes> {
        self.rest.read_partial(tpe, id, cacheable, offset, length)
    }

    fn warmup_path(&self, tpe: FileType, id: &Id) -> String {
        self.rest.warmup_path(tpe, id)
    }
}

impl WriteBackend for Served {
    fn create(&self) -> RusticResult<()> {
        self.rest.create()
    }

    fn write_bytes(
        &self,
        tpe: FileType,
        id: &Id,
        cacheable: bool,
        content: BytesList,
    ) -> RusticResult<()> {
        self.rest.write_bytes(tpe, id, cacheable, content)
    }

    fn remove(&self, tpe: FileType, id: &Id, cacheable: bool) -> RusticResult<()> {
        self.rest.remove(tpe, id, cacheable)
    }
}

/// A started `rclone` server, stopped and reaped when dropped, and the
/// thread reading its output joined: what the server writes while it stops
/// is logged before this returns, inside whatever masking the caller holds
/// (I4), as rustic's own backend does.
struct Server {
    child: Child,
    reader: Option<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        // An `rclone` that has exited already is only reaped.
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Ends with the output: now the server is gone, unless a program it
        // started still holds the pipe, which this then waits for.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Fails unless `rclone`, run as `rclone version`, is [`OLDEST_VERSION`] or
/// newer. Its start holds the gate, the wait for its answer doesn't.
fn supported_version(mut rclone: Command) -> RusticResult<()> {
    let failed = |error: std::io::Error| {
        RusticError::with_source(
            ErrorKind::ExternalCommand,
            "Experienced an error while running `rclone version` command. Please check if rclone is installed correctly and is in your PATH.",
            error,
        )
    };
    let _ = rclone
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = spawn_gate::hold(|| rclone.spawn()).map_err(failed)?;
    let mut first = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let mut stdout = BufReader::new(stdout);
        let read = read_line(&mut stdout, &mut first);
        // The rest of its output is not read: letting go of the pipe ends
        // any write of it. Through the gate, so no start in flight holds a
        // copy of its read end that would keep the write going (decision 58).
        spawn_gate::hold(|| drop(stdout));
        if let Err(error) = read {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failed(error));
        }
    }
    let _ = child.wait().map_err(failed)?;
    let first = String::from_utf8_lossy(&first);
    match version_of(&first) {
        Some(version) if version >= OLDEST_VERSION => Ok(()),
        Some(_) => Err(RusticError::new(
            ErrorKind::Unsupported,
            "Unsupported rclone version `{version}`. We must not use rclone without authentication! Please upgrade to rclone >= 1.52.2!",
        )
        .attach_context("version", first.trim().to_owned())),
        None => Err(RusticError::new(
            ErrorKind::Internal,
            "Error parsing rclone version `{version}`. This should not happen. Please check the `rclone version` output manually.",
        )
        .attach_context("version", first.trim().to_owned())),
    }
}

/// The version in the first line `rclone version` prints (`rclone v1.65.0`,
/// `rclone v1.66.0-beta.7712.84a8d4f`), as rustic reads it: from its first
/// digit, three numbers and whatever pre-release or build follows them.
fn version_of(first_line: &str) -> Option<(u64, u64, u64)> {
    let version = first_line
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit());
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut numbers = core.split('.').map(|number| {
        let leading_zero = number.len() > 1 && number.starts_with('0');
        let digits = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
        (digits && !leading_zero)
            .then(|| number.parse::<u64>().ok())
            .flatten()
    });
    let version = (numbers.next()??, numbers.next()??, numbers.next()??);
    numbers.next().is_none().then_some(version)
}

/// Reads the server's output until it says where it serves, and returns
/// that address. Each other line is logged, masked (I4).
fn served_at(
    child: &mut Child,
    output: &mut BufReader<ChildStderr>,
    redactor: &Redactor,
) -> RusticResult<String> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = read_line(output, &mut line).map_err(|error| {
            RusticError::with_source(
                ErrorKind::InputOutput,
                "Experienced an error while reading rclone output. Please check if rclone is installed and working correctly.",
                error,
            )
        })?;
        let Some(cut) = read else {
            // Its output ended: it exited, or it never will say. Killing an
            // `rclone` that has exited keeps the status it exited with.
            let _ = child.kill();
            let status = child.wait().map_err(|error| {
                RusticError::with_source(
                    ErrorKind::ExternalCommand,
                    "Experienced an error while running rclone. Please check if rclone is installed and working correctly.",
                    error,
                )
            })?;
            return Err(RusticError::new(
                ErrorKind::ExternalCommand,
                "rclone exited before it could start the REST server: `{exit_status}`. Please check the exit status for more information.",
            )
            .attach_context("exit_status", status.to_string()));
        };
        if let Some(address) = address_in(&String::from_utf8_lossy(&line)) {
            return Ok(address);
        }
        log_line(&line, cut, redactor);
    }
}

/// The address a line of the server's says it serves on, as rustic reads
/// it: what follows [`SERVING`], without the brackets rclone 1.61 and later
/// put around it.
fn address_in(line: &str) -> Option<String> {
    let at = line.find(SERVING)?;
    let address = line.get(at + SERVING.len()..)?;
    Some(address.trim_end().trim_matches(['[', ']']).to_owned())
}

/// Reads the rest of the server's output on a thread of its own, so a full
/// pipe never stops it, and logs each line, masked. The thread ends when the
/// output does: when the server is stopped, unless a program it started
/// still holds it; [`Server`]'s drop waits for that.
fn keep_reading(
    mut output: BufReader<ChildStderr>,
    redactor: Redactor,
) -> RusticResult<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("rclone-output".to_owned())
        .spawn(move || {
            let mut line = Vec::new();
            while let Ok(Some(cut)) = read_line(&mut output, &mut line) {
                log_line(&line, cut, &redactor);
                line.clear();
            }
        })
        .map_err(|error| {
            RusticError::with_source(
                ErrorKind::Internal,
                "Could not start reading rclone's output.",
                error,
            )
        })
}

/// Reads one line of `reader` into `line`, without its newline: the first
/// [`MAX_LINE_BYTES`] bytes of it. The rest of a longer line is read and
/// dropped, never held. `Some(cut)` is a line, `cut` whether it was longer
/// than that or ended without its newline; `None` is the end of the output.
fn read_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> std::io::Result<Option<bool>> {
    let mut cut = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            // A last line without its newline counts, as a cut one.
            return Ok((!line.is_empty() || cut).then_some(true));
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let content = newline.unwrap_or(available.len());
        let taken = newline.map_or(available.len(), |at| at.saturating_add(1));
        let room = MAX_LINE_BYTES.saturating_sub(line.len());
        cut |= content > room;
        line.extend_from_slice(available.get(..content.min(room)).unwrap_or_default());
        reader.consume(taken);
        if newline.is_some() {
            return Ok(Some(cut));
        }
    }
}

/// Logs a line of the server's output, masked: as a cut text when it was
/// cut, so a secret the cut went through leaves nothing behind (I4).
fn log_line(line: &[u8], cut: bool, redactor: &Redactor) {
    let text = masked(line, cut, redactor);
    if !text.is_empty() {
        log::info!("rclone output: {text}");
    }
}

/// A line of the server's output as it is logged.
fn masked(line: &[u8], cut: bool, redactor: &Redactor) -> String {
    if cut {
        redactor.redact_cut(line)
    } else {
        redactor.redact(String::from_utf8_lossy(line).trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_its_first_digit_without_its_pre_release() {
        assert_eq!(version_of("rclone v1.65.0\n"), Some((1, 65, 0)));
        assert_eq!(
            version_of("rclone v1.66.0-beta.7712.84a8d4f"),
            Some((1, 66, 0))
        );
        assert_eq!(version_of("rclone v1.52.2+build"), Some((1, 52, 2)));
    }

    #[test]
    fn a_version_that_is_not_three_numbers_is_none() {
        assert_eq!(version_of("rclone v1.65"), None);
        assert_eq!(version_of("rclone v1.65.0.1"), None);
        assert_eq!(version_of("rclone v1.6x.0"), None);
        assert_eq!(version_of("rclone v1.065.0"), None);
        assert_eq!(version_of("rclone"), None);
    }

    #[test]
    fn the_oldest_supported_version_is_1_52_2() {
        assert!(version_of("rclone v1.52.2").unwrap() >= OLDEST_VERSION);
        assert!(version_of("rclone v1.52.1").unwrap() < OLDEST_VERSION);
        assert!(version_of("rclone v1.9.9").unwrap() < OLDEST_VERSION);
        assert!(version_of("rclone v2.0.0").unwrap() >= OLDEST_VERSION);
    }

    #[test]
    fn the_address_follows_the_serving_line_without_its_brackets() {
        assert_eq!(
            address_in("2026/11/12 NOTICE: Serving restic REST API on [http://127.0.0.1:53682/]\n"),
            Some("http://127.0.0.1:53682/".to_owned())
        );
        assert_eq!(
            address_in("NOTICE: Serving restic REST API on http://[::1]:53682/\n"),
            Some("http://[::1]:53682/".to_owned())
        );
        assert_eq!(address_in("NOTICE: Config file not found\n"), None);
    }

    /// `rclone version` run by `sh`: `script` is what it does, and the
    /// `version` argument is its `$1`.
    #[cfg(unix)]
    fn rclone_version(script: &str) -> Command {
        let mut command = Command::new("sh");
        let _ = command.args(["-c", script, "sh"]);
        command
    }

    #[cfg(unix)]
    #[test]
    fn an_rclone_older_than_1_52_2_is_refused() {
        let refused = supported_version(rclone_version("echo \"rclone v1.52.1\""))
            .expect_err("rclone 1.52.1 serves without a password");
        assert!(refused.to_string().contains("rclone v1.52.1"), "{refused}");
        supported_version(rclone_version(
            "echo \"rclone v1.52.2\"; echo \"- os/version: demo\"",
        ))
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_rclone_whose_version_is_not_one_is_refused() {
        supported_version(rclone_version("echo \"rclone $1\""))
            .expect_err("`version` is no version");
        supported_version(rclone_version("exit 0")).expect_err("no answer is no version");
    }

    /// The wait for `rclone version`'s answer holds up no other start: the
    /// gate is held for its start alone (decision 58, #319).
    #[cfg(unix)]
    #[test]
    fn an_rclone_version_that_never_answers_holds_up_no_other_start() {
        use std::time::{Duration, Instant};
        let scratch = tempfile::tempdir().unwrap();
        let started = scratch.path().join("started");
        let let_go = scratch.path().join("let-go");
        // Stalls until `$2` is made, then answers; `version` follows the two
        // files as `$3`.
        let mut command = rclone_version(
            ": > \"$1\"; n=0; while [ ! -e \"$2\" ] && [ $n -lt 600 ]; do sleep 0.05; n=$((n + 1)); done; echo \"rclone v1.65.0\"",
        );
        let _ = command.arg(&started).arg(&let_go);
        let checking = std::thread::spawn(move || supported_version(command));
        let waiting_since = Instant::now();
        while !started.exists() {
            assert!(
                waiting_since.elapsed() < Duration::from_secs(10),
                "the rclone stand-in never started"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Nothing else in this test's process holds the gate, so a free gate
        // is taken at once; 2 s is slack for a loaded machine.
        let (taken, took) = std::sync::mpsc::channel();
        let other_start = std::thread::spawn(move || {
            spawn_gate::hold(|| {
                let _ = taken.send(());
            });
        });
        let free = took.recv_timeout(Duration::from_secs(2)).is_ok();
        std::fs::write(&let_go, b"").unwrap();
        let checked = checking.join().unwrap();
        other_start.join().unwrap();
        assert!(free, "another start waited for rclone version's answer");
        checked.unwrap();
    }

    #[test]
    fn the_rest_of_a_line_past_the_cap_is_dropped_not_read_as_a_line() {
        let mut output = vec![b'a'; MAX_LINE_BYTES + 10];
        output.extend_from_slice(b"\nnext\nlast");
        let mut reader = output.as_slice();
        let mut line = Vec::new();
        assert_eq!(read_line(&mut reader, &mut line).unwrap(), Some(true));
        assert_eq!(line.len(), MAX_LINE_BYTES);
        line.clear();
        assert_eq!(read_line(&mut reader, &mut line).unwrap(), Some(false));
        assert_eq!(line, b"next");
        line.clear();
        assert_eq!(read_line(&mut reader, &mut line).unwrap(), Some(true));
        assert_eq!(line, b"last");
        line.clear();
        assert_eq!(read_line(&mut reader, &mut line).unwrap(), None);
    }

    #[test]
    fn a_line_of_64_kib_is_read_whole_and_one_byte_more_is_cut() {
        const CAP: usize = 64 * 1024;
        for (length, cut) in [(CAP, false), (CAP + 1, true)] {
            let mut output = vec![b'a'; length];
            output.push(b'\n');
            let mut line = Vec::new();
            let read = read_line(&mut output.as_slice(), &mut line).unwrap();
            assert_eq!(read, Some(cut), "a line of {length} bytes");
            assert_eq!(line.len(), CAP.min(length), "a line of {length} bytes");
        }
    }

    #[test]
    fn a_secret_the_cap_cuts_through_is_masked_and_never_logged_in_part() {
        let secret = SecretValue::new("CANARY-rclone-secret");
        let redactor = Redactor::new([&secret]);
        let mut output = vec![b'a'; MAX_LINE_BYTES - 6];
        output.extend_from_slice(b"CANARY-rclone-secret\nnext CANARY-rclone-secret\n");
        let mut reader = output.as_slice();
        let mut logged = Vec::new();
        let mut line = Vec::new();
        while let Some(cut) = read_line(&mut reader, &mut line).unwrap() {
            logged.push(masked(&line, cut, &redactor));
            line.clear();
        }
        assert_eq!(logged.len(), 2, "the cut line was read as two");
        for text in &logged {
            assert!(!text.contains("CANARY"), "{text}");
            assert!(!text.contains("rclone-secret"), "{text}");
        }
    }
}
