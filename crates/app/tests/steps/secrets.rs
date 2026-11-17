//! Steps for `features/secrets/*.feature`: they drive `ResolveRepository`
//! over a `FakeSecrets` store held by the world's `secrets` field.

use std::assert_matches;
use std::sync::{Arc, Mutex, PoisonError};

use app::execution::RunOperation;
use app::fakes::{FakeClock, FakeSecrets, Script};
use app::keyring_check::{KeyringCheck, KeyringRead, KeyringUser, check_keyring_entries};
use app::ports::Clock;
use app::retry::{RunEnd, RunOutcome};
use app::secrets::ResolvedRepository;
use app::secrets::{Cause, Field, KeyringWait, ResolveError, ResolveRepository};
use cucumber::{given, then, when};
use domain::execution::refusal::SettingRefusal;
use domain::execution::{FailureReason, OperationStatus, Target, Trigger};
use domain::notifications::{Auth, Channel, ChannelKind, ChannelName, Notifications};
use domain::planning::{GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings};
use domain::secrets::{
    CommandLine, FileRead, KeyringCause, KeyringEntry, MAX_MASKED_BYTES, MAX_SECRET_BYTES, Problem,
    Redactor, SecretError, SecretRef, SecretValue, SecretWarning, SourceName, TemplateError,
};
use domain::{OperationId, PlanId, RepositoryAlias, SignedDuration, Timestamp, Uuid};

use super::Agent;

/// The store, the settings being built, what resolving them gave, and the
/// warnings the resolution told, as the daemon's log hears them.
pub struct SecretsWorld {
    store: Arc<FakeSecrets>,
    settings: RepositorySettings,
    outcome: Option<Result<ResolvedRepository, ResolveError>>,
    told: Arc<Mutex<Vec<SecretWarning>>>,
    /// When the daemon started, by the world's clock.
    started: Option<Timestamp>,
    /// When the daemon started, by the world's clock's monotonic time.
    up_from: Option<SignedDuration>,
    /// Each keyring entry the daemon was told to log that a run waits for.
    waited: Arc<Mutex<Vec<SourceName>>>,
    /// When the wait of a run whose daemon stopped meanwhile ended.
    stopped_at: Option<Timestamp>,
    /// The repositories whose keyring entries a check reads, by alias.
    checked_repositories: std::collections::BTreeMap<RepositoryAlias, RepositoryConfig>,
    /// The notification channels whose keyring entries a check reads.
    checked_channels: std::collections::BTreeMap<ChannelName, Channel>,
    /// What the check of the keyring entries came to.
    keyring_check: Option<KeyringCheck>,
}

impl Default for SecretsWorld {
    fn default() -> Self {
        Self {
            store: Arc::new(FakeSecrets::new()),
            settings: RepositorySettings {
                repository: String::new(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new("")),
                options: Default::default(),
                options_hot: Default::default(),
                options_cold: Default::default(),
            },
            outcome: None,
            told: Arc::default(),
            started: None,
            up_from: None,
            waited: Arc::default(),
            stopped_at: None,
            checked_repositories: Default::default(),
            checked_channels: Default::default(),
            keyring_check: None,
        }
    }
}

impl SecretsWorld {
    fn resolved(&self) -> &ResolvedRepository {
        match self.outcome.as_ref().expect("the repository was resolved") {
            Ok(resolved) => resolved,
            Err(error) => panic!("the resolution failed: {error:?}"),
        }
    }

    fn error(&self) -> &ResolveError {
        match self.outcome.as_ref().expect("the repository was resolved") {
            Ok(resolved) => panic!("the resolution succeeded: {resolved:?}"),
            Err(error) => error,
        }
    }

    fn unresolved(&self) -> &SecretError {
        match &self.error().cause {
            Cause::Unresolved(error) => error,
            other => panic!("a secret was expected to fail, not {other:?}"),
        }
    }

    fn cause(&self) -> &Cause {
        &self.error().cause
    }

    fn command(&self, text: &str) -> CommandLine {
        CommandLine::parse(text).expect("the scenario's command is valid")
    }

    fn script(&self, command: &str, script: Script) {
        self.store.script(&self.command(command), script);
    }
}

/// Scenarios write a line break as `\n` and a carriage return as `\r`.
fn text(text: &str) -> String {
    text.replace("\\n", "\n").replace("\\r", "\r")
}

/// Text of exactly `bytes` bytes.
fn filler(bytes: usize) -> String {
    "a".repeat(bytes)
}

/// A location of exactly `bytes` bytes: a full path, which a job takes.
fn location_filler(bytes: usize) -> String {
    format!("/{}", filler(bytes.saturating_sub(1)))
}

/// Bytes that aren't UTF-8.
const NOT_TEXT: &[u8] = &[0xff, 0xfe, 0xfd];

/// The `Field` a scenario names as the config does: `password`,
/// `repository`, `repo-hot`, `options.<name>`, `options-hot.<name>` or
/// `options-cold.<name>`.
fn field(name: &str) -> Field {
    match name.split_once('.') {
        None if name == "password" => Field::Password,
        None if name == "repository" => Field::Repository,
        None if name == "repo-hot" => Field::RepoHot,
        Some(("options", option)) => Field::Option(option.to_owned()),
        Some(("options-hot", option)) => Field::OptionHot(option.to_owned()),
        Some(("options-cold", option)) => Field::OptionCold(option.to_owned()),
        _ => panic!("unknown field {name:?}"),
    }
}

// What the secrets store holds.

#[given(expr = "the environment variable {string} is {string}")]
fn variable_is(agent: &mut Agent, name: String, value: String) {
    agent.secrets.store.set_env(&name, &text(&value));
}

#[given(expr = "the environment variable {string} is empty")]
fn variable_is_empty(agent: &mut Agent, name: String) {
    agent.secrets.store.set_env(&name, "");
}

#[given(expr = "the environment variable {string} holds {int} bytes of text")]
fn variable_holds(agent: &mut Agent, name: String, bytes: usize) {
    agent.secrets.store.set_env(&name, &filler(bytes));
}

#[given(expr = "the environment variable {string} is not set")]
fn variable_is_not_set(_agent: &mut Agent, _name: String) {
    // Nothing is set until a scenario sets it.
}

#[given(expr = "the environment variables {string} are set")]
fn variables_are_set(agent: &mut Agent, names: String) {
    for name in names.split(", ").filter(|name| !name.is_empty()) {
        agent.secrets.store.set_env(name, "demo-value");
    }
}

#[given(expr = "the file {string} holds {string}")]
fn file_holds(agent: &mut Agent, path: String, contents: String) {
    agent
        .secrets
        .store
        .write_file(&path, text(&contents).as_bytes(), 0o600);
}

#[given(expr = "the file {string} with mode {word} holds {string}")]
fn file_with_mode_holds(agent: &mut Agent, path: String, mode: String, contents: String) {
    let mode = u32::from_str_radix(&mode, 8).expect("the mode is octal");
    agent
        .secrets
        .store
        .write_file(&path, text(&contents).as_bytes(), mode);
}

#[given(expr = "the file {string} holds {int} bytes of text")]
fn file_holds_bytes(agent: &mut Agent, path: String, bytes: usize) {
    agent
        .secrets
        .store
        .write_file(&path, filler(bytes).as_bytes(), 0o600);
}

#[given(expr = "the file {string} holds bytes that aren't text")]
fn file_holds_binary(agent: &mut Agent, path: String) {
    agent.secrets.store.write_file(&path, NOT_TEXT, 0o600);
}

#[given(expr = "the file {string} is a directory")]
fn file_is_directory(agent: &mut Agent, path: String) {
    agent.secrets.store.make_directory(&path);
}

#[given(expr = "the file {string} is a link to {string}")]
fn file_is_link(agent: &mut Agent, path: String, target: String) {
    agent.secrets.store.link(&path, &target);
}

#[given(expr = "the keyring holds {string} under {string}")]
fn keyring_holds(agent: &mut Agent, secret: String, entry: String) {
    let entry = KeyringEntry::parse(&entry).expect("the scenario's entry is valid");
    agent.secrets.store.set_keyring_entry(&entry, &secret);
}

#[given("the machine has no keyring service")]
fn no_keyring_service(agent: &mut Agent) {
    agent.secrets.store.remove_keyring_service();
}

#[given("the keyring is locked")]
fn keyring_locked(agent: &mut Agent) {
    agent.secrets.store.lock_keyring();
}

#[given("the keyring's unlock prompt was dismissed")]
fn unlock_prompt_dismissed(agent: &mut Agent) {
    agent.secrets.store.dismiss_unlock_prompt();
}

#[given("the keyring doesn't answer in time")]
fn keyring_stalls(agent: &mut Agent) {
    agent.secrets.store.stall_keyring();
}

#[given(expr = "the file {string} doesn't answer in time")]
fn file_stalls(agent: &mut Agent, path: String) {
    agent.secrets.store.stall_file(&path);
}

#[given(expr = "the command {string} prints {string}")]
fn command_prints(agent: &mut Agent, command: String, stdout: String) {
    let stdout = text(&stdout).into_bytes();
    agent.secrets.script(
        &command,
        Script::Exits {
            status: 0,
            stdout,
            stderr: Vec::new(),
        },
    );
}

#[given(expr = "the command {string} prints {int} bytes of text")]
fn command_prints_bytes(agent: &mut Agent, command: String, bytes: usize) {
    agent.secrets.script(
        &command,
        Script::Exits {
            status: 0,
            stdout: filler(bytes).into_bytes(),
            stderr: Vec::new(),
        },
    );
}

#[given(expr = "the command {string} prints bytes that aren't text")]
fn command_prints_binary(agent: &mut Agent, command: String) {
    agent.secrets.script(
        &command,
        Script::Exits {
            status: 0,
            stdout: NOT_TEXT.to_vec(),
            stderr: Vec::new(),
        },
    );
}

#[given(expr = "the command {string} prints {string} and fails with status {int} saying {string}")]
fn command_fails(agent: &mut Agent, command: String, stdout: String, status: i32, stderr: String) {
    agent.secrets.script(
        &command,
        Script::Exits {
            status,
            stdout: text(&stdout).into_bytes(),
            stderr: text(&stderr).into_bytes(),
        },
    );
}

#[given(
    expr = "the command {string} prints {string} while something it started holds its output open"
)]
fn command_prints_held_open(agent: &mut Agent, command: String, stdout: String) {
    agent.secrets.script(
        &command,
        Script::LeavesOutputOpen {
            status: 0,
            stdout: text(&stdout).into_bytes(),
            stderr: Vec::new(),
        },
    );
}

#[given(expr = "the command {string} never ends")]
fn command_hangs(agent: &mut Agent, command: String) {
    agent.secrets.script(&command, Script::Hangs);
}

// The repository's settings.

#[given(expr = "the repository is {string}")]
fn repository_is(agent: &mut Agent, repository: String) {
    agent.secrets.settings.repository = repository;
}

#[given(expr = "the repository is {int} bytes of text")]
fn repository_is_bytes(agent: &mut Agent, bytes: usize) {
    agent.secrets.settings.repository = location_filler(bytes);
}

#[given(expr = "the repository is {int} references to the variable {string}")]
fn repository_is_references(agent: &mut Agent, count: usize, name: String) {
    agent.secrets.settings.repository = format!("${{{name}}}").repeat(count);
}

#[given(expr = "the repository is {int} bytes of text followed by {string}")]
fn repository_is_bytes_then(agent: &mut Agent, bytes: usize, rest: String) {
    agent.secrets.settings.repository = location_filler(bytes) + &rest;
}

/// `count` environment variables `DEMO_SECRET_1` on, each a different
/// secret, referred to by the options `note-1` on, so that they and the
/// inline password come to `total` bytes.
#[given(
    expr = "different secrets that come to {int} bytes with the password, in the options \"note-1\" to \"note-{int}\""
)]
fn secrets_in_options(agent: &mut Agent, total: usize, count: usize) {
    let password = agent
        .secrets
        .settings
        .password
        .inline()
        .map_or(0, |password| password.expose().len());
    let rest = total - password;
    for at in 1..=count {
        let share = rest / count + usize::from(at <= rest % count);
        assert!(
            share <= MAX_SECRET_BYTES,
            "{share} bytes is too large for one secret"
        );
        let name = format!("DEMO_SECRET_{at}");
        // Each of its own letter, so that no two are the same secret.
        let letter = char::from_digit(u32::try_from(at + 9).unwrap(), 36).unwrap();
        agent
            .secrets
            .store
            .set_env(&name, &letter.to_string().repeat(share));
        agent
            .secrets
            .settings
            .options
            .insert(format!("note-{at}"), format!("${{{name}}}"));
    }
}

#[given(expr = "the hot repository is {string}")]
fn hot_repository_is(agent: &mut Agent, repository: String) {
    agent.secrets.settings.repo_hot = Some(repository);
}

/// An rclone connection string with `count` credentials, each a setting of
/// its own: `rclone::sftp,host=nas.example.net,pass=demo-pass-0,…:backups`.
fn rclone_credentials(count: usize) -> String {
    let settings: String = (0..count).map(|n| format!(",pass=demo-pass-{n}")).collect();
    format!("rclone::sftp,host=nas.example.net{settings}:backups")
}

#[given(expr = "the repository is an rclone connection string with {int} credentials")]
fn repository_with_credentials(agent: &mut Agent, count: usize) {
    agent.secrets.settings.repository = rclone_credentials(count);
}

#[given(expr = "the hot repository is an rclone connection string with {int} credentials")]
fn hot_repository_with_credentials(agent: &mut Agent, count: usize) {
    agent.secrets.settings.repo_hot = Some(rclone_credentials(count));
}

#[given(expr = "the option {string} is {string}")]
fn option_is(agent: &mut Agent, name: String, value: String) {
    agent.secrets.settings.options.insert(name, value);
}

#[given(expr = "the hot option {string} is {string}")]
fn hot_option_is(agent: &mut Agent, name: String, value: String) {
    agent.secrets.settings.options_hot.insert(name, value);
}

#[given(expr = "the cold option {string} is {string}")]
fn cold_option_is(agent: &mut Agent, name: String, value: String) {
    agent.secrets.settings.options_cold.insert(name, value);
}

fn password_from(agent: &mut Agent, secret: SecretRef) {
    agent.secrets.settings.password = secret;
}

#[given(expr = "the repository's password is {string}")]
fn password_is(agent: &mut Agent, password: String) {
    password_from(agent, SecretRef::Inline(SecretValue::new(password)));
}

#[given(expr = "the repository's password is in the environment variable {string}")]
fn password_in_variable(agent: &mut Agent, name: String) {
    password_from(agent, SecretRef::Env(name));
}

#[given(expr = "the repository's password is in the file {string}")]
fn password_in_file(agent: &mut Agent, path: String) {
    let read = FileRead::FirstLine;
    password_from(agent, SecretRef::File { path, read });
}

#[given(expr = "the repository's password is printed by the command {string}")]
fn password_from_command(agent: &mut Agent, command: String) {
    let command = agent.secrets.command(&command);
    password_from(agent, SecretRef::Command(command));
}

#[given(expr = "the repository's password is in the keyring under {string}")]
fn password_in_keyring(agent: &mut Agent, entry: String) {
    let entry = KeyringEntry::parse(&entry).expect("the scenario's entry is valid");
    password_from(agent, SecretRef::Keyring(entry));
}

#[when("the repository is resolved")]
async fn resolve(agent: &mut Agent) {
    let told = Arc::clone(&agent.secrets.told);
    let use_case =
        ResolveRepository::new(agent.secrets.store.clone()).telling(Arc::new(move |warning| {
            told.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(warning.clone());
        }));
    agent.secrets.outcome = Some(use_case.resolve(&agent.secrets.settings).await);
}

// What resolving gave.

#[then(expr = "the resolved password is {string}")]
fn resolved_password(agent: &mut Agent, password: String) {
    let resolved = agent.secrets.resolved();
    assert_eq!(resolved.access.password.expose(), text(&password));
}

#[then(expr = "the resolved repository is {string}")]
fn resolved_repository(agent: &mut Agent, repository: String) {
    assert_eq!(agent.secrets.resolved().access.repository, repository);
}

#[then(expr = "the resolved repository has {int} bytes")]
fn resolved_repository_size(agent: &mut Agent, bytes: usize) {
    assert_eq!(agent.secrets.resolved().access.repository.len(), bytes);
}

#[then(expr = "the resolved hot repository is {string}")]
fn resolved_hot_repository(agent: &mut Agent, repository: String) {
    assert_eq!(
        agent.secrets.resolved().access.repo_hot.as_deref(),
        Some(repository.as_str())
    );
}

#[then(expr = "the resolved option {string} is {string}")]
fn resolved_option(agent: &mut Agent, name: String, value: String) {
    let options = &agent.secrets.resolved().access.options;
    assert_eq!(options.get(&name), Some(&text(&value)));
}

#[then(expr = "the resolved option {string} has {int} bytes")]
fn resolved_option_size(agent: &mut Agent, name: String, bytes: usize) {
    let options = &agent.secrets.resolved().access.options;
    assert_eq!(options.get(&name).map(String::len), Some(bytes));
}

#[then(expr = "the resolved hot option {string} is {string}")]
fn resolved_hot_option(agent: &mut Agent, name: String, value: String) {
    let options = &agent.secrets.resolved().access.options_hot;
    assert_eq!(options.get(&name), Some(&value));
}

#[then(expr = "the resolved cold option {string} is {string}")]
fn resolved_cold_option(agent: &mut Agent, name: String, value: String) {
    let options = &agent.secrets.resolved().access.options_cold;
    assert_eq!(options.get(&name), Some(&value));
}

#[then(expr = "the resolved options are {int}")]
fn resolved_option_count(agent: &mut Agent, count: usize) {
    assert_eq!(agent.secrets.resolved().access.options.len(), count);
}

// Why resolving failed.

#[then(expr = "the resolution fails for the field {string}")]
fn fails_for(agent: &mut Agent, name: String) {
    assert_eq!(agent.secrets.error().field, field(&name));
}

/// `kind` names what is wrong with the reference at byte `at`.
#[then(expr = "the field is invalid because of its {word} reference at byte {int}")]
fn invalid_reference_at(agent: &mut Agent, kind: String, at: usize) {
    let expected = match kind.as_str() {
        "unterminated" => TemplateError::Unterminated { at },
        "empty" => TemplateError::EmptyReference { at },
        "misnamed" => TemplateError::InvalidName { at },
        "nested" => TemplateError::NestedReference { at },
        "pathless" => TemplateError::EmptyPath { at },
        "relative" => TemplateError::RelativePath { at },
        _ => panic!("unknown reference error {kind:?}"),
    };
    assert_eq!(agent.secrets.cause(), &Cause::Invalid(expected));
}

#[then(expr = "the field is invalid because it is longer than {int} bytes")]
fn invalid_too_long(agent: &mut Agent, limit: usize) {
    assert_eq!(
        agent.secrets.cause(),
        &Cause::Invalid(TemplateError::TooLong { limit })
    );
}

#[then(expr = "the field is invalid because it has more than {int} references")]
fn invalid_too_many(agent: &mut Agent, limit: usize) {
    assert_eq!(
        agent.secrets.cause(),
        &Cause::Invalid(TemplateError::TooManyReferences { limit })
    );
}

#[then(expr = "the references expand to more than {int} bytes")]
fn expand_too_long(agent: &mut Agent, limit: usize) {
    assert_eq!(agent.secrets.cause(), &Cause::ExpansionTooLong { limit });
    assert_eq!(limit, MAX_SECRET_BYTES);
}

#[then(expr = "the secrets are refused because together they come to more than {int} bytes")]
fn too_much_to_mask(agent: &mut Agent, limit: usize) {
    assert_eq!(agent.secrets.cause(), &Cause::TooMuchToMask { limit });
    assert_eq!(limit, MAX_MASKED_BYTES);
}

#[then("the reference is refused because it would reach the arguments of a process")]
fn refused(agent: &mut Agent) {
    assert_eq!(agent.secrets.cause(), &Cause::ReferenceInProcessArguments);
}

#[then(expr = "the field is refused because it holds more than {int} credentials")]
fn too_many_credentials(agent: &mut Agent, limit: usize) {
    assert_eq!(agent.secrets.cause(), &Cause::TooManyCredentials { limit });
}

#[then("the location is refused because it is not a full path on this machine")]
fn relative_location(agent: &mut Agent) {
    assert_eq!(agent.secrets.cause(), &Cause::RelativePath);
}

#[then(expr = "the failure names the variable {string}")]
fn names_variable(agent: &mut Agent, name: String) {
    assert_eq!(agent.secrets.unresolved().source, SourceName::Env { name });
}

#[then(expr = "the failure names the file {string}")]
fn names_file(agent: &mut Agent, path: String) {
    assert_eq!(agent.secrets.unresolved().source, SourceName::File { path });
}

#[then(expr = "the failure names the program {string}")]
fn names_program(agent: &mut Agent, program: String) {
    assert_eq!(
        agent.secrets.unresolved().source,
        SourceName::Command { program }
    );
}

#[then(expr = "the failure names the keyring entry {string}")]
fn names_entry(agent: &mut Agent, entry: String) {
    assert_eq!(
        agent.secrets.unresolved().source,
        SourceName::Keyring { entry }
    );
}

#[then("the problem is that it is missing")]
fn problem_missing(agent: &mut Agent) {
    assert_eq!(agent.secrets.unresolved().problem, Problem::Missing);
}

#[then("the problem is that it isn't text")]
fn problem_not_text(agent: &mut Agent) {
    assert_eq!(agent.secrets.unresolved().problem, Problem::NotUnicode);
}

#[then("the problem is that it can't be reached")]
fn problem_unreachable(agent: &mut Agent) {
    assert_matches!(
        agent.secrets.unresolved().problem,
        Problem::Unreachable { .. },
        "{:?}",
        agent.secrets.unresolved()
    );
}

#[then("the problem is that there is no keyring service to ask")]
fn problem_no_keyring_service(agent: &mut Agent) {
    assert_eq!(agent.secrets.keyring_cause(), KeyringCause::NoService);
}

#[then("the problem is that the keyring is locked")]
fn problem_keyring_locked(agent: &mut Agent) {
    assert_eq!(agent.secrets.keyring_cause(), KeyringCause::Locked);
}

#[then(expr = "the problem is that it is larger than {int} bytes")]
fn problem_too_large(agent: &mut Agent, limit: usize) {
    assert_eq!(
        agent.secrets.unresolved().problem,
        Problem::TooLarge { limit }
    );
}

#[then(expr = "the problem is that the command exited with status {int} saying {string}")]
fn problem_command_failed(agent: &mut Agent, status: i32, detail: String) {
    assert_eq!(
        agent.secrets.unresolved().problem,
        Problem::CommandFailed {
            status: Some(status),
            detail
        }
    );
}

#[then(
    expr = "the problem is that the command exited with status {int}, and nothing it said is shown"
)]
fn problem_command_failed_unquoted(agent: &mut Agent, status: i32) {
    assert_eq!(
        agent.secrets.unresolved().problem,
        Problem::CommandFailed {
            status: Some(status),
            detail: String::new(),
        }
    );
}

#[then("the problem is that its output was cut short")]
fn problem_output_cut(agent: &mut Agent) {
    assert_eq!(agent.secrets.unresolved().problem, Problem::OutputCut);
}

#[then("the problem is that the command took too long")]
fn problem_timed_out(agent: &mut Agent) {
    assert_matches!(
        agent.secrets.unresolved().problem,
        Problem::TimedOut { seconds } if seconds > 0,
        "{:?}",
        agent.secrets.unresolved()
    );
}

#[then(expr = "the failure doesn't mention {string}")]
fn failure_hides(agent: &mut Agent, value: String) {
    let error = agent.secrets.error();
    let shown = format!("{error:?} {error}");
    assert!(!shown.contains(&value), "{shown}");
}

// Warnings, and how often secrets were read.

#[then(expr = "the resolution warns once that others can read the file {string}")]
fn warns_once(agent: &mut Agent, path: String) {
    let warnings = &agent.secrets.resolved().warnings;
    let named = |w: &&SecretWarning| matches!(w, SecretWarning::FileReadableByOthers { path: at, .. } if *at == path);
    assert_eq!(warnings.iter().filter(named).count(), 1);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[then("the resolution warns about nothing")]
fn warns_about_nothing(agent: &mut Agent) {
    assert_eq!(agent.secrets.resolved().warnings, []);
}

#[then(expr = "the log is told once that others can read the file {string}")]
fn log_told_once(agent: &mut Agent, path: String) {
    let told = agent
        .secrets
        .told
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_matches!(
        told.as_slice(),
        [SecretWarning::FileReadableByOthers { path: at, .. }] if *at == path,
        "{told:?}"
    );
}

#[then(expr = "the variable {string} was read {int} time(s)")]
fn variable_reads(agent: &mut Agent, name: String, times: usize) {
    let secret = SecretRef::Env(name);
    assert_eq!(agent.secrets.store.read_count(&secret), times);
}

#[then("only the password was read")]
fn only_password_read(agent: &mut Agent) {
    let password = agent.secrets.settings.password.clone();
    assert_eq!(agent.secrets.store.reads(), [password]);
}

#[then("nothing was read")]
fn nothing_read(agent: &mut Agent) {
    assert_eq!(agent.secrets.store.reads(), []);
}

#[then(expr = "the file {string} was read {int} time(s)")]
fn file_reads(agent: &mut Agent, path: String, times: usize) {
    let read = FileRead::Whole;
    let secret = SecretRef::File { path, read };
    assert_eq!(agent.secrets.store.read_count(&secret), times);
}

// What the engine is told to hide, and what a debug form shows.

#[then(expr = "the access lists the secret {string}")]
fn lists(agent: &mut Agent, value: String) {
    let secrets = &agent.secrets.resolved().access.secrets;
    assert!(
        secrets.iter().any(|secret| secret.expose() == value),
        "{} secrets, none of them {value:?}",
        secrets.len()
    );
}

#[then(expr = "the access doesn't list the secret {string}")]
fn doesnt_list(agent: &mut Agent, value: String) {
    let secrets = &agent.secrets.resolved().access.secrets;
    assert!(secrets.iter().all(|secret| secret.expose() != value));
}

#[then(expr = "the access lists {int} secret(s)")]
fn lists_count(agent: &mut Agent, count: usize) {
    assert_eq!(agent.secrets.resolved().access.secrets.len(), count);
}

fn assert_hides(debug: &str, value: &str) {
    assert!(!debug.contains(value), "{debug}");
}

#[then(expr = "the debug form of the access shows no {string}")]
fn access_hides(agent: &mut Agent, value: String) {
    assert_hides(&format!("{:?}", agent.secrets.resolved().access), &value);
}

#[then(expr = "the debug form of the access still shows {string}")]
fn access_shows(agent: &mut Agent, value: String) {
    let debug = format!("{:?}", agent.secrets.resolved().access);
    assert!(debug.contains(&value), "{debug}");
}

#[then(expr = "the debug form of the resolved repository shows no {string}")]
fn resolved_hides(agent: &mut Agent, value: String) {
    assert_hides(&format!("{:?}", agent.secrets.resolved()), &value);
}

#[then(expr = "the debug form of the resolved repository still shows {string}")]
fn resolved_shows(agent: &mut Agent, value: String) {
    let debug = format!("{:?}", agent.secrets.resolved());
    assert!(debug.contains(&value), "{debug}");
}

#[then(expr = "the debug form of the settings shows no {string}")]
fn settings_hide(agent: &mut Agent, value: String) {
    assert_hides(&format!("{:?}", agent.secrets.settings), &value);
}

#[then(expr = "the debug form of the settings still shows {string}")]
fn settings_show(agent: &mut Agent, value: String) {
    let debug = format!("{:?}", agent.secrets.settings);
    assert!(debug.contains(&value), "{debug}");
}

// A backup the schedule starts, refused before it runs (§5).

#[when(expr = "the schedule starts a backup of the plan {string} on the repository")]
async fn schedule_starts_a_backup(agent: &mut Agent, plan: String) {
    let use_case = ResolveRepository::new(agent.secrets.store.clone());
    let error = match use_case.resolve(&agent.secrets.settings).await {
        Ok(_) => panic!("the repository's settings resolve, so the run isn't refused"),
        Err(error) => error,
    };
    // No secret of the repository was read, so none is masked.
    let reason = error.failure(&Redactor::new([]));
    let run = RunOperation::new(
        agent.engine.clone(),
        agent.clock.clone(),
        agent.store.clone(),
    );
    let target = Target {
        plan: Some(PlanId::try_from(plan).expect("a plan ID")),
        repository: None,
    };
    let id = OperationId::from_uuid(Uuid::from_u128(1));
    agent.finished = Some(run.refused(id, Trigger::Schedule, target, reason).await);
}

#[then(
    expr = "the operation ends as failed because the password's keyring entry {string} is missing"
)]
fn failed_for_a_missing_entry(agent: &mut Agent, entry: String) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::SecretRefused {
                field: "password".to_owned(),
                refusal: SettingRefusal::SecretMissing { source: entry },
            }
        }
    );
}

#[then(
    expr = "the operation ends as failed because the keyring entry {string} of the password didn't answer within {int} seconds"
)]
fn failed_for_a_keyring_that_did_not_answer(agent: &mut Agent, entry: String, seconds: u64) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::SecretRefused {
                field: "password".to_owned(),
                refusal: SettingRefusal::SecretKeyringTimedOut {
                    source: entry,
                    seconds,
                },
            }
        }
    );
}

#[then(
    expr = "the operation ends as failed because the file {string} of the password didn't answer within {int} seconds"
)]
fn failed_for_a_file_that_did_not_answer(agent: &mut Agent, path: String, seconds: u64) {
    assert_eq!(
        agent.operation().status(),
        &OperationStatus::Failed {
            reason: FailureReason::SecretRefused {
                field: "password".to_owned(),
                refusal: SettingRefusal::SecretFileTimedOut {
                    source: path,
                    seconds,
                },
            }
        }
    );
}

#[then(expr = "the run ends as a failed scheduled run of the plan {string}")]
fn ends_as_a_failed_scheduled_run(agent: &mut Agent, plan: String) {
    let operation = agent.operation();
    assert_eq!(
        RunEnd::of(operation),
        Some(RunEnd {
            operation: operation.id(),
            plan: PlanId::try_from(plan).expect("a plan ID"),
            trigger: Trigger::Schedule,
            outcome: RunOutcome::Failed,
        })
    );
}

// A scheduled run that waits for the session's keyring service while the
// daemon starts (§5).

impl SecretsWorld {
    fn started(&self) -> Timestamp {
        self.started.expect("the daemon started")
    }

    fn after_start(&self, seconds: i64) -> Timestamp {
        self.started() + SignedDuration::from_secs(seconds)
    }

    /// The repository the settings make, as the configuration holds it.
    fn entry(&self) -> RepositoryConfig {
        RepositoryConfig {
            settings: self.settings.clone(),
            locations: Default::default(),
            id: None,
            maintenance: Maintenance::Local,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        }
    }

    /// The daemon's start in the world's monotonic time.
    fn up_from(&self) -> SignedDuration {
        self.up_from.expect("the daemon started")
    }

    /// A resolution that waits for the keyring by `clock`, on which the
    /// daemon started at the monotonic time `up_from`.
    fn waits(&self, clock: Arc<dyn Clock>, up_from: SignedDuration) -> ResolveRepository {
        ResolveRepository::new(self.store.clone())
            .waiting_for_keyring(KeyringWait::new(clock, up_from))
    }

    fn waited(&self) -> Vec<SourceName> {
        self.waited
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn keyring_cause(&self) -> KeyringCause {
        match &self.unresolved().problem {
            Problem::KeyringUnavailable { cause, .. } => *cause,
            other => panic!("the keyring was expected to be unavailable, not {other:?}"),
        }
    }
}

fn demo_alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

#[given("the daemon started a moment ago")]
fn daemon_started(agent: &mut Agent) {
    agent.secrets.started = Some(agent.clock.now());
    agent.secrets.up_from = Some(agent.clock.monotonic());
}

#[given(expr = "the session's keyring service starts {int} seconds after the daemon")]
fn keyring_service_starts(agent: &mut Agent, seconds: i64) {
    let at = agent.secrets.after_start(seconds);
    agent
        .secrets
        .store
        .start_keyring_service_at(agent.clock.clone(), at);
}

#[when(expr = "the schedule starts a backup {int} seconds after the daemon started")]
async fn schedule_starts_after_start(agent: &mut Agent, seconds: i64) {
    agent.clock.advance(SignedDuration::from_secs(seconds));
    let waited = Arc::clone(&agent.secrets.waited);
    let waiting = move |source: &SourceName| {
        waited
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(source.clone());
    };
    let outcome = agent
        .secrets
        .waits(agent.clock.clone(), agent.secrets.up_from())
        .resolve_saved_unattended(&demo_alias(), &agent.secrets.entry(), &waiting)
        .await;
    agent.secrets.outcome = Some(outcome);
}

#[when(
    expr = "the schedule starts a backup {int} seconds after the daemon started, and the clock is set back an hour once the run waits"
)]
async fn schedule_starts_and_clock_set_back(agent: &mut Agent, seconds: i64) {
    agent.clock.advance(SignedDuration::from_secs(seconds));
    let clock = agent.clock.clone();
    let waiting = move |_: &SourceName| clock.set_back(SignedDuration::from_hours(1));
    let outcome = agent
        .secrets
        .waits(agent.clock.clone(), agent.secrets.up_from())
        .resolve_saved_unattended(&demo_alias(), &agent.secrets.entry(), &waiting)
        .await;
    assert!(
        agent.clock.now() < agent.secrets.started(),
        "the clock was never set back"
    );
    agent.secrets.outcome = Some(outcome);
}

#[when(expr = "a person tests the repository {int} seconds after the daemon started")]
async fn person_tests_after_start(agent: &mut Agent, seconds: i64) {
    agent.clock.advance(SignedDuration::from_secs(seconds));
    let outcome = agent
        .secrets
        .waits(agent.clock.clone(), agent.secrets.up_from())
        .resolve_saved(&demo_alias(), &agent.secrets.entry())
        .await;
    agent.secrets.outcome = Some(outcome);
}

#[when(
    expr = "the schedule starts a backup {int} seconds after the daemon started and the daemon stops once the run waits"
)]
async fn schedule_starts_and_daemon_stops(agent: &mut Agent, seconds: i64) {
    agent.clock.advance(SignedDuration::from_secs(seconds));
    // A clock that moves only when told, so the wait is still going when the
    // daemon stops.
    let clock = Arc::new(FakeClock::new(agent.clock.now()));
    let told = Arc::new(Mutex::new(false));
    let waiting = {
        let told = Arc::clone(&told);
        move |_: &SourceName| *told.lock().unwrap_or_else(PoisonError::into_inner) = true
    };
    // This clock's monotonic time starts at zero, `seconds` after the
    // daemon started.
    let use_case = agent
        .secrets
        .waits(clock.clone(), -SignedDuration::from_secs(seconds));
    let entry = agent.secrets.entry();
    let alias = demo_alias();
    let resolving = use_case.resolve_saved_unattended(&alias, &entry, &waiting);
    futures::pin_mut!(resolving);
    // The run waits once it sleeps on the clock.
    let asleep = async {
        while clock.sleepers() == 0 {
            tokio::task::yield_now().await;
        }
    };
    futures::pin_mut!(asleep);
    // The daemon's stop drops the run's start, and the wait with it.
    match futures::future::select(resolving, asleep).await {
        futures::future::Either::Left((outcome, _)) => {
            panic!("the run's wait ended before the daemon stopped: {outcome:?}")
        }
        futures::future::Either::Right(_) => {
            assert!(
                *told.lock().unwrap_or_else(PoisonError::into_inner),
                "the run waited without the daemon being told"
            );
            agent.secrets.stopped_at = Some(clock.now());
        }
    }
}

#[then(expr = "the run's secrets are resolved {int} seconds after the daemon started")]
fn resolved_after_start(agent: &mut Agent, seconds: i64) {
    assert_eq!(
        agent.secrets.resolved().access.password.expose(),
        "CANARY-keyring-password"
    );
    assert_eq!(agent.clock.now(), agent.secrets.after_start(seconds));
}

#[then(
    expr = "the run fails {int} seconds after the daemon started because there is no keyring service to ask"
)]
fn fails_with_no_service(agent: &mut Agent, seconds: i64) {
    assert_eq!(agent.secrets.keyring_cause(), KeyringCause::NoService);
    assert_eq!(agent.clock.now(), agent.secrets.after_start(seconds));
}

#[then(
    expr = "the run fails once the daemon has been up {int} seconds, because there is no keyring service to ask"
)]
fn fails_with_no_service_after_uptime(agent: &mut Agent, seconds: i64) {
    assert_eq!(agent.secrets.keyring_cause(), KeyringCause::NoService);
    let up_from = agent.secrets.up_from();
    assert_eq!(
        agent.clock.monotonic().saturating_sub(up_from),
        SignedDuration::from_secs(seconds)
    );
}

#[then(expr = "the run fails {int} seconds after the daemon started because the keyring is locked")]
fn fails_locked(agent: &mut Agent, seconds: i64) {
    assert_eq!(agent.secrets.keyring_cause(), KeyringCause::Locked);
    assert_eq!(agent.clock.now(), agent.secrets.after_start(seconds));
}

#[then(expr = "the run fails {int} seconds after the daemon started because the keyring refused")]
fn fails_refused(agent: &mut Agent, seconds: i64) {
    assert_eq!(agent.secrets.keyring_cause(), KeyringCause::Refused);
    assert_eq!(agent.clock.now(), agent.secrets.after_start(seconds));
}

#[then(expr = "the run is refused with {string} for the keyring entry {string}")]
fn refused_for_keyring_entry(agent: &mut Agent, code: String, entry: String) {
    // No secret of the repository was read, so none is masked.
    let FailureReason::SecretRefused { field, refusal } =
        agent.secrets.error().failure(&Redactor::new([]))
    else {
        panic!("the run was expected to be refused for a secret")
    };
    assert_eq!(field, "password");
    let source = match &refusal {
        SettingRefusal::SecretKeyringUnavailable { source, .. }
        | SettingRefusal::SecretKeyringLocked { source, .. } => source,
        other => panic!("a keyring refusal was expected, not {other:?}"),
    };
    assert_eq!(*source, entry);
    assert_eq!(refusal.code(), code, "{refusal:?}");
}

#[then(expr = "the wait ended {int} seconds after the daemon started, and the run never started")]
fn wait_ended_after_start(agent: &mut Agent, seconds: i64) {
    assert_eq!(
        agent.secrets.stopped_at,
        Some(agent.secrets.after_start(seconds))
    );
    assert!(agent.secrets.outcome.is_none());
}

#[then(expr = "the keyring entry {string} was read {int} time(s)")]
fn keyring_read_times(agent: &mut Agent, entry: String, times: usize) {
    let entry = KeyringEntry::parse(&entry).expect("the scenario's entry is valid");
    assert_eq!(
        agent.secrets.store.read_count(&SecretRef::Keyring(entry)),
        times
    );
}

#[then(expr = "the file {string} was read once")]
fn file_read_once(agent: &mut Agent, path: String) {
    let file = SecretRef::File {
        path,
        read: FileRead::Whole,
    };
    assert_eq!(agent.secrets.store.read_count(&file), 1);
}

#[then(expr = "the daemon logged once that the run waits for the keyring entry {string}")]
fn logged_waiting_for(agent: &mut Agent, entry: String) {
    assert_eq!(agent.secrets.waited(), vec![SourceName::Keyring { entry }]);
}

#[then(expr = "what the daemon logged of the wait holds no {string}")]
fn logged_wait_holds_no(agent: &mut Agent, secret: String) {
    assert_hides(&format!("{:?}", agent.secrets.waited()), &secret);
}

#[then("the daemon never logged that the run waits")]
fn never_logged_waiting(agent: &mut Agent) {
    assert_eq!(agent.secrets.waited(), Vec::new());
}

// Checking the keyring entries (features/secrets/keyring_check.feature).

impl SecretsWorld {
    /// Adds the repository `name`, at a location of its own, with its
    /// password in `password`.
    fn checked_repository(&mut self, name: &str, password: SecretRef) {
        let alias = RepositoryAlias::parse(name).expect("the scenario's alias is valid");
        let repository = RepositoryConfig {
            settings: RepositorySettings {
                repository: format!("/srv/{name}"),
                password,
                ..self.settings.clone()
            },
            locations: Default::default(),
            id: None,
            maintenance: Maintenance::Local,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        };
        self.checked_repositories.insert(alias, repository);
    }

    fn keyring_check(&self) -> &KeyringCheck {
        self.keyring_check
            .as_ref()
            .expect("the keyring entries were checked")
    }

    /// What the check reported of the repository `name`.
    fn read_of(&self, name: &str) -> &KeyringRead {
        let repository = RepositoryAlias::parse(name).expect("the scenario's alias is valid");
        self.reported(|read| read.user == KeyringUser::Repository(repository.clone()))
    }

    /// What the check reported of the channel `name`'s entry `entry`.
    fn channel_read_of(&self, name: &str, entry: &str) -> &KeyringRead {
        let channel = ChannelName::parse(name).expect("the scenario's channel is valid");
        self.reported(|read| {
            read.user == KeyringUser::Channel(channel.clone()) && read.entry.to_string() == entry
        })
    }

    fn reported(&self, which: impl Fn(&KeyringRead) -> bool) -> &KeyringRead {
        let check = self.keyring_check();
        check
            .reads
            .iter()
            .find(|read| which(read))
            .unwrap_or_else(|| panic!("the check reported no such read: {check:?}"))
    }
}

#[given(expr = "the repository {string} takes its password from the keyring entry {string}")]
fn repository_in_keyring(agent: &mut Agent, name: String, entry: String) {
    let entry = KeyringEntry::parse(&entry).expect("the scenario's entry is valid");
    agent
        .secrets
        .checked_repository(&name, SecretRef::Keyring(entry));
}

#[given(expr = "the repository {string} takes its password from the command {string}")]
fn repository_from_command(agent: &mut Agent, name: String, command: String) {
    let command = agent.secrets.command(&command);
    agent
        .secrets
        .checked_repository(&name, SecretRef::Command(command));
}

#[given(expr = "the repository {string} takes its password from the file {string}")]
fn repository_from_file(agent: &mut Agent, name: String, path: String) {
    let read = FileRead::FirstLine;
    agent
        .secrets
        .checked_repository(&name, SecretRef::File { path, read });
}

#[given(
    expr = "the notification channel {string} takes its address from the keyring entry {string} and its token from the keyring entry {string}"
)]
fn channel_in_keyring(agent: &mut Agent, name: String, address: String, token: String) {
    let entry = |text: &str| KeyringEntry::parse(text).expect("the scenario's entry is valid");
    let channel = Channel::new(
        ChannelKind::Ntfy,
        SecretRef::Keyring(entry(&address)),
        Some(Auth::Bearer(SecretRef::Keyring(entry(&token)))),
    )
    .expect("the scenario's channel is valid");
    let name = ChannelName::parse(&name).expect("the scenario's channel is valid");
    agent.secrets.checked_channels.insert(name, channel);
}

#[given(expr = "the notification channel {string} takes its address from the command {string}")]
fn channel_from_command(agent: &mut Agent, name: String, command: String) {
    let command = agent.secrets.command(&command);
    let channel = Channel::new(ChannelKind::Webhook, SecretRef::Command(command), None)
        .expect("the scenario's channel is valid");
    let name = ChannelName::parse(&name).expect("the scenario's channel is valid");
    agent.secrets.checked_channels.insert(name, channel);
}

#[given(expr = "the keyring has no entry {string}")]
fn keyring_has_no_entry(_agent: &mut Agent, _entry: String) {
    // The keyring holds nothing until a scenario puts an entry in it.
}

#[when("the keyring entries are checked")]
async fn check_entries(agent: &mut Agent) {
    let notifications =
        Notifications::new(agent.secrets.checked_channels.clone(), Default::default())
            .expect("the scenario's channels are valid");
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        Hooks::default(),
        agent.secrets.checked_repositories.clone(),
        Vec::new(),
    )
    .and_then(|config| config.with_notifications(notifications))
    .expect("the scenario's configuration is valid");
    agent.secrets.keyring_check =
        Some(check_keyring_entries(&config, agent.secrets.store.as_ref()).await);
}

#[then(expr = "the check reports the repository {string} and its keyring entry {string} read")]
fn check_reports_read(agent: &mut Agent, name: String, entry: String) {
    let read = agent.secrets.read_of(&name);
    assert_eq!(read.entry.to_string(), entry);
    assert_eq!(read.refused, None, "{read:?}");
}

#[then(expr = "the check reports the repository {string} refused with {string}")]
fn check_reports_refused(agent: &mut Agent, name: String, code: String) {
    let read = agent.secrets.read_of(&name);
    let refused = read
        .refused
        .as_ref()
        .unwrap_or_else(|| panic!("{name:?} was read: {read:?}"));
    assert_eq!(refused.code(), code, "{refused:?}");
}

#[then(
    expr = "the check reports the notification channel {string} and its keyring entry {string} read"
)]
fn check_reports_channel_read(agent: &mut Agent, name: String, entry: String) {
    let read = agent.secrets.channel_read_of(&name, &entry);
    assert_eq!(read.refused, None, "{read:?}");
}

#[then(
    expr = "the check reports the notification channel {string} and its keyring entry {string} refused with {string}"
)]
fn check_reports_channel_refused(agent: &mut Agent, name: String, entry: String, code: String) {
    let read = agent.secrets.channel_read_of(&name, &entry);
    let refused = read
        .refused
        .as_ref()
        .unwrap_or_else(|| panic!("{entry:?} was read: {read:?}"));
    assert_eq!(refused.code(), code, "{refused:?}");
}

#[then(expr = "the check reports the repositories {string} alone")]
fn check_reports_alone(agent: &mut Agent, names: String) {
    let reported: Vec<String> = agent
        .secrets
        .keyring_check()
        .reads
        .iter()
        .map(|read| match &read.user {
            KeyringUser::Repository(alias) => alias.as_str().to_owned(),
            KeyringUser::Channel(name) => format!("channel {}", name.as_str()),
        })
        .collect();
    assert_eq!(reported, names.split(", ").collect::<Vec<_>>());
}

#[then(expr = "nothing but the keyring entry {string} was read")]
fn only_entry_read(agent: &mut Agent, entry: String) {
    let entry = KeyringEntry::parse(&entry).expect("the scenario's entry is valid");
    assert_eq!(agent.secrets.store.reads(), vec![SecretRef::Keyring(entry)]);
}

#[then(expr = "the check's report holds no {string}")]
fn report_holds_no(agent: &mut Agent, secret: String) {
    let report = format!("{:?}", agent.secrets.keyring_check());
    assert!(!report.contains(&secret), "{report}");
}

#[then("the check passes")]
fn check_passes(agent: &mut Agent) {
    assert!(agent.secrets.keyring_check().passed());
}

#[then("the check fails")]
fn check_fails(agent: &mut Agent) {
    assert!(!agent.secrets.keyring_check().passed());
}
