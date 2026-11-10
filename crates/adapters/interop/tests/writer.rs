//! What the writer makes of one plan (technical design §6.2): a profile the
//! stock rustic accepts, scoped to the plan, that holds a way to read each
//! secret and never the secret.

mod common;

use std::collections::BTreeMap;

use app::ports::{ExportRefusal, PlanExport, ProfileNotice, ProfileReader, ProfileWriter};
use common::{Scratch, nas};
use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::planning::{
    HookEntry, Hooks, Hostname, KeepCount, KeepWithin, OnFailure, Plan, RepositoryConfig,
    RepositorySettings, Source,
};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef, SecretValue};
use domain::{AgentId, PlanId, RepositoryAlias, Uuid};
use interop::RusticProfileWriter;

const AGENT: &str = "00000000-0000-0000-0000-000000000001";

fn id(text: &str) -> PlanId {
    PlanId::parse(text).expect("a plan ID")
}

fn agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(1))
}

fn entry(text: &str) -> HookEntry {
    HookEntry::parse(text).expect("a hook")
}

fn file_password() -> SecretRef {
    SecretRef::File {
        path: "/srv/demo-password".to_owned(),
        read: FileRead::FirstLine,
    }
}

fn settings(location: &str, password: SecretRef) -> RepositorySettings {
    RepositorySettings {
        repository: location.to_owned(),
        repo_hot: None,
        password,
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    }
}

/// The plan `home` of `/home/demo`, in a repository in `/srv/demo-repo` whose
/// password is in a file, on an agent that has no hostname and no hooks.
fn export() -> PlanExport {
    PlanExport {
        plan: Plan::new(
            id("home"),
            RepositoryAlias::parse("nas").expect("an alias"),
            vec![Source::parse("/home/demo").expect("a source")],
        ),
        repository: RepositoryConfig {
            settings: settings("/srv/demo-repo", file_password()),
            maintenance: domain::planning::Maintenance::None,
            group_by: domain::planning::GroupBy::default(),
            hooks: Hooks::default(),
        },
        agent_hooks: Hooks::default(),
        agent_hostname: None,
        tags: PlanTags::new(id("home"), agent()),
    }
}

fn render(export: &PlanExport) -> Result<app::ports::RenderedProfile, ExportRefusal> {
    RusticProfileWriter::new().render(export)
}

fn text(export: &PlanExport) -> String {
    render(export)
        .unwrap_or_else(|refusal| panic!("the export should be made: {refusal:?}"))
        .text
}

fn parsed(text: &str) -> toml::Value {
    toml::from_str(text).unwrap_or_else(|error| panic!("not TOML: {error}\n{text}"))
}

/// The value at a dotted path, a number being an index of an array.
fn at<'a>(value: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    path.split('.').try_fold(value, |value, key| match value {
        toml::Value::Table(table) => table.get(key),
        toml::Value::Array(items) => items.get(key.parse::<usize>().ok()?),
        _ => None,
    })
}

fn string(value: &str) -> toml::Value {
    toml::Value::String(value.to_owned())
}

fn strings(values: &[&str]) -> toml::Value {
    toml::Value::Array(values.iter().map(|value| string(value)).collect())
}

fn assert_has(text: &str, path: &str, expected: toml::Value) {
    assert_eq!(at(&parsed(text), path), Some(&expected), "{path}\n{text}");
}

fn assert_hasnt(text: &str, path: &str) {
    assert_eq!(at(&parsed(text), path), None, "{path}\n{text}");
}

fn refused(export: &PlanExport) -> ExportRefusal {
    match render(export) {
        Ok(profile) => panic!("the export should be refused, got\n{}", profile.text),
        Err(refusal) => refusal,
    }
}

// --- The whole profile ------------------------------------------------------

#[test]
fn a_plan_is_written_as_this_profile_exactly() {
    let mut export = export();
    export.agent_hostname = Some(Hostname::parse("demo-laptop").expect("a hostname"));
    export.agent_hooks.run_before = vec![entry("echo agent")];
    export.repository.hooks.run_before = vec![entry("echo repository")];
    export
        .repository
        .settings
        .options
        .insert("region".to_owned(), "eu-demo-1".to_owned());
    export.plan.label = Some("demo home".to_owned());
    export
        .plan
        .sources
        .push(Source::parse("/srv/demo data").expect("a source"));
    export.plan.tags = vec![UserTag::new("laptop").expect("a tag")];
    export.plan.filter.git_ignore = true;
    export.plan.filter.exclude_if_present = vec![".nobackup".to_owned()];
    export.plan.hooks.run_before = vec![entry("echo plan-before")];
    export.plan.hooks.run_after = vec![entry("echo plan-after")];
    export.plan.retention.keep_last = Some(KeepCount::new(3).expect("a count"));
    export.plan.retention.keep_daily = Some(KeepCount::new(7).expect("a count"));
    let expected = format!(
        "\
# A rustic profile for the Ladderback plan home, written by Ladderback.

[global.hooks]
run-before = [\"echo agent\"]

[repository]
repository = \"/srv/demo-repo\"
password-file = \"/srv/demo-password\"

[repository.options]
region = \"eu-demo-1\"

[repository.hooks]
run-before = [\"echo repository\"]

[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\", \"/srv/demo data\"]
label = \"demo home\"
host = \"demo-laptop\"
tags = [\"laptop\", \"ladderback-plan:home\", \"ladderback-agent:{AGENT}\"]
git-ignore = true
exclude-if-present = [\".nobackup\"]

[backup.snapshots.hooks]
run-before = [\"echo plan-before\"]
run-after = [\"echo plan-after\"]

[forget]
group-by = \"\"
keep-last = 3
keep-daily = 7

[snapshot-filter]
filter-tags = [\"ladderback-plan:home,ladderback-agent:{AGENT}\"]
"
    );
    let written = render(&export).expect("the export is made");
    assert_eq!(written.text, expected);
    assert!(written.notices.is_empty());
}

#[test]
fn the_same_plan_is_written_the_same_way_every_time() {
    let export = export();
    assert_eq!(text(&export), text(&export));
}

#[test]
fn a_plan_with_nothing_set_is_written_with_what_scopes_it() {
    let text = text(&export());
    assert_has(&text, "backup.snapshots.0.name", string("home"));
    assert_has(
        &text,
        "backup.snapshots.0.sources",
        strings(&["/home/demo"]),
    );
    assert_has(
        &text,
        "backup.snapshots.0.tags",
        strings(&["ladderback-plan:home", &format!("ladderback-agent:{AGENT}")]),
    );
    assert_has(&text, "forget.group-by", string(""));
    assert_has(
        &text,
        "snapshot-filter.filter-tags",
        strings(&[&format!("ladderback-plan:home,ladderback-agent:{AGENT}")]),
    );
    for absent in [
        "global",
        "backup.snapshots.0.label",
        "backup.snapshots.0.host",
        "backup.snapshots.0.hooks",
        "backup.snapshots.0.git-ignore",
        "backup.snapshots.0.globs",
        "repository.hooks",
        "repository.options",
        "repository.repo-hot",
        "forget.prune",
        "forget.keep-last",
    ] {
        assert_hasnt(&text, absent);
    }
}

#[test]
fn what_ladderback_does_by_itself_is_not_in_the_profile() {
    let mut export = export();
    export.plan.schedule = Some(domain::planning::Schedule::new(
        domain::planning::CronExpression::parse("0 * * * *").expect("a schedule"),
    ));
    export.plan.retry = Some(
        domain::planning::RetryPolicy::new(3, domain::SignedDuration::from_mins(10))
            .expect("a retry"),
    );
    export.repository.maintenance = domain::planning::Maintenance::Local;
    let text = text(&export);
    for left_out in [
        "schedule",
        "cron",
        "retry",
        "backoff",
        "maintenance",
        "prune",
    ] {
        assert!(!text.contains(left_out), "{left_out}\n{text}");
    }
}

// --- Where a snapshot is from -----------------------------------------------

#[test]
fn the_host_is_the_plans_own_else_the_agents_else_none() {
    let host = |text: &str| Some(Hostname::parse(text).expect("a hostname"));
    let mut export = export();
    assert_hasnt(&text(&export), "backup.snapshots.0.host");
    export.agent_hostname = host("agent-host");
    assert_has(
        &text(&export),
        "backup.snapshots.0.host",
        string("agent-host"),
    );
    export.plan.host = host("plan-host");
    assert_has(
        &text(&export),
        "backup.snapshots.0.host",
        string("plan-host"),
    );
}

#[test]
fn the_plans_own_tags_come_first_and_ladderbacks_last() {
    let mut export = export();
    export.plan.tags = ["b", "a"]
        .map(|tag| UserTag::new(tag).expect("a tag"))
        .to_vec();
    assert_has(
        &text(&export),
        "backup.snapshots.0.tags",
        strings(&[
            "b",
            "a",
            "ladderback-plan:home",
            &format!("ladderback-agent:{AGENT}"),
        ]),
    );
}

// --- What it keeps ----------------------------------------------------------

#[test]
fn every_retention_rule_is_written_under_its_key() {
    let mut export = export();
    let count = |n| Some(KeepCount::new(n).expect("a count"));
    let within = |text: &str| Some(KeepWithin::parse(text).expect("a span"));
    let retention = &mut export.plan.retention;
    retention.keep_last = count(1);
    retention.keep_minutely = count(2);
    retention.keep_hourly = count(3);
    retention.keep_daily = count(4);
    retention.keep_weekly = count(5);
    retention.keep_monthly = count(6);
    retention.keep_quarter_yearly = count(7);
    retention.keep_half_yearly = count(8);
    retention.keep_yearly = count(-1);
    retention.keep_within = within("1y");
    retention.keep_within_minutely = within("30m");
    retention.keep_within_hourly = within("12h");
    retention.keep_within_daily = within("7d");
    retention.keep_within_weekly = within("4w");
    retention.keep_within_monthly = within("6mo");
    retention.keep_within_quarter_yearly = within("2y");
    retention.keep_within_half_yearly = within("3y");
    retention.keep_within_yearly = within("10y");
    let text = text(&export);
    let integer = |n| toml::Value::Integer(n);
    for (key, expected) in [
        ("keep-last", integer(1)),
        ("keep-minutely", integer(2)),
        ("keep-hourly", integer(3)),
        ("keep-daily", integer(4)),
        ("keep-weekly", integer(5)),
        ("keep-monthly", integer(6)),
        ("keep-quarter-yearly", integer(7)),
        ("keep-half-yearly", integer(8)),
        ("keep-yearly", integer(-1)),
    ] {
        assert_has(&text, &format!("forget.{key}"), expected);
    }
    // The spans are written as the plan reads them back: whatever words jiff
    // gives, the same span comes out.
    let value = parsed(&text);
    for (key, original) in [
        ("keep-within", "1y"),
        ("keep-within-minutely", "30m"),
        ("keep-within-hourly", "12h"),
        ("keep-within-daily", "7d"),
        ("keep-within-weekly", "4w"),
        ("keep-within-monthly", "6mo"),
        ("keep-within-quarter-yearly", "2y"),
        ("keep-within-half-yearly", "3y"),
        ("keep-within-yearly", "10y"),
    ] {
        let written = at(&value, &format!("forget.{key}"))
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("{key} is not written\n{text}"));
        assert_eq!(
            KeepWithin::parse(written).expect("a span"),
            KeepWithin::parse(original).expect("a span"),
            "{key}: {written}"
        );
    }
}

// --- What it leaves out ------------------------------------------------------

#[test]
fn a_filter_is_written_only_for_what_it_sets() {
    let mut export = export();
    let filter = &mut export.plan.filter;
    filter.globs = vec!["*.tmp".to_owned()];
    filter.iglobs = vec!["*.BAK".to_owned()];
    filter.glob_files = vec!["/etc/globs".to_owned()];
    filter.iglob_files = vec!["/etc/iglobs".to_owned()];
    filter.no_require_git = true;
    filter.custom_ignorefiles = vec![".ignore".to_owned()];
    filter.exclude_if_xattr = vec!["user.skip".to_owned()];
    filter.one_file_system = true;
    let text = text(&export);
    for (key, expected) in [
        ("globs", strings(&["*.tmp"])),
        ("iglobs", strings(&["*.BAK"])),
        ("glob-files", strings(&["/etc/globs"])),
        ("iglob-files", strings(&["/etc/iglobs"])),
        ("no-require-git", toml::Value::Boolean(true)),
        ("custom-ignorefiles", strings(&[".ignore"])),
        ("exclude-if-xattr", strings(&["user.skip"])),
        ("one-file-system", toml::Value::Boolean(true)),
    ] {
        assert_has(&text, &format!("backup.snapshots.0.{key}"), expected);
    }
    for unset in ["git-ignore", "exclude-if-present"] {
        assert_hasnt(&text, &format!("backup.snapshots.0.{unset}"));
    }
}

// --- Hooks --------------------------------------------------------------------

#[test]
fn hooks_are_written_where_rustic_reads_each_level() {
    let mut export = export();
    export.agent_hooks.run_failed = vec![entry("echo agent-failed")];
    export.repository.hooks.run_finally = vec![entry("echo repository-finally")];
    export.plan.hooks.run_before = vec![entry("echo one"), entry("echo two")];
    let text = text(&export);
    assert_has(
        &text,
        "global.hooks.run-failed",
        strings(&["echo agent-failed"]),
    );
    assert_has(
        &text,
        "repository.hooks.run-finally",
        strings(&["echo repository-finally"]),
    );
    assert_has(
        &text,
        "backup.snapshots.0.hooks.run-before",
        strings(&["echo one", "echo two"]),
    );
    assert_hasnt(&text, "global.hooks.run-before");
}

#[test]
fn a_hook_that_does_not_fail_the_operation_is_written_as_a_table() {
    let mut export = export();
    export.plan.hooks.run_before = vec![
        HookEntry::new("/opt/demo tools/run", ["a b", "--flag"], OnFailure::Warn).expect("a hook"),
        HookEntry::new("true", Vec::<String>::new(), OnFailure::Ignore).expect("a hook"),
        HookEntry::new("false", ["x"], OnFailure::Error).expect("a hook"),
    ];
    let text = text(&export);
    let value = parsed(&text);
    let hooks = at(&value, "backup.snapshots.0.hooks.run-before")
        .and_then(toml::Value::as_array)
        .expect("a list");
    let table = |command: &str, args: &[&str], on_failure: &str| {
        let mut table = toml::map::Map::new();
        table.insert("command".to_owned(), string(command));
        table.insert("args".to_owned(), strings(args));
        table.insert("on-failure".to_owned(), string(on_failure));
        toml::Value::Table(table)
    };
    assert_eq!(
        hooks[0],
        table("/opt/demo tools/run", &["a b", "--flag"], "warn")
    );
    assert_eq!(hooks[1], table("true", &[], "ignore"));
    // A hook that fails the operation is the plain string, as rustic writes it.
    assert_eq!(hooks[2], string("false x"));
}

#[test]
fn a_command_is_written_so_that_it_splits_into_the_same_words() {
    let words = [
        "plain",
        "with space",
        "it's",
        "say \"hi\"",
        "back\\slash",
        "a#b",
        "$HOME",
        "tab\there",
        "new\nline",
        "",
        "ünï",
        "-n",
        "a=b",
    ];
    for word in words {
        let mut export = export();
        export.plan.hooks.run_before =
            vec![HookEntry::new("run", [word], OnFailure::Error).expect("a hook")];
        let written = text(&export);
        let value = parsed(&written);
        let line = at(&value, "backup.snapshots.0.hooks.run-before.0")
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("{word:?}\n{written}"));
        let read = CommandLine::parse(line).expect("a command line");
        assert_eq!(read.program(), "run", "{word:?}: {line}");
        assert_eq!(read.args(), [word], "{word:?}: {line}");
    }
}

// --- Text ------------------------------------------------------------------------

#[test]
fn any_text_is_written_so_that_toml_reads_it_back() {
    let odd = "quote \" backslash \\ tab \t newline \n cr \r bell \u{7} nul-free \u{1b} del \u{7f} \
               ünïcödé ✓ 𝄞 ${not} $${a}";
    let mut export = export();
    export.plan.label = Some(odd.to_owned());
    export.plan.sources = vec![Source::parse(&format!("/{odd}")).expect("a source")];
    export.plan.filter.globs = vec![odd.to_owned()];
    let text = text(&export);
    assert_has(&text, "backup.snapshots.0.label", string(odd));
    assert_has(&text, "backup.snapshots.0.globs", strings(&[odd]));
    assert_has(
        &text,
        "backup.snapshots.0.sources",
        strings(&[&format!("/{odd}")]),
    );
}

// --- The repository --------------------------------------------------------------

#[test]
fn a_location_that_has_an_escaped_reference_is_written_as_the_text_it_stands_for() {
    let mut export = export();
    export.repository.settings.repository = "/srv/$${demo}".to_owned();
    export.repository.settings.repo_hot = Some("/hot/$${demo}".to_owned());
    let text = text(&export);
    assert_has(&text, "repository.repository", string("/srv/${demo}"));
    assert_has(&text, "repository.repo-hot", string("/hot/${demo}"));
}

#[test]
fn a_location_that_is_a_reference_is_refused_naming_the_key() {
    for (location, hot, key) in [
        ("${DEMO_REPOSITORY}", None, "repository"),
        ("s3:${file:/srv/demo-endpoint}", None, "repository"),
        ("/srv/demo-repo", Some("${DEMO_HOT}"), "repo-hot"),
        ("/srv/demo-repo/${", None, "repository"),
    ] {
        let mut export = export();
        export.repository.settings.repository = location.to_owned();
        export.repository.settings.repo_hot = hot.map(str::to_owned);
        assert_eq!(
            refused(&export),
            ExportRefusal::Reference {
                key: key.to_owned()
            },
            "{location} {hot:?}"
        );
    }
}

#[test]
fn an_option_that_is_a_reference_is_left_out_with_the_variable_to_set_instead() {
    let mut export = export();
    let settings = &mut export.repository.settings;
    settings
        .options
        .insert("region".to_owned(), "eu-demo-1".to_owned());
    settings
        .options
        .insert("token".to_owned(), "${DEMO_TOKEN}".to_owned());
    settings
        .options
        .insert("no-cache".to_owned(), "${file:/srv/demo}".to_owned());
    settings
        .options
        .insert("sas_token".to_owned(), "${DEMO_SAS}".to_owned());
    settings
        .options_hot
        .insert("endpoint".to_owned(), "${DEMO_HOT}".to_owned());
    settings
        .options_cold
        .insert("root_path".to_owned(), "${DEMO_COLD}".to_owned());
    let written = render(&export).expect("the export is made");
    assert_has(
        &written.text,
        "repository.options.region",
        string("eu-demo-1"),
    );
    for left_out in [
        "token",
        "DEMO",
        "no-cache",
        "sas_token",
        "endpoint",
        "root_path",
        "options-hot",
    ] {
        assert!(
            !written.text.contains(left_out),
            "{left_out}\n{}",
            written.text
        );
    }
    let left_out = |option: &str, variable: &str| ProfileNotice::OptionLeftOut {
        option: option.to_owned(),
        variable: variable.to_owned(),
    };
    let mut notices = written.notices;
    notices.sort_by_key(|notice| format!("{notice:?}"));
    let mut expected = vec![
        left_out("token", "RUSTIC_REPO_OPT_TOKEN"),
        left_out("no-cache", "RUSTIC_REPO_OPT_NO_CACHE"),
        left_out("sas_token", "OPENDAL_SAS_TOKEN"),
        left_out("endpoint", "RUSTIC_REPO_OPTHOT_ENDPOINT"),
        left_out("root_path", "OPENDALCOLD_ROOT_PATH"),
    ];
    expected.sort_by_key(|notice| format!("{notice:?}"));
    assert_eq!(notices, expected);
}

#[test]
fn a_secret_option_written_in_the_config_is_left_out_like_a_reference() {
    let mut export = export();
    export
        .repository
        .settings
        .options
        .insert("secret_access_key".to_owned(), "CANARY-option".to_owned());
    export
        .repository
        .settings
        .options
        .insert("region".to_owned(), "eu-demo-1".to_owned());
    let written = render(&export).expect("the export is made");
    assert!(!written.text.contains("CANARY"), "{}", written.text);
    assert_has(
        &written.text,
        "repository.options.region",
        string("eu-demo-1"),
    );
    assert_eq!(
        written.notices,
        [ProfileNotice::OptionLeftOut {
            option: "secret_access_key".to_owned(),
            variable: "OPENDAL_SECRET_ACCESS_KEY".to_owned(),
        }]
    );
}

#[test]
fn an_option_whose_variable_rustic_could_not_name_is_refused() {
    for name in ["Mixed_Case", "kebab-and_snake", "with space", "", "ünï"] {
        let mut export = export();
        export
            .repository
            .settings
            .options
            .insert(name.to_owned(), "${DEMO}".to_owned());
        assert_eq!(
            refused(&export),
            ExportRefusal::Reference {
                key: format!("options.{name}")
            },
            "{name:?}"
        );
    }
}

#[test]
fn the_options_of_the_hot_and_cold_repositories_are_written_in_their_own_tables() {
    let mut export = export();
    export
        .repository
        .settings
        .options_hot
        .insert("region".to_owned(), "hot-1".to_owned());
    export
        .repository
        .settings
        .options_cold
        .insert("region".to_owned(), "cold-1".to_owned());
    export.repository.settings.repo_hot = Some("s3:hot".to_owned());
    let text = text(&export);
    assert_has(&text, "repository.options-hot.region", string("hot-1"));
    assert_has(&text, "repository.options-cold.region", string("cold-1"));
    assert_has(&text, "repository.repo-hot", string("s3:hot"));
    assert_hasnt(&text, "repository.options");
}

// --- The password ------------------------------------------------------------------

fn password_of(secret: SecretRef) -> String {
    let mut export = export();
    export.repository.settings.password = secret;
    text(&export)
}

#[test]
fn a_password_in_a_file_is_the_file() {
    let text = password_of(SecretRef::File {
        path: "/srv/demo password".to_owned(),
        read: FileRead::FirstLine,
    });
    assert_has(
        &text,
        "repository.password-file",
        string("/srv/demo password"),
    );
    assert_hasnt(&text, "repository.password-command");
}

#[test]
fn a_password_command_is_written_as_the_command() {
    let text = password_of(SecretRef::Command(
        CommandLine::new("pass", ["show", "demo store/nas"]).expect("a command"),
    ));
    assert_has(
        &text,
        "repository.password-command",
        string("pass show 'demo store/nas'"),
    );
    assert_hasnt(&text, "repository.password-file");
}

#[test]
fn a_password_in_a_variable_is_read_with_printenv() {
    let text = password_of(SecretRef::Env("DEMO_PASSWORD".to_owned()));
    assert_has(
        &text,
        "repository.password-command",
        string("printenv DEMO_PASSWORD"),
    );
}

#[test]
fn a_password_in_the_keyring_is_read_with_the_command_of_the_platform() {
    let entry = KeyringEntry::parse("demo service/it's a name").expect("an entry");
    let text = password_of(SecretRef::Keyring(entry));
    let value = parsed(&text);
    let command = at(&value, "repository.password-command")
        .and_then(toml::Value::as_str)
        .expect("a command");
    let words = CommandLine::parse(command).expect("a command line");
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        (
            "security",
            vec![
                "find-generic-password",
                "-s",
                "demo service",
                "-a",
                "it's a name",
                "-w",
            ],
        )
    } else {
        (
            "secret-tool",
            vec![
                "lookup",
                "service",
                "demo service",
                "username",
                "it's a name",
            ],
        )
    };
    assert_eq!(words.program(), program, "{command}");
    assert_eq!(words.args(), args, "{command}");
}

#[test]
fn a_password_in_the_config_is_refused_and_never_shown() {
    let mut export = export();
    export.repository.settings.password =
        SecretRef::Inline(SecretValue::new("CANARY-inline-password"));
    let refusal = refused(&export);
    assert_eq!(refusal, ExportRefusal::InlineSecret);
    assert!(!format!("{refusal:?}{refusal}").contains("CANARY"));
}

// --- What reads it back ---------------------------------------------------------------

#[tokio::test]
async fn the_reader_reads_the_plan_back_apart_from_ladderbacks_own_tags() {
    let mut export = export();
    export.agent_hostname = Some(Hostname::parse("demo-laptop").expect("a hostname"));
    export.agent_hooks.run_before = vec![entry("echo agent")];
    export.repository.hooks.run_after = vec![entry("echo repository")];
    export
        .repository
        .settings
        .options
        .insert("region".to_owned(), "eu-demo-1".to_owned());
    export.plan.label = Some("demo home".to_owned());
    export.plan.tags = vec![UserTag::new("laptop").expect("a tag")];
    export.plan.hooks.run_before = vec![
        entry("echo plan"),
        HookEntry::new("run", ["a b"], OnFailure::Warn).expect("a hook"),
    ];
    export.plan.filter.git_ignore = true;
    export.plan.retention.keep_last = Some(KeepCount::new(3).expect("a count"));
    let scratch = Scratch::new();
    let file = scratch.profile("nas.toml", &text(&export));
    let read = scratch
        .reader()
        .read(&file, &nas())
        .await
        .expect("the profile is read");
    assert_eq!(read.plans.len(), 1);
    let mut expected = export.plan.clone();
    // The agent's hostname is the plan's host, the profile having no other.
    expected.host = export.agent_hostname.clone();
    assert_eq!(read.plans[0], expected);
    assert_eq!(read.repository.settings, export.repository.settings);
    assert_eq!(read.repository.hooks, export.repository.hooks);
    assert_eq!(read.hooks, export.agent_hooks);
    assert_eq!(
        read.notices,
        [
            ProfileNotice::TagDropped {
                tag: "ladderback-plan:home".to_owned()
            },
            ProfileNotice::TagDropped {
                tag: format!("ladderback-agent:{AGENT}")
            }
        ]
    );
}

#[test]
fn every_key_written_is_a_key_rustic_has() {
    // The reader's lists of rustic 0.11.4's keys are pinned against
    // `rustic_core` by its own parity test.
    let mut export = export();
    export.agent_hostname = Some(Hostname::parse("demo-laptop").expect("a hostname"));
    export.agent_hooks.run_before = vec![entry("echo agent")];
    export.repository.hooks.run_before = vec![entry("echo repository")];
    export.repository.settings.repo_hot = Some("/hot".to_owned());
    export
        .repository
        .settings
        .options
        .insert("a".to_owned(), "b".to_owned());
    export
        .repository
        .settings
        .options_hot
        .insert("a".to_owned(), "b".to_owned());
    export
        .repository
        .settings
        .options_cold
        .insert("a".to_owned(), "b".to_owned());
    export.plan.label = Some("label".to_owned());
    export.plan.hooks.run_finally = vec![entry("echo plan")];
    export.plan.retention.keep_within = Some(KeepWithin::parse("1y").expect("a span"));
    export.plan.filter.git_ignore = true;
    let value = parsed(&text(&export));
    let keys = |path: &str| -> Vec<String> {
        at(&value, path)
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("no table at {path}"))
            .keys()
            .cloned()
            .collect()
    };
    let known = [
        ("global", vec!["hooks"]),
        (
            "repository",
            vec![
                "repository",
                "repo-hot",
                "password-file",
                "options",
                "options-hot",
                "options-cold",
                "hooks",
            ],
        ),
        ("snapshot-filter", vec!["filter-tags"]),
        ("backup", vec!["snapshots"]),
        (
            "backup.snapshots.0",
            vec![
                "name",
                "sources",
                "label",
                "host",
                "tags",
                "git-ignore",
                "hooks",
            ],
        ),
        ("forget", vec!["group-by", "keep-within"]),
        ("global.hooks", vec!["run-before"]),
        ("repository.hooks", vec!["run-before"]),
        ("backup.snapshots.0.hooks", vec!["run-finally"]),
    ];
    for (path, expected) in known {
        assert_eq!(keys(path), expected, "{path}");
    }
    let top: Vec<_> = value.as_table().expect("a table").keys().cloned().collect();
    assert_eq!(
        top,
        [
            "global",
            "repository",
            "backup",
            "forget",
            "snapshot-filter"
        ]
    );
}
