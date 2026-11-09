//! Writing a configuration as the text of a config file: what it leaves out,
//! what it quotes, and what it refuses to write.

mod support;

use std::collections::BTreeMap;
use std::path::Path;

use app::ports::Problem;
use config_adapter::{Schemas, parse, render};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, HookEntry, Hooks, Maintenance, OnFailure, Plan,
    RepositoryConfig, RepositorySettings, Source,
};
use domain::secrets::{FileRead, SecretRef, SecretValue};
use domain::{PlanId, RepositoryAlias};
use support::*;

fn file() -> &'static Path {
    Path::new(FILE)
}

fn alias(text: &str) -> RepositoryAlias {
    RepositoryAlias::parse(text).expect("an alias")
}

fn repository(password: SecretRef) -> RepositoryConfig {
    RepositoryConfig {
        settings: RepositorySettings {
            repository: "/srv/demo-repo".to_owned(),
            repo_hot: None,
            password,
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Maintenance::None,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    }
}

fn config(password: SecretRef, plans: Vec<Plan>, hooks: Hooks) -> AgentConfig {
    AgentConfig::new(
        AgentSettings::default(),
        hooks,
        BTreeMap::from([(alias("nas"), repository(password))]),
        plans,
    )
    .expect("a valid configuration")
}

fn home() -> Plan {
    Plan::new(
        PlanId::parse("home").expect("an ID"),
        alias("nas"),
        vec![Source::parse("/home/demo").expect("a source")],
    )
}

fn table_of(text: &str) -> toml::Table {
    text.parse()
        .unwrap_or_else(|error| panic!("not TOML: {error}\n{text}"))
}

fn keys(table: &toml::Value) -> Vec<&str> {
    table
        .as_table()
        .expect("a table")
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn the_empty_configuration_is_written_as_its_schema_alone() {
    let text = render(file(), &AgentConfig::default()).expect("written");
    assert_eq!(table_of(&text), table_of("schema = 1\n"));
}

#[test]
fn what_is_left_to_its_default_is_not_written() {
    let config = config(
        SecretRef::Env("DEMO_PASSWORD".to_owned()),
        vec![home()],
        Hooks::default(),
    );
    let table = table_of(&render(file(), &config).expect("written"));
    assert_eq!(
        table.keys().map(String::as_str).collect::<Vec<_>>(),
        ["schema", "repositories", "plans"]
    );
    assert_eq!(
        keys(&table["repositories"]["nas"]),
        ["repository", "password-env"],
        "no maintenance, no group-by, no hooks and no options"
    );
    let plans = table["plans"].as_array().expect("plans");
    assert_eq!(plans.len(), 1);
    assert_eq!(keys(&plans[0]), ["id", "repository", "sources"]);
}

#[test]
fn a_schedule_writes_only_what_differs_from_its_defaults() {
    let mut plan = home();
    plan.schedule = Some(domain::planning::Schedule::new(
        domain::planning::CronExpression::parse("0 * * * *").expect("a cron"),
    ));
    let config = config(SecretRef::Env("X".to_owned()), vec![plan], Hooks::default());
    let table = table_of(&render(file(), &config).expect("written"));
    assert_eq!(keys(&table["plans"][0]["schedule"]), ["cron"]);
}

#[test]
fn a_hook_is_a_string_unless_it_says_what_to_do_on_failure() {
    let hooks = Hooks {
        run_before: vec![HookEntry::parse("echo 'two words' \"and more\"").expect("a hook")],
        run_after: vec![
            HookEntry::new("/opt/demo tools/notify", ["done", "it's"], OnFailure::Warn)
                .expect("a hook"),
        ],
        run_failed: vec![
            HookEntry::new("true", Vec::<String>::new(), OnFailure::Error).expect("a hook"),
        ],
        run_finally: Vec::new(),
    };
    let config = config(SecretRef::Env("X".to_owned()), vec![home()], hooks.clone());
    let text = render(file(), &config).expect("written");
    let table = table_of(&text);
    let written = &table["hooks"];
    assert_eq!(keys(written), ["run-before", "run-after", "run-failed"]);
    assert_eq!(
        written["run-before"][0].as_str(),
        Some("echo 'two words' 'and more'"),
        "a string is the command line as shell words, joined"
    );
    assert_eq!(written["run-failed"][0].as_str(), Some("true"));
    let notify = &written["run-after"][0];
    assert_eq!(notify["command"].as_str(), Some("/opt/demo tools/notify"));
    assert_eq!(notify["args"].as_array().map(Vec::len), Some(2));
    assert_eq!(notify["on-failure"].as_str(), Some("warn"));
    let again = parse(file(), &text, &Schemas::production()).expect("read back");
    assert_eq!(again.config.hooks(), &hooks);
}

#[test]
fn every_way_of_giving_a_password_is_written_as_its_own_key() {
    let rows = [
        (
            SecretRef::Inline(SecretValue::new("demo-password")),
            "password",
        ),
        (
            SecretRef::Command(
                domain::secrets::CommandLine::parse("get-demo --quiet").expect("a command"),
            ),
            "password-command",
        ),
        (
            SecretRef::Keyring(
                domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry"),
            ),
            "password-keyring",
        ),
        (
            SecretRef::File {
                path: "/home/demo/.pw".to_owned(),
                read: FileRead::FirstLine,
            },
            "password-file",
        ),
        (SecretRef::Env("DEMO_PASSWORD".to_owned()), "password-env"),
    ];
    for (password, key) in rows {
        let config = config(password.clone(), Vec::new(), Hooks::default());
        let text = render(file(), &config).expect("written");
        let table = table_of(&text);
        assert_eq!(
            keys(&table["repositories"]["nas"]),
            ["repository", key],
            "{key}"
        );
        let again = parse(file(), &text, &Schemas::production()).expect("read back");
        assert_eq!(again.config, config, "{key}");
    }
}

#[test]
fn a_password_the_file_cannot_say_is_refused_not_written_wrong() {
    // `${file:/path}` reads a whole file; a `password-file` reads its first line.
    let whole = SecretRef::File {
        path: "/home/demo/.pw".to_owned(),
        read: FileRead::Whole,
    };
    let config = config(whole, Vec::new(), Hooks::default());
    let error = invalid(render(file(), &config));
    assert_eq!(error.problem, Problem::Unwritable);
    assert_eq!(error.key.as_deref(), Some("repositories.nas.password"));
    assert_eq!(error.at, None);
}

#[test]
fn text_that_needs_quoting_is_written_so_that_it_reads_back_unchanged() {
    let mut plan = home();
    plan.label = Some("line one\nline \"two\" \\ #3\ttab \u{2013} caf\u{e9} \u{1f4be}".to_owned());
    plan.sources = vec![
        Source::parse("/srv/demo data/it's \"here\"").expect("a source"),
        Source::parse("/home/demo/\u{65e5}\u{672c}").expect("a source"),
    ];
    let config = config(
        SecretRef::Inline(SecretValue::new("pass \"word\" #1\n'two'")),
        vec![plan],
        Hooks::default(),
    );
    let text = render(file(), &config).expect("written");
    let again = parse(file(), &text, &Schemas::production()).expect("read back");
    assert_eq!(again.config, config);
}

#[test]
fn a_secret_is_written_where_the_user_put_it_and_nowhere_else() {
    let config = config(
        SecretRef::Inline(SecretValue::new("CANARY-inline")),
        vec![home()],
        Hooks::default(),
    );
    let text = render(file(), &config).expect("written");
    assert_eq!(
        text.matches("CANARY-inline").count(),
        1,
        "the password is in the file once, as `password`: {text}"
    );
    assert_eq!(
        table_of(&text)["repositories"]["nas"]["password"].as_str(),
        Some("CANARY-inline")
    );
}
