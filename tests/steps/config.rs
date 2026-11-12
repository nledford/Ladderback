//! Steps for the `@e2e` config scenarios: the real `FileConfigStore` reads and
//! writes a real file in the scenario's scratch folder, so the scenarios follow
//! what a person who edits the file sees (technical design §6.1).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use app::ports::{ConfigError, ConfigStore, Problem};
use config_adapter::{FileConfigStore, MigrationError, Schemas};
use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use domain::PlanId;
use domain::planning::{AgentConfig, ValueKind};
use platform::SystemClock;

use super::Agent;

/// What a scenario sets up and what the last load or save gave.
#[derive(Default)]
pub struct ConfigSetup {
    /// The text the file was written with.
    written: Option<String>,
    /// The versions of the format the agent reads, when not production's.
    schemas: Option<Schemas>,
    loaded: Option<Result<AgentConfig, ConfigError>>,
}

/// `{scratch}` in a step's text stands for the scenario's scratch folder,
/// and the placeholders [`long_texts`] knows for what they say.
pub(super) fn in_scratch(agent: &Agent, text: &str) -> String {
    long_texts(&text.replace("{scratch}", &agent.scratch().display().to_string()))
}

/// `text` with each placeholder for a text too long to write in a step
/// replaced: a file or a request a person wrote can be that long, a step's
/// text can't. `{a location of N bytes}` is a local path of `N` bytes,
/// `{a value of N bytes}` is `N` bytes of `v`, `{N options}` is `N` options
/// (`o1 = "demo", …`) as the inside of a TOML inline table, and `{N options
/// as JSON}` is the same as a JSON object. Any other `{` is left as it is.
pub(super) fn long_texts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        let inside = after.get(1..).and_then(|after| after.split_once('}'));
        match inside.and_then(|(name, after)| Some((placeholder(name)?, after))) {
            Some((text, after)) => {
                out.push_str(&text);
                rest = after;
            }
            None => {
                out.push('{');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// What the placeholder `{name}` stands for, if it is one.
fn placeholder(name: &str) -> Option<String> {
    let bytes = |prefix: &str| -> Option<usize> {
        name.strip_prefix(prefix)?
            .strip_suffix(" bytes")?
            .parse()
            .ok()
    };
    let options = |count: &str, one: fn(usize) -> String, comma: &str| -> Option<String> {
        let count: usize = count.parse().ok()?;
        Some((1..=count).map(one).collect::<Vec<_>>().join(comma))
    };
    if let Some(bytes) = bytes("a location of ") {
        Some(format!("/srv/{}", "r".repeat(bytes - "/srv/".len())))
    } else if let Some(bytes) = bytes("a value of ") {
        Some("v".repeat(bytes))
    } else if let Some(count) = name.strip_suffix(" options as JSON") {
        let json = options(count, |n| format!("\"o{n}\":\"demo\""), ",")?;
        Some(format!("{{{json}}}"))
    } else {
        options(
            name.strip_suffix(" options")?,
            |n| format!("o{n} = \"demo\""),
            ", ",
        )
    }
}

fn config_path(agent: &Agent) -> PathBuf {
    agent.scratch().join("config").join("config.toml")
}

fn history_dir(agent: &Agent) -> PathBuf {
    agent.scratch().join("config").join("history")
}

fn store(agent: &Agent) -> FileConfigStore {
    let store = FileConfigStore::new(config_path(agent), Arc::new(SystemClock));
    match agent.config.schemas.clone() {
        Some(schemas) => store.with_schemas(schemas),
        None => store,
    }
}

fn loaded(agent: &Agent) -> &AgentConfig {
    match agent.config.loaded.as_ref() {
        Some(Ok(config)) => config,
        other => panic!("the configuration should have loaded, got {other:?}"),
    }
}

fn refusal(agent: &Agent) -> &ConfigError {
    match agent.config.loaded.as_ref() {
        Some(Err(error)) => error,
        other => panic!("the load should have been refused, got {other:?}"),
    }
}

/// The saved file as TOML, to look at its keys rather than its layout.
fn table(agent: &Agent) -> toml::Table {
    let text = fs::read_to_string(config_path(agent)).expect("the config file");
    text.parse().expect("the config file is TOML")
}

/// Version 1 to 2: an agent's `name` became its `hostname`.
fn rename_name(table: &mut toml::Table) -> Result<(), MigrationError> {
    if let Some(agent) = table.get_mut("agent").and_then(toml::Value::as_table_mut)
        && let Some(name) = agent.remove("name")
    {
        agent.insert("hostname".to_owned(), name);
    }
    Ok(())
}

#[given(expr = "a config file holding:")]
fn a_config_file(agent: &mut Agent, step: &Step) {
    let text = step.docstring.as_deref().expect("the step has a text");
    // cucumber hands a docstring over with the newline after its opening
    // quotes, and without the one a file ends with, which an editor adds.
    let text = format!("{}\n", in_scratch(agent, text.trim_start_matches('\n')));
    let path = config_path(agent);
    fs::create_dir_all(path.parent().expect("a folder")).unwrap();
    fs::write(&path, &text).unwrap();
    agent.config.written = Some(text);
}

#[given(
    expr = "the agent reads version 2 of the format, where an agent's name became its hostname"
)]
fn reads_version_two(agent: &mut Agent) {
    agent.config.schemas = Some(Schemas {
        current: 2,
        steps: vec![rename_name],
    });
}

#[when(expr = "the agent loads the config file")]
async fn loads(agent: &mut Agent) {
    let result = store(agent).load().await;
    agent.config.loaded = Some(result);
}

#[when(expr = "the agent saves the configuration it loaded")]
async fn saves(agent: &mut Agent) {
    let config = loaded(agent).clone();
    store(agent).save(&config).await.expect("the save");
}

#[then(expr = "the agent has the plan {string} backing up {string} into {string}")]
fn has_the_plan(agent: &mut Agent, id: String, source: String, repository: String) {
    let plan = loaded(agent)
        .plan(&PlanId::parse(&id).expect("a plan ID"))
        .unwrap_or_else(|| panic!("the agent has no plan {id:?}"));
    assert_eq!(plan.repository.as_str(), repository);
    assert!(
        plan.sources.iter().any(|s| s.as_str() == source),
        "{source:?} isn't among {:?}",
        plan.sources
    );
}

#[then(expr = "the agent's hostname is {string}")]
fn hostname_is(agent: &mut Agent, hostname: String) {
    let found = loaded(agent).agent().hostname.as_ref();
    assert_eq!(found.map(|h| h.as_str()), Some(hostname.as_str()));
}

#[then(expr = "the config file is exactly as it was written")]
fn file_is_as_written(agent: &mut Agent) {
    let now = fs::read_to_string(config_path(agent)).expect("the config file");
    assert_eq!(Some(&now), agent.config.written.as_ref());
}

#[then(expr = "the load is refused because the key {string} is unknown")]
fn refused_unknown_key(agent: &mut Agent, key: String) {
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::UnknownKey,
            key: Some(found),
            ..
        } => assert_eq!(*found, key),
        other => panic!("the load should name the unknown key {key:?}, got {other:?}"),
    }
}

#[then(expr = "the load is refused because the glob file {string} is not an absolute path")]
fn refused_relative_glob_file(agent: &mut Agent, key: String) {
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Value(ValueKind::GlobFile),
            key: Some(found),
            ..
        } => assert_eq!(*found, key),
        other => panic!("the load should refuse the glob file at {key:?}, got {other:?}"),
    }
}

#[then(expr = "the load is refused because the location {string} is longer than 64 KiB")]
fn refused_long_location(agent: &mut Agent, key: String) {
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Value(ValueKind::Location),
            key: Some(found),
            ..
        } => assert_eq!(*found, key),
        other => panic!("the load should refuse the location at {key:?}, got {other:?}"),
    }
}

#[then(expr = "the load is refused because the option {string} is longer than 64 KiB")]
fn refused_long_option(agent: &mut Agent, key: String) {
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Value(ValueKind::OptionValue),
            key: Some(found),
            ..
        } => assert_eq!(*found, key),
        other => panic!("the load should refuse the option at {key:?}, got {other:?}"),
    }
}

#[then(expr = "the load is refused because {string} holds more than 64 options")]
fn refused_too_many_options(agent: &mut Agent, key: String) {
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Value(ValueKind::Options),
            key: Some(found),
            ..
        } => assert_eq!(*found, key),
        other => panic!("the load should refuse the options at {key:?}, got {other:?}"),
    }
}

#[then(expr = "the load is refused because the path {string} is not an absolute path")]
fn refused_relative_local_path(agent: &mut Agent, key: String) {
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Value(ValueKind::LocalPath),
            key: Some(found),
            ..
        } => assert_eq!(*found, key),
        other => panic!("the load should refuse the path at {key:?}, got {other:?}"),
    }
}

#[then(expr = "the load is refused because {string} and {string} are one repository")]
fn refused_one_repository(agent: &mut Agent, first: String, second: String) {
    let alias = |name: &str| domain::RepositoryAlias::parse(name).expect("an alias");
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Violation(violation),
            key,
            detail,
            ..
        } => {
            assert_eq!(
                violation,
                &domain::planning::Violation::SameRepository {
                    first: alias(&first),
                    second: alias(&second),
                }
            );
            assert_eq!(key.as_deref(), Some(&*format!("repositories.{second}")));
            for name in [&first, &second] {
                assert!(
                    detail.contains(&format!("repositories.{name}")),
                    "the refusal names {name}: {detail}"
                );
            }
        }
        other => panic!("the load should be refused as one repository, got {other:?}"),
    }
}

#[then(expr = "the refusal points at line {int} of the config file")]
fn refusal_points_at(agent: &mut Agent, line: usize) {
    let path = config_path(agent).display().to_string();
    match refusal(agent) {
        ConfigError::Invalid { file, at, .. } => {
            assert_eq!(*file, path);
            assert_eq!(at.map(|at| at.line), Some(line));
        }
        other => panic!("the load should be refused as invalid, got {other:?}"),
    }
}

#[then(expr = "the refusal never shows {string}")]
fn refusal_never_shows(agent: &mut Agent, text: String) {
    let error = refusal(agent);
    for shown in [format!("{error}"), format!("{error:?}")] {
        assert!(!shown.contains(&text), "{shown}");
    }
}

#[then(expr = "the load is refused because the file names no schema version")]
fn refused_no_schema(agent: &mut Agent) {
    let path = config_path(agent).display().to_string();
    assert_eq!(refusal(agent), &ConfigError::SchemaMissing { file: path });
}

#[then(
    expr = "the load is refused because the file is in version {int} and the agent reads version {int}"
)]
fn refused_newer(agent: &mut Agent, found: u64, supported: u64) {
    let path = config_path(agent).display().to_string();
    assert_eq!(
        refusal(agent),
        &ConfigError::SchemaNewer {
            file: path,
            found,
            supported
        }
    );
}

#[then(expr = "no earlier version of the config file is kept")]
fn nothing_kept(agent: &mut Agent) {
    let kept = fs::read_dir(history_dir(agent)).map_or(0, Iterator::count);
    assert_eq!(kept, 0);
}

#[then(expr = "the saved config file gives the repository {string} the {string} {string}")]
fn saved_gives_key(agent: &mut Agent, alias: String, key: String, value: String) {
    let value = in_scratch(agent, &value);
    let table = table(agent);
    let found = table["repositories"][alias.as_str()][key.as_str()].as_str();
    assert_eq!(found, Some(value.as_str()));
}

#[then(
    expr = "the saved config file gives the repository {string} the option {string} as {string}"
)]
fn saved_gives_option(agent: &mut Agent, alias: String, option: String, value: String) {
    let value = in_scratch(agent, &value);
    let table = table(agent);
    let found = table["repositories"][alias.as_str()]["options"][option.as_str()].as_str();
    assert_eq!(found, Some(value.as_str()));
}

#[then(expr = "the saved config file doesn't hold {string}")]
fn saved_doesnt_hold(agent: &mut Agent, text: String) {
    let saved = fs::read_to_string(config_path(agent)).expect("the config file");
    assert!(!saved.contains(&text), "{saved}");
}

#[then(expr = "the history of the config file holds the old file exactly as it was written")]
fn history_holds_old_file(agent: &mut Agent) {
    let kept: Vec<_> = fs::read_dir(history_dir(agent))
        .expect("a history folder")
        .map(|entry| fs::read_to_string(entry.expect("an entry").path()).expect("a kept version"))
        .collect();
    assert_eq!(
        Some(&kept),
        agent
            .config
            .written
            .as_ref()
            .map(|text| vec![text.clone()])
            .as_ref()
    );
}

#[then(expr = "the config file now says it is in version {int}")]
fn file_says_version(agent: &mut Agent, version: i64) {
    assert_eq!(table(agent)["schema"].as_integer(), Some(version));
}
