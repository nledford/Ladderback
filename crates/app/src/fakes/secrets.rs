use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use async_trait::async_trait;
use domain::secrets::{
    CommandLine, KeyringEntry, Problem, SecretError, SecretRef, SourceName, Stdout,
    command_failure, command_secret, decode, file_warning,
};

use crate::ports::{Resolved, SecretResolver};

/// What the fake does when told to run a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Script {
    /// Runs to its end with this exit status and output.
    Exits {
        status: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// Exits with this status and output, while something it started still
    /// holds its stdout open when the pipe grace runs out: the resolver has
    /// only what came before.
    LeavesOutputOpen {
        status: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// Never ends on its own: the resolver's time limit does.
    Hangs,
}

/// What a fake resolver reports for a command that hangs: the real one
/// reports the limit it was given, which is a minute unless set otherwise.
const FAKE_TIME_LIMIT_SECONDS: u64 = 60;

/// A secret store in memory: an environment, files with permissions, a
/// keyring and scripted commands, read through the same rules as the real
/// resolver (`domain::secrets::{decode, file_warning}`). It remembers what it
/// was asked, so a scenario can tell how often a secret was read.
#[derive(Default)]
pub struct FakeSecrets {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    env: BTreeMap<String, String>,
    files: BTreeMap<String, File>,
    directories: BTreeSet<String>,
    keyring: BTreeMap<String, String>,
    commands: BTreeMap<(String, Vec<String>), Script>,
    /// Link path to what it leads to.
    links: BTreeMap<String, String>,
    reads: Vec<SecretRef>,
}

/// More links than a path is followed through: a cycle never ends.
const MAX_LINKS_FOLLOWED: usize = 40;

/// Where `path` leads when the links are followed, `None` for a cycle.
fn follow(links: &BTreeMap<String, String>, path: &str) -> Option<String> {
    let mut at = path;
    for _ in 0..=MAX_LINKS_FOLLOWED {
        match links.get(at) {
            Some(target) => at = target,
            None => return Some(at.to_owned()),
        }
    }
    None
}

struct File {
    contents: Vec<u8>,
    /// Unix permission bits.
    mode: u32,
}

impl FakeSecrets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_env(&self, name: &str, value: &str) {
        self.lock().env.insert(name.to_owned(), value.to_owned());
    }

    pub fn write_file(&self, path: &str, contents: &[u8], mode: u32) {
        self.lock().files.insert(
            path.to_owned(),
            File {
                contents: contents.to_vec(),
                mode,
            },
        );
    }

    /// A link at `path` that leads to `target`, a file written with
    /// [`FakeSecrets::write_file`]: reading `path` reads `target`, and where
    /// `path` really is is where `target` is.
    pub fn link(&self, path: &str, target: &str) {
        self.lock().links.insert(path.to_owned(), target.to_owned());
    }

    /// A directory at `path`: reading it as a file fails.
    pub fn make_directory(&self, path: &str) {
        self.lock().directories.insert(path.to_owned());
    }

    pub fn set_keyring_entry(&self, entry: &KeyringEntry, secret: &str) {
        self.lock()
            .keyring
            .insert(entry.to_string(), secret.to_owned());
    }

    /// A machine with no keyring service to ask, as a server with no
    /// Secret Service: every keyring entry fails as `KeyringUnavailable`.
    pub fn remove_keyring_service(&self) {
        todo!()
    }

    /// What running `command` does. A command with no script can't be started.
    pub fn script(&self, command: &CommandLine, script: Script) {
        self.lock().commands.insert(
            (command.program().to_owned(), command.args().to_vec()),
            script,
        );
    }

    /// Every secret the resolver was asked for, in order.
    pub fn reads(&self) -> Vec<SecretRef> {
        self.lock().reads.clone()
    }

    /// How many times `secret` was asked for.
    pub fn read_count(&self, secret: &SecretRef) -> usize {
        self.lock()
            .reads
            .iter()
            .filter(|read| *read == secret)
            .count()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while holding this lock leaves plain data; keep using it.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[async_trait]
impl SecretResolver for FakeSecrets {
    async fn resolve(&self, secret: &SecretRef) -> Result<Resolved, SecretError> {
        let mut state = self.lock();
        state.reads.push(secret.clone());
        match secret {
            SecretRef::Inline(value) => Ok(Resolved {
                value: value.clone(),
                warnings: Vec::new(),
            }),
            SecretRef::Env(name) => {
                let source = SourceName::Env { name: name.clone() };
                let value = state
                    .env
                    .get(name)
                    .ok_or_else(|| SecretError::missing(source.clone()))?;
                let value = decode(&source, value.as_bytes(), |text| text)?;
                Ok(Resolved {
                    value,
                    warnings: Vec::new(),
                })
            }
            SecretRef::File { path, read } => {
                let source = SourceName::File { path: path.clone() };
                // Where the path really is, after the links.
                let Some(real) = follow(&state.links, path) else {
                    return Err(SecretError {
                        source,
                        problem: Problem::Unreachable {
                            detail: "too many levels of symbolic links".to_owned(),
                        },
                    });
                };
                if state.directories.contains(&real) {
                    return Err(SecretError {
                        source,
                        problem: Problem::Unreachable {
                            detail: "is a directory".to_owned(),
                        },
                    });
                }
                let file = state
                    .files
                    .get(&real)
                    .ok_or_else(|| SecretError::missing(source.clone()))?;
                let value = decode(&source, &file.contents, |text| read.extract(text))?;
                let credentials = state.env.get("CREDENTIALS_DIRECTORY").map(String::as_str);
                Ok(Resolved {
                    value,
                    warnings: file_warning(path, file.mode, credentials, |at| {
                        follow(&state.links, at)
                    })
                    .into_iter()
                    .collect(),
                })
            }
            SecretRef::Command(command) => {
                let source = SourceName::Command {
                    program: command.program().to_owned(),
                };
                let key = (command.program().to_owned(), command.args().to_vec());
                let fail = |problem| SecretError {
                    source: source.clone(),
                    problem,
                };
                match state.commands.get(&key) {
                    None => Err(fail(Problem::Unreachable {
                        detail: "no such file or directory".to_owned(),
                    })),
                    Some(Script::Hangs) => Err(fail(Problem::TimedOut {
                        seconds: FAKE_TIME_LIMIT_SECONDS,
                    })),
                    Some(Script::Exits {
                        status,
                        stdout,
                        stderr,
                    }) => exited(&source, *status, Stdout::ReadToEnd(stdout), stderr),
                    Some(Script::LeavesOutputOpen {
                        status,
                        stdout,
                        stderr,
                    }) => exited(&source, *status, Stdout::LeftOpen(stdout), stderr),
                }
            }
            SecretRef::Keyring(entry) => {
                let source = SourceName::Keyring {
                    entry: entry.to_string(),
                };
                let secret = state
                    .keyring
                    .get(&entry.to_string())
                    .ok_or_else(|| SecretError::missing(source.clone()))?;
                let value = decode(&source, secret.as_bytes(), |text| text)?;
                Ok(Resolved {
                    value,
                    warnings: Vec::new(),
                })
            }
        }
    }
}

/// What a scripted command that exited with `status` gives, read through the
/// rules the real resolver reads it by.
fn exited(
    source: &SourceName,
    status: i32,
    stdout: Stdout<'_>,
    stderr: &[u8],
) -> Result<Resolved, SecretError> {
    if status != 0 {
        return Err(command_failure(source, Some(status), stdout, stderr));
    }
    Ok(Resolved {
        value: command_secret(source, stdout)?,
        warnings: Vec::new(),
    })
}
