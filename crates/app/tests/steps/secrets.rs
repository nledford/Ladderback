//! Steps for `features/secrets/*.feature`: they drive `ResolveRepository`
//! over a `FakeSecrets` store held by the world's `secrets` field.

use std::sync::Arc;

use app::fakes::{FakeSecrets, Script};
use app::secrets::ResolvedRepository;
use app::secrets::{Cause, Field, RepositorySettings, ResolveError, ResolveRepository};
use cucumber::{given, then, when};
use domain::secrets::{
    CommandLine, FileRead, KeyringEntry, MAX_SECRET_BYTES, Problem, SecretError, SecretRef,
    SecretValue, SecretWarning, SourceName, TemplateError,
};

use super::Agent;

/// The store, the settings being built, and what resolving them gave.
pub struct SecretsWorld {
    store: Arc<FakeSecrets>,
    settings: RepositorySettings,
    outcome: Option<Result<ResolvedRepository, ResolveError>>,
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

#[given(expr = "the keyring holds {string} under {string}")]
fn keyring_holds(agent: &mut Agent, secret: String, entry: String) {
    let entry = KeyringEntry::parse(&entry).expect("the scenario's entry is valid");
    agent.secrets.store.set_keyring_entry(&entry, &secret);
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
    agent.secrets.settings.repository = filler(bytes);
}

#[given(expr = "the repository is {int} references to the variable {string}")]
fn repository_is_references(agent: &mut Agent, count: usize, name: String) {
    agent.secrets.settings.repository = format!("${{{name}}}").repeat(count);
}

#[given(expr = "the repository is {int} bytes of text followed by {string}")]
fn repository_is_bytes_then(agent: &mut Agent, bytes: usize, rest: String) {
    agent.secrets.settings.repository = filler(bytes) + &rest;
}

#[given(expr = "the hot repository is {string}")]
fn hot_repository_is(agent: &mut Agent, repository: String) {
    agent.secrets.settings.repo_hot = Some(repository);
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
    let use_case = ResolveRepository::new(agent.secrets.store.clone());
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
        agent.secrets.resolved().repo_hot.as_deref(),
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
    let options = &agent.secrets.resolved().options_hot;
    assert_eq!(options.get(&name), Some(&value));
}

#[then(expr = "the resolved cold option {string} is {string}")]
fn resolved_cold_option(agent: &mut Agent, name: String, value: String) {
    let options = &agent.secrets.resolved().options_cold;
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

#[then("the reference is refused because it would reach the arguments of a process")]
fn refused(agent: &mut Agent) {
    assert_eq!(agent.secrets.cause(), &Cause::ReferenceInProcessArguments);
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
    assert!(
        matches!(
            agent.secrets.unresolved().problem,
            Problem::Unreachable { .. }
        ),
        "{:?}",
        agent.secrets.unresolved()
    );
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

#[then("the problem is that the command took too long")]
fn problem_timed_out(agent: &mut Agent) {
    assert!(
        matches!(
            agent.secrets.unresolved().problem,
            Problem::TimedOut { seconds } if seconds > 0
        ),
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
    let expected = SecretWarning::FileReadableByOthers { path };
    let warnings = &agent.secrets.resolved().warnings;
    assert_eq!(warnings.iter().filter(|w| **w == expected).count(), 1);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[then("the resolution warns about nothing")]
fn warns_about_nothing(agent: &mut Agent) {
    assert_eq!(agent.secrets.resolved().warnings, []);
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
