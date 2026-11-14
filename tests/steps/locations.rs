//! The `@e2e` steps of `features/repositories/repository_locations.feature`
//! (decision 80): a repository whose locations are two local copies of one
//! scratch repository, so both hold the same repository ID, written into the
//! config file the real daemon and the real CLI read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use app::ports::{ConfigError, ConfigStore, Problem};
use config_adapter::FileConfigStore;
use cucumber::{given, then, when};
use platform::SystemClock;

use super::events_steps::{self, Events};
use super::operations_steps::{exchange, start_backup, wait_ended};
use super::{Agent, blocking};

/// What the location scenarios keep between steps.
#[derive(Default)]
pub struct LocationsE2e {
    /// The repository's alias.
    alias: Option<String>,
    /// Each location's folder, by the name the scenario calls it, in order.
    places: Vec<(String, PathBuf)>,
    /// The options of each location, by name: TOML key and value as written.
    options: BTreeMap<String, Vec<(String, String)>>,
    /// The plan that backs up into the repository.
    plan: Option<String>,
    /// A repository table the scenario adds below the repository's own.
    extra: String,
    /// The secret no output may show.
    canary: Option<String>,
    /// What loading the config file came to.
    loaded: Option<Result<(), ConfigError>>,
    /// The config file as last written.
    written: String,
    /// The operation the scenario ran.
    operation: Option<String>,
    /// The text of every event the connected page heard during the run.
    heard: String,
}

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 path")
}

/// Copies the folder `from` to `to`, as a person copies a repository.
fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy's folder");
    for entry in fs::read_dir(from).expect("the folder reads") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the file copies");
        }
    }
}

fn place(agent: &Agent, name: &str) -> PathBuf {
    agent
        .locations
        .places
        .iter()
        .find(|(known, _)| known == name)
        .map(|(_, path)| path.clone())
        .unwrap_or_else(|| panic!("no location {name:?}"))
}

/// The config file's place for the daemon, and for the CLI, which the
/// interop steps point at another folder.
fn config_files(agent: &Agent) -> [PathBuf; 2] {
    [
        Events::config_dir(agent).join("config.toml"),
        agent.scratch().join("config").join("config.toml"),
    ]
}

fn location_table(agent: &Agent, name: &str) -> String {
    let mut text = format!("repository = {}\n", quoted(path_text(&place(agent, name))));
    if let Some(options) = agent.locations.options.get(name) {
        let pairs: Vec<String> = options
            .iter()
            .map(|(key, value)| format!("{key} = {}", quoted(value)))
            .collect();
        text.push_str(&format!("options = {{ {} }}\n", pairs.join(", ")));
    }
    text
}

/// Writes the repository with its locations, its plan and whatever table the
/// scenario added, where both the daemon and the CLI read it.
fn write_config(agent: &mut Agent) {
    let alias = agent.locations.alias.clone().expect("a repository");
    let id = agent
        .backup
        .id
        .clone()
        .expect("the repository's ID is known");
    let names: Vec<String> = agent
        .locations
        .places
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    let (first, rest) = names.split_first().expect("a first location");
    // The password is read from a file, as a profile can name it: an export
    // refuses a password written in the configuration.
    let password = agent.scratch().join("password");
    fs::write(&password, super::backup_steps::PASSWORD).expect("the password file");
    // A password file only its owner can read, as a person keeps one.
    fs::set_permissions(
        &password,
        <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600),
    )
    .expect("the password file's mode");
    let mut text = format!(
        "schema = 1\n\n[repositories.{alias}]\n{}repository-id = {}\npassword-file = {}\nmaintenance = \"local\"\n",
        location_table(agent, first),
        quoted(id.as_str()),
        quoted(path_text(&password)),
    );
    for name in rest {
        text.push_str(&format!(
            "\n[[repositories.{alias}.locations]]\n{}",
            location_table(agent, name)
        ));
    }
    text.push_str(&agent.locations.extra);
    if let Some(plan) = &agent.locations.plan {
        let source = agent.backup.source.clone().expect("a plan source");
        text.push_str(&format!(
            "\n[[plans]]\nid = {}\nrepository = {}\nsources = [{}]\n",
            quoted(plan),
            quoted(&alias),
            quoted(path_text(&source)),
        ));
    }
    for file in config_files(agent) {
        fs::create_dir_all(file.parent().expect("a folder")).expect("the config folder");
        fs::write(&file, &text).expect("the config file");
    }
    agent.locations.written = text;
}

async fn load(agent: &mut Agent) {
    let [file, _] = config_files(agent);
    let loaded = FileConfigStore::new(file, Arc::new(SystemClock))
        .load()
        .await
        .map(drop);
    agent.locations.loaded = Some(loaded);
}

/// The line of the config file as written that is exactly `text`, from 1.
fn line_of(agent: &Agent, text: &str) -> usize {
    agent
        .locations
        .written
        .lines()
        .position(|line| line == text)
        .map(|index| index + 1)
        .unwrap_or_else(|| panic!("the config file has no line {text:?}"))
}

fn refusal(agent: &Agent) -> &ConfigError {
    match agent.locations.loaded.as_ref() {
        Some(Err(error)) => error,
        other => panic!("the configuration should have been refused, got {other:?}"),
    }
}

/// A text with the bidi isolation marks a placeable carries left out.
fn read_as_shown(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{2066}'..='\u{2069}').contains(c))
        .collect()
}

#[given(
    expr = "a repository {string} that records its repository ID, with the locations {string} and {string}, in that order"
)]
async fn repository_with_locations(
    agent: &mut Agent,
    alias: String,
    first: String,
    second: String,
) {
    let folder = super::backup_steps::new_repository(agent).await;
    super::backup_steps::id_known(agent).await;
    let copy = agent.scratch().join(&second);
    let from = folder.clone();
    let to = copy.clone();
    blocking(move || copy_folder(&from, &to)).await;
    agent.locations.alias = Some(alias);
    agent.locations.places = vec![(first, folder), (second, copy)];
    write_config(agent);
}

#[given(expr = "the plan {string} backs up to {string}")]
fn plan_backs_up(agent: &mut Agent, plan: String, alias: String) {
    assert_eq!(agent.locations.alias.as_deref(), Some(alias.as_str()));
    super::backup_steps::source_with_files(agent, &plan, 3);
    agent.locations.plan = Some(plan);
    write_config(agent);
}

#[given(expr = "a repository {string} that records no repository ID")]
fn records_no_id(agent: &mut Agent, alias: String) {
    let folder = agent.scratch().join(&alias);
    agent.locations.extra = format!(
        "\n[repositories.{alias}]\nrepository = {}\npassword = {}\n",
        quoted(path_text(&folder)),
        quoted(super::backup_steps::PASSWORD),
    );
    write_config(agent);
}

#[when("the configuration gives it a second location")]
async fn gives_it_a_second_location(agent: &mut Agent) {
    let table = agent
        .locations
        .extra
        .lines()
        .find_map(|line| line.strip_prefix("[repositories."))
        .and_then(|rest| rest.strip_suffix(']'))
        .expect("a repository with no ID")
        .to_owned();
    let second = agent.scratch().join(format!("{table}-second"));
    agent.locations.extra.push_str(&format!(
        "\n[[repositories.{table}.locations]]\nrepository = {}\n",
        quoted(path_text(&second)),
    ));
    write_config(agent);
    load(agent).await;
}

#[then("the configuration is refused at the table of that location")]
fn refused_at_the_table(agent: &mut Agent) {
    let header = agent
        .locations
        .written
        .lines()
        .rfind(|line| line.starts_with("[[repositories.") && line.ends_with(".locations]]"))
        .expect("a table of a location")
        .to_owned();
    let alias = header
        .trim_start_matches("[[repositories.")
        .trim_end_matches(".locations]]")
        .to_owned();
    let line = line_of(agent, &header);
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::MissingKey,
            key,
            at,
            ..
        } => {
            assert_eq!(
                key.as_deref(),
                Some(&*format!("repositories.{alias}.repository-id"))
            );
            assert_eq!(
                at.map(|at| at.line),
                Some(line),
                "the refusal is at {header}"
            );
        }
        other => panic!("the configuration should miss the repository ID, got {other:?}"),
    }
}

#[then("the message says to run a job once or to write the ID")]
async fn message_says_the_way_out(agent: &mut Agent) {
    let refused = events_steps::try_serve(agent).await;
    let Err((status, log)) = refused else {
        panic!("the daemon started on a configuration it should refuse")
    };
    assert!(!status.success(), "{log}");
    let log = read_as_shown(&log);
    for words in ["Run a job on it once", "repository-id"] {
        assert!(log.contains(words), "the refusal says {words:?}: {log}");
    }
}

#[when(
    expr = "the configuration gives the repository {string} a second location at the address of its first"
)]
async fn second_at_the_first(agent: &mut Agent, alias: String) {
    assert_eq!(agent.locations.alias.as_deref(), Some(alias.as_str()));
    let first = agent
        .locations
        .places
        .first()
        .expect("a first location")
        .1
        .clone();
    agent.locations.places[1].1 = first;
    write_config(agent);
    load(agent).await;
}

#[then("the configuration is refused at the second location's key")]
fn refused_at_the_second(agent: &mut Agent) {
    let alias = agent.locations.alias.clone().expect("a repository");
    match refusal(agent) {
        ConfigError::Invalid {
            problem: Problem::Value(domain::planning::ValueKind::Location),
            key,
            ..
        } => assert_eq!(
            key.as_deref(),
            Some(&*format!("repositories.{alias}.locations[0].repository"))
        ),
        other => panic!("the second location should be refused, got {other:?}"),
    }
}

#[given(
    expr = "the location {string} carries the canary secret {string} in its option {string} as the reference {string}"
)]
fn carries_the_canary(
    agent: &mut Agent,
    name: String,
    canary: String,
    option: String,
    reference: String,
) {
    let variable = reference
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .expect("a reference to a variable");
    agent.events.env.push((variable.to_owned(), canary.clone()));
    agent
        .locations
        .options
        .entry(name)
        .or_default()
        .push((option, reference));
    agent.locations.canary = Some(canary);
    write_config(agent);
}

#[when(expr = "the plan {string} runs")]
async fn the_plan_runs(agent: &mut Agent, plan: String) {
    if !agent.events.serving() {
        events_steps::serve(agent).await;
    }
    events_steps::connect_page(agent).await;
    let page = agent.events.pages_count() - 1;
    let id = start_backup(agent, &plan);
    wait_ended(agent, &id).await;
    agent.locations.heard = agent
        .events
        .heard(
            page,
            &[
                "operation-succeeded",
                "operation-failed",
                "operation-cancelled",
            ],
        )
        .await;
    let (status, body) = exchange(agent, "GET", &format!("/api/v1/operations/{id}"), "");
    assert_eq!(status, 200, "{body}");
    assert!(
        body.contains("\"status\":\"succeeded\""),
        "the backup succeeded: {body}"
    );
    agent.locations.operation = Some(id);
}

#[then("the canary appears in no log line, event, operation record or page")]
async fn canary_nowhere(agent: &mut Agent) {
    let canary = agent.locations.canary.clone().expect("a canary");
    let id = agent.locations.operation.clone().expect("an operation ran");
    let mut seen = vec![
        ("the daemon's log", agent.events.whole_log()),
        ("the events", agent.locations.heard.clone()),
    ];
    for (what, route) in [
        ("the operation", format!("/api/v1/operations/{id}")),
        ("the history", "/api/v1/operations?limit=50".to_owned()),
        ("the Repositories page", "/api/v1/repositories".to_owned()),
    ] {
        let (status, body) = exchange(agent, "GET", &route, "");
        assert_eq!(status, 200, "{what}: {body}");
        seen.push((what, body));
    }
    for (what, text) in &seen {
        assert!(!text.contains(&canary), "{what} shows the canary");
    }
    // The operation log and every log file the daemon keeps.
    let data = Events::data_dir(agent);
    let files = blocking(move || every_file(&data)).await;
    for file in files {
        let bytes = fs::read(&file).expect("a file of the data folder");
        assert!(
            !bytes
                .windows(canary.len())
                .any(|window| window == canary.as_bytes()),
            "{} holds the canary",
            file.display()
        );
    }
}

fn every_file(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(every_file(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[then(expr = "the Repositories page shows the reference {string} and never its value")]
fn page_shows_the_reference(agent: &mut Agent, reference: String) {
    let canary = agent.locations.canary.clone().expect("a canary");
    let (status, body) = exchange(agent, "GET", "/api/v1/repositories", "");
    assert_eq!(status, 200, "{body}");
    let listed: serde_json::Value = serde_json::from_str(&body).expect("the list is JSON");
    let shown = listed.to_string();
    let reference = serde_json::Value::String(reference).to_string();
    assert!(
        shown.contains(reference.trim_matches('"')),
        "the page shows the reference"
    );
    assert!(!shown.contains(&canary), "the page shows the canary");
}

#[when(expr = "the plan {string} is exported as a rustic profile")]
async fn exported(agent: &mut Agent, plan: String) {
    super::interop_steps::run(agent, &["export-profile", "--plan", &plan]).await;
    let ran = super::interop_steps::ran(agent);
    assert!(ran.success, "the export was refused: {}", ran.stderr);
}

#[then(expr = "the profile's repository is the location {string}")]
fn profile_repository(agent: &mut Agent, name: String) {
    let profile: toml::Table = super::interop_steps::ran(agent)
        .stdout
        .parse()
        .expect("the profile is TOML");
    let repository = profile
        .get("repository")
        .and_then(|table| table.get("repository"))
        .and_then(toml::Value::as_str)
        .expect("the profile names a repository");
    assert_eq!(repository, path_text(&place(agent, &name)));
    assert!(
        !super::interop_steps::ran(agent)
            .stdout
            .contains("locations"),
        "the profile holds no further location"
    );
}

#[then(expr = "the export says that {int} further location was left out")]
fn export_says_left_out(agent: &mut Agent, count: usize) {
    let said = read_as_shown(&super::interop_steps::ran(agent).stderr);
    let words = format!("{count} further location was left out");
    assert!(said.contains(&words), "the export says {words:?}: {said}");
}
