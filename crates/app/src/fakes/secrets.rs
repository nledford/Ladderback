use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use domain::Timestamp;
use domain::secrets::{
    CommandLine, KeyringCause, KeyringEntry, Problem, SecretError, SecretRef, SourceName, Stdout,
    command_failure, command_secret, decode, file_warning,
};

use crate::ports::{
    Clock, Kept, Keyring, KeyringActivation, KeyringError, KeyringLock, Resolved, SecretResolver,
};

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

/// What a fake resolver reports for a command, a keyring or a file that
/// doesn't answer: the real one reports the limit it was given, which is a
/// minute unless set otherwise.
const FAKE_TIME_LIMIT_SECONDS: u64 = 60;

/// How often a keyring entry may be read while the keyring can't be asked
/// before the fake stops saying so: a run that holds for the keyring reads
/// it 13 times at most (`KEYRING_WAIT` over `KEYRING_POLL`), so more is a
/// run that spins.
const FAKE_UNAVAILABLE_READS: usize = 1_000;

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
    /// No keyring service to ask: every keyring read fails.
    no_keyring_service: bool,
    /// The session's keyring service starts when this clock reaches this
    /// time: until then there is none to ask, as with `no_keyring_service`.
    service_starts: Option<(Arc<dyn Clock>, Timestamp)>,
    /// The session's keyring service goes away when this clock reaches this
    /// time: from then there is none to ask, locked or not.
    service_ends: Option<(Arc<dyn Clock>, Timestamp)>,
    /// Asking whether the keyring is locked takes this long on this clock:
    /// the answer is the one it was when asked, and arrives that much later.
    lock_answer_takes: Option<(Arc<dyn Clock>, domain::SignedDuration)>,
    /// How the keyring service is started, when a test said; otherwise on
    /// demand, or absent with `no_keyring_service`.
    activation: Option<KeyringActivation>,
    /// The keyring is locked: every keyring read fails, entry or not.
    keyring_locked: bool,
    /// The keyring is locked until this clock reaches this time, as a login
    /// keyring nobody has unlocked yet; from then it is unlocked.
    unlock_at: Option<(Arc<dyn Clock>, Timestamp)>,
    /// The keyring is locked until it has been read this many times, as one
    /// a person unlocks right after the first look; from the next read it
    /// is unlocked.
    unlocks_after_reads: Option<usize>,
    /// The keyring's unlock prompt was dismissed: every keyring read fails,
    /// entry or not.
    unlock_prompt_dismissed: bool,
    /// The keyring service has no collection to look in: every keyring read
    /// fails, entry or not.
    no_default_collection: bool,
    /// The keyring answers that it is unlocked, but its entries are in a
    /// collection that stays locked, as one the default alias doesn't name:
    /// every keyring read fails as locked while asking whether the keyring
    /// is locked says unlocked.
    entries_stay_locked: bool,
    commands: BTreeMap<(String, Vec<String>), Script>,
    /// The keyring doesn't answer, as a macOS keychain waiting on a prompt
    /// nobody answered: every keyring read outlives the time limit.
    keyring_stalls: bool,
    /// Paths whose reads outlive the time limit, as on a hung network mount.
    stalled_files: BTreeSet<String>,
    /// Link path to what it leads to.
    links: BTreeMap<String, String>,
    reads: Vec<SecretRef>,
    /// How many times the resolver was asked whether the keyring is locked.
    lock_queries: usize,
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

    /// What the keyring entry holds, if it is there.
    pub fn keyring_entry(&self, entry: &KeyringEntry) -> Option<String> {
        self.lock().keyring.get(&entry.to_string()).cloned()
    }

    /// A machine with no keyring service to ask, as a server with no
    /// Secret Service: every keyring entry fails as `KeyringUnavailable`.
    pub fn remove_keyring_service(&self) {
        self.lock().no_keyring_service = true;
    }

    /// A keyring that is locked, as a Secret Service whose collection
    /// nobody unlocked: every keyring entry, there or not, fails as
    /// `KeyringUnavailable`, never as missing.
    pub fn lock_keyring(&self) {
        self.lock().keyring_locked = true;
    }

    /// A keyring that is locked until `clock` reaches `at`, as a login
    /// keyring nobody has unlocked while the machine waits at a login
    /// screen: until then every keyring entry fails as `KeyringUnavailable`
    /// locked, and asking whether the keyring is locked says locked (#1221).
    pub fn lock_keyring_until(&self, clock: Arc<dyn Clock>, at: Timestamp) {
        self.lock().unlock_at = Some((clock, at));
    }

    /// A keyring that is locked for the first `reads` reads and unlocked
    /// from the next, as one a person unlocks just after the daemon first
    /// found it locked: no clock decides when, so a test can have the
    /// unlock fall between two reads that follow each other at once.
    pub fn lock_keyring_for_reads(&self, reads: usize) {
        self.lock().unlocks_after_reads = Some(reads);
    }

    /// A keyring whose unlock prompt the person dismissed: every keyring
    /// entry, there or not, fails as `KeyringUnavailable`, dismissed.
    pub fn dismiss_unlock_prompt(&self) {
        self.lock().unlock_prompt_dismissed = true;
    }

    /// A keyring service with no collection to look in, as one no default
    /// collection is set for: every keyring entry, there or not, fails as
    /// `KeyringUnavailable`, refused.
    pub fn remove_default_collection(&self) {
        self.lock().no_default_collection = true;
    }

    /// A keyring that answers it is unlocked while its entries stay locked,
    /// as one whose default collection is unlocked while the entry sits in
    /// another that isn't: every keyring entry fails as `KeyringUnavailable`
    /// locked, and asking whether the keyring is locked says unlocked.
    pub fn keep_entries_locked(&self) {
        self.lock().entries_stay_locked = true;
    }

    /// A session whose keyring service starts when `clock` reaches `at`, as
    /// one that starts after the daemon at login: until then every keyring
    /// entry fails as `KeyringUnavailable` with no service to ask.
    pub fn start_keyring_service_at(&self, clock: Arc<dyn Clock>, at: Timestamp) {
        self.lock().service_starts = Some((clock, at));
    }

    /// A session whose keyring service goes away when `clock` reaches `at`,
    /// as one that stops under a run: from then every keyring entry fails as
    /// `KeyringUnavailable` with no service to ask.
    pub fn end_keyring_service_at(&self, clock: Arc<dyn Clock>, at: Timestamp) {
        self.lock().service_ends = Some((clock, at));
    }

    /// A service that is slow to say whether the keyring is locked: its
    /// answer is the one it was when asked, and arrives `by` later on
    /// `clock`, as one from a service that answers within the limit but not
    /// at once.
    pub fn answer_lock_questions_after(&self, clock: Arc<dyn Clock>, by: domain::SignedDuration) {
        self.lock().lock_answer_takes = Some((clock, by));
    }

    /// A keyring that doesn't answer, as a macOS keychain waiting on a
    /// prompt nobody answered: every keyring entry fails as `TimedOut`.
    pub fn stall_keyring(&self) {
        self.lock().keyring_stalls = true;
    }

    /// How the keyring service is started, as `activation` then answers.
    pub fn set_keyring_activation(&self, activation: KeyringActivation) {
        self.lock().activation = Some(activation);
    }

    /// A file at `path` whose read doesn't answer, as one on a network
    /// mount that hung: reading it fails as `TimedOut`.
    pub fn stall_file(&self, path: &str) {
        self.lock().stalled_files.insert(path.to_owned());
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

    /// How many times the resolver was asked whether the keyring is locked,
    /// as a run waiting for the keyring to be unlocked asks (#1221).
    pub fn lock_query_count(&self) -> usize {
        self.lock().lock_queries
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
                if state.stalled_files.contains(&real) {
                    return Err(SecretError {
                        source,
                        problem: Problem::TimedOut {
                            seconds: FAKE_TIME_LIMIT_SECONDS,
                        },
                    });
                }
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
                if state.keyring_stalls {
                    return Err(SecretError {
                        source,
                        problem: Problem::TimedOut {
                            seconds: FAKE_TIME_LIMIT_SECONDS,
                        },
                    });
                }
                let unavailable = keyring_unavailable(&state).or_else(|| {
                    state
                        .entries_stay_locked
                        .then_some(("the item is locked", KeyringCause::Locked))
                });
                if let Some((detail, cause)) = unavailable {
                    // A run that spins on a keyring it can't ask would hang
                    // its test: past the cap, the read fails otherwise, so
                    // the test ends and says so.
                    let reads = state.reads.iter().filter(|read| *read == secret).count();
                    if reads > FAKE_UNAVAILABLE_READS {
                        return Err(SecretError {
                            source,
                            problem: Problem::Unreachable {
                                detail: "read without end while it can't be asked".to_owned(),
                            },
                        });
                    }
                    return Err(SecretError {
                        source,
                        problem: Problem::KeyringUnavailable {
                            detail: detail.to_owned(),
                            cause,
                        },
                    });
                }
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

    async fn keyring_lock(&self) -> KeyringLock {
        let (answer, slow) = {
            let mut state = self.lock();
            state.lock_queries += 1;
            let answer = match keyring_unavailable(&state) {
                Some((_, KeyringCause::NoService | KeyringCause::Refused)) => KeyringLock::Unknown,
                // A dismissed unlock prompt leaves the collection locked, and a
                // run refused that way never waits to ask.
                Some((_, KeyringCause::Locked | KeyringCause::Dismissed)) => KeyringLock::Locked,
                None => KeyringLock::Unlocked,
            };
            (answer, state.lock_answer_takes.clone())
        };
        if let Some((clock, by)) = slow {
            clock.sleep(by).await;
        }
        answer
    }
}

#[async_trait]
impl Keyring for FakeSecrets {
    async fn keep(
        &self,
        entry: &KeyringEntry,
        password: &domain::secrets::SecretValue,
    ) -> Result<Kept, KeyringError> {
        let mut state = self.lock();
        unavailable(&state)?;
        match state.keyring.get(&entry.to_string()) {
            Some(held) if held == password.expose() => Ok(Kept::AlreadyThere),
            Some(_) => Err(KeyringError::Taken),
            None => {
                state
                    .keyring
                    .insert(entry.to_string(), password.expose().to_owned());
                Ok(Kept::Added)
            }
        }
    }

    async fn remove(&self, entry: &KeyringEntry) -> Result<(), KeyringError> {
        let mut state = self.lock();
        unavailable(&state)?;
        state.keyring.remove(&entry.to_string());
        Ok(())
    }

    async fn activation(&self) -> KeyringActivation {
        let state = self.lock();
        match state.activation {
            Some(activation) => activation,
            None if state.no_keyring_service => KeyringActivation::Absent,
            None => KeyringActivation::OnDemand,
        }
    }
}

/// A keyring with no service to ask, a locked one or one that refused, as
/// the resolver reads it: what keeping or removing an entry is refused with
/// there.
fn unavailable(state: &State) -> Result<(), KeyringError> {
    match keyring_unavailable(state) {
        Some((detail, KeyringCause::Locked | KeyringCause::Dismissed)) => {
            Err(KeyringError::Locked {
                detail: detail.to_owned(),
            })
        }
        Some((detail, KeyringCause::NoService | KeyringCause::Refused)) => {
            Err(KeyringError::Unavailable {
                detail: detail.to_owned(),
            })
        }
        None => Ok(()),
    }
}

/// Why the keyring can't be asked, in the platform's words, if it can't.
fn keyring_unavailable(state: &State) -> Option<(&'static str, KeyringCause)> {
    let not_started = state
        .service_starts
        .as_ref()
        .is_some_and(|(clock, at)| clock.now() < *at);
    let ended = state
        .service_ends
        .as_ref()
        .is_some_and(|(clock, at)| clock.now() >= *at);
    if state.no_keyring_service || not_started || ended {
        Some(("no session bus", KeyringCause::NoService))
    } else if state.keyring_locked
        || state
            .unlock_at
            .as_ref()
            .is_some_and(|(clock, at)| clock.now() < *at)
        || state
            .unlocks_after_reads
            .is_some_and(|reads| state.reads.len() <= reads)
    {
        Some(("the keyring is locked", KeyringCause::Locked))
    } else if state.unlock_prompt_dismissed {
        Some(("the unlock prompt was dismissed", KeyringCause::Dismissed))
    } else if state.no_default_collection {
        Some(("no default collection", KeyringCause::Refused))
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;
    use domain::secrets::KeyringEntry;

    /// A keyring that can't be asked says so for as many reads as any hold
    /// makes, then fails the read otherwise, so a run that spins ends its
    /// test rather than hanging it.
    #[tokio::test]
    async fn a_keyring_read_without_end_stops_saying_it_cant_be_asked() {
        let secrets = FakeSecrets::new();
        secrets.remove_keyring_service();
        let entry = KeyringEntry::parse("ladderback-demo/nas").unwrap();
        let other = SecretRef::Keyring(KeyringEntry::parse("ladderback-demo/other").unwrap());
        let _ = secrets.resolve(&other).await;
        let secret = SecretRef::Keyring(entry);
        for read in 1..=FAKE_UNAVAILABLE_READS {
            assert!(
                matches!(
                    secrets.resolve(&secret).await,
                    Err(SecretError {
                        problem: Problem::KeyringUnavailable { .. },
                        ..
                    })
                ),
                "read {read}"
            );
        }
        assert!(matches!(
            secrets.resolve(&secret).await,
            Err(SecretError {
                problem: Problem::Unreachable { .. },
                ..
            })
        ));
    }

    /// Unless a test says how the keyring service is started, it is started
    /// on demand, and a machine with no keyring service has none to start.
    #[tokio::test]
    async fn a_keyring_service_is_started_on_demand_unless_there_is_none() {
        let secrets = FakeSecrets::new();
        assert_eq!(secrets.activation().await, KeyringActivation::OnDemand);
        secrets.remove_keyring_service();
        assert_eq!(secrets.activation().await, KeyringActivation::Absent);
    }

    /// A keyring locked for its first reads fails each of them as locked,
    /// says it is locked before them, and gives the entry from the next.
    #[tokio::test]
    async fn a_keyring_locked_for_its_first_reads_gives_the_entry_at_the_next() {
        let secrets = FakeSecrets::new();
        let entry = KeyringEntry::parse("ladderback-demo/nas").unwrap();
        secrets.set_keyring_entry(&entry, "canary-secret");
        secrets.lock_keyring_for_reads(1);
        assert_eq!(secrets.keyring_lock().await, KeyringLock::Locked);
        let secret = SecretRef::Keyring(entry);
        let first = secrets.resolve(&secret).await;
        assert!(
            matches!(
                first,
                Err(SecretError {
                    problem: Problem::KeyringUnavailable {
                        cause: KeyringCause::Locked,
                        ..
                    },
                    ..
                })
            ),
            "{first:?}"
        );
        let next = secrets.resolve(&secret).await;
        assert!(next.is_ok(), "{next:?}");
    }

    /// A keyring service started at a time answers from that time on, and
    /// not before.
    #[tokio::test]
    async fn a_keyring_service_answers_from_the_moment_it_starts() {
        let start = domain::Timestamp::from_second(1_800_000_000).unwrap();
        let clock = Arc::new(crate::fakes::FakeClock::new(start));
        let secrets = FakeSecrets::new();
        let wait = domain::SignedDuration::from_secs(60);
        secrets.start_keyring_service_at(clock.clone(), start + wait);
        assert_eq!(secrets.keyring_lock().await, KeyringLock::Unknown);
        clock.advance(wait);
        assert_eq!(secrets.keyring_lock().await, KeyringLock::Unlocked);
    }

    /// A keyring locked until a time gives its entries from that time on,
    /// and not before, as the scenarios that unlock it at a moment read it.
    #[tokio::test]
    async fn a_keyring_locked_until_a_time_gives_the_entry_from_that_time() {
        let start = domain::Timestamp::from_second(1_800_000_000).unwrap();
        let clock = Arc::new(crate::fakes::FakeClock::new(start));
        let secrets = FakeSecrets::new();
        let entry = KeyringEntry::parse("ladderback-demo/nas").unwrap();
        secrets.set_keyring_entry(&entry, "canary-secret");
        let wait = domain::SignedDuration::from_secs(60);
        secrets.lock_keyring_until(clock.clone(), start + wait);
        let secret = SecretRef::Keyring(entry);
        assert_eq!(secrets.keyring_lock().await, KeyringLock::Locked);
        let before = secrets.resolve(&secret).await;
        assert!(
            matches!(
                before,
                Err(SecretError {
                    problem: Problem::KeyringUnavailable {
                        cause: KeyringCause::Locked,
                        ..
                    },
                    ..
                })
            ),
            "{before:?}"
        );
        clock.advance(wait);
        assert_eq!(secrets.keyring_lock().await, KeyringLock::Unlocked);
        let at = secrets.resolve(&secret).await;
        assert!(at.is_ok(), "{at:?}");
    }
}
