//! Reading the text of a config file: what it says, and what it says when it
//! is wrong (technical design §6.1).

mod support;

use app::ports::{ConfigError, PasswordProblem, Position, Problem};
use config_adapter::{MigrationError, Schemas};
use domain::planning::{
    AgentSettings, Concurrency, CronExpression, GroupBy, HookEntry, Hooks, Hostname, KeepCount,
    KeepWithin, Locale, Maintenance, Nice, OnFailure, Plan, RetentionPolicy, RetryPolicy, Schedule,
    Source, SourceFilter, ValueKind, Violation,
};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef, SecretValue};
use domain::{IdKind, PlanId, RepositoryAlias, SignedDuration};
use support::*;

fn plan_of(text: &str) -> Plan {
    let parsed = parsed(text).expect("the file is valid");
    parsed.config.plans()[0].clone()
}

fn alias(text: &str) -> RepositoryAlias {
    RepositoryAlias::parse(text).expect("an alias")
}

#[test]
fn a_file_that_only_says_its_schema_is_the_empty_configuration() {
    let parsed = parsed("schema = 1\n").expect("a valid file");
    assert_eq!(parsed.config, domain::planning::AgentConfig::default());
    assert!(!parsed.migrated, "a current file is not migrated");
}

#[test]
fn a_file_with_a_repository_and_a_plan_gets_the_defaults_for_the_rest() {
    let parsed = parsed(BASE).expect("a valid file");
    assert_eq!(parsed.config.agent(), &AgentSettings::default());
    assert_eq!(parsed.config.hooks(), &Hooks::default());
    let nas = parsed
        .config
        .repository(&alias("nas"))
        .expect("the repository");
    assert_eq!(nas.settings.repository, "/srv/demo-repo");
    assert_eq!(nas.settings.repo_hot, None);
    assert_eq!(
        nas.maintenance,
        Maintenance::None,
        "nobody is asked to prune"
    );
    assert_eq!(nas.group_by, GroupBy::default());
    assert_eq!(nas.hooks, Hooks::default());
    assert_eq!(
        parsed.config.plans(),
        [Plan::new(
            PlanId::parse("home").expect("an ID"),
            alias("nas"),
            vec![Source::parse("/home/demo").expect("a source")],
        )]
    );
}

#[test]
fn the_agent_table_is_read() {
    let text = "schema = 1\n\n[agent]\nhostname = \"demo-host\"\nconcurrency = 4\nnice = 10\n\
                allow-hub-restore = true\nlocale = \"en-US\"\n";
    assert_eq!(
        parsed(text).expect("valid").config.agent(),
        &AgentSettings {
            hostname: Some(Hostname::parse("demo-host").expect("a hostname")),
            concurrency: Concurrency::new(4).expect("a concurrency"),
            nice: Some(Nice::new(10).expect("a nice")),
            allow_hub_restore: true,
            locale: Some(Locale::parse("en-US").expect("a locale")),
        }
    );
}

#[test]
fn a_schedule_that_says_only_when_keeps_the_other_defaults() {
    let plan = plan_of(&base_with("schedule = { cron = \"0 * * * *\" }\n"));
    let schedule = plan.schedule.expect("a schedule");
    assert_eq!(schedule.cron().as_str(), "0 * * * *");
    assert_eq!(schedule.jitter(), None);
    assert!(schedule.catch_up(), "a missed run is made up for");
    assert!(!schedule.on_ac_power(), "any power source will do");
    assert_eq!(plan.retry, None);
}

#[test]
fn a_schedule_and_a_retry_policy_are_read_in_full() {
    let plan = plan_of(&base_with(
        "schedule = { cron = \"0 3 * * 1-5\", jitter = \"90s\", catch-up = false, \
         on-ac-power = true }\nretry = { attempts = 3, backoff = \"10m\" }\n",
    ));
    assert_eq!(
        plan.schedule,
        Some(
            Schedule::new(CronExpression::parse("0 3 * * 1-5").expect("a cron"))
                .with_jitter(SignedDuration::from_secs(90))
                .expect("a jitter")
                .with_catch_up(false)
                .with_on_ac_power(true)
        )
    );
    assert_eq!(
        plan.retry,
        Some(RetryPolicy::new(3, SignedDuration::from_secs(600)).expect("a retry"))
    );
}

#[test]
fn a_plan_says_who_it_is_and_what_it_backs_up() {
    let plan = plan_of(&base_with(
        "host = \"demo-laptop\"\nlabel = \"Demo \\\"home\\\" \u{e9}\"\n\
         tags = [\"demo\", \"Home Dir\"]\n",
    ));
    assert_eq!(
        plan.host,
        Some(Hostname::parse("demo-laptop").expect("a host"))
    );
    assert_eq!(plan.label.as_deref(), Some("Demo \"home\" \u{e9}"));
    let tags: Vec<_> = plan.tags.iter().map(|tag| format!("{tag:?}")).collect();
    assert_eq!(tags, ["UserTag(\"demo\")", "UserTag(\"Home Dir\")"]);
}

#[test]
fn a_plans_filter_keys_are_read() {
    let plan = plan_of(&base_with(
        "globs = [\"*.tmp\", \"!keep.tmp\"]\niglobs = [\"*.LOG\"]\nglob-files = [\"/home/demo/.g\"]\n\
         iglob-files = [\"/home/demo/.ig\"]\ngit-ignore = true\nno-require-git = true\n\
         custom-ignorefiles = [\".backupignore\"]\nexclude-if-present = [\".nobackup\"]\n\
         exclude-if-xattr = [\"com.demo.skip\"]\none-file-system = true\n",
    ));
    let strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        plan.filter,
        SourceFilter {
            globs: strings(&["*.tmp", "!keep.tmp"]),
            iglobs: strings(&["*.LOG"]),
            glob_files: vec![domain::planning::GlobFile::parse("/home/demo/.g").unwrap()],
            iglob_files: vec![domain::planning::GlobFile::parse("/home/demo/.ig").unwrap()],
            git_ignore: true,
            no_require_git: true,
            custom_ignorefiles: strings(&[".backupignore"]),
            exclude_if_present: strings(&[".nobackup"]),
            exclude_if_xattr: strings(&["com.demo.skip"]),
            one_file_system: true,
        }
    );
}

#[test]
fn a_plans_retention_table_is_read() {
    let plan = plan_of(&base_with(
        "\n[plans.retention]\nkeep-last = 3\nkeep-yearly = -1\nkeep-daily = 0\n\
         keep-within = \"36 hours\"\nkeep-within-quarter-yearly = \"2 years\"\n",
    ));
    let count = |n| Some(KeepCount::new(n).expect("a count"));
    let within = |text: &str| Some(KeepWithin::parse(text).expect("a duration"));
    assert_eq!(
        plan.retention,
        RetentionPolicy {
            keep_last: count(3),
            keep_yearly: count(-1),
            keep_daily: count(0),
            keep_within: within("36 hours"),
            keep_within_quarter_yearly: within("2 years"),
            ..RetentionPolicy::default()
        }
    );
}

#[test]
fn hooks_are_a_string_or_a_table_at_every_level() {
    let text = "schema = 1\n\n[hooks]\nrun-before = [\"echo 'agent starting'\"]\n\n\
         [repositories.nas]\nrepository = \"/srv/demo-repo\"\npassword-env = \"DEMO_PASSWORD\"\n\
         hooks = { run-finally = [\"echo done\"] }\n\n[[plans]]\nid = \"home\"\n\
         repository = \"nas\"\nsources = [\"/home/demo\"]\n\
         hooks = { run-before = [\"echo 'plan starting' --now\", \
         { command = \"/opt/demo tools/notify\", args = [\"done\", \"two words\"], \
         on-failure = \"warn\" }], run-failed = [{ command = \"true\", on-failure = \"ignore\" }] }\n";
    let parsed = parsed(text).expect("valid");
    let entry = |text: &str| HookEntry::parse(text).expect("a hook");
    assert_eq!(
        parsed.config.hooks().run_before,
        [entry("echo 'agent starting'")]
    );
    assert_eq!(
        parsed
            .config
            .repository(&alias("nas"))
            .expect("nas")
            .hooks
            .run_finally,
        [entry("echo done")]
    );
    let plan = &parsed.config.plans()[0];
    assert_eq!(
        plan.hooks.run_before,
        [
            entry("echo 'plan starting' --now"),
            HookEntry::new(
                "/opt/demo tools/notify",
                ["done", "two words"],
                OnFailure::Warn
            )
            .expect("a hook"),
        ],
        "a table's command is the program as it is, never split into words"
    );
    assert_eq!(
        plan.hooks.run_failed,
        [HookEntry::new("true", Vec::<String>::new(), OnFailure::Ignore).expect("a hook")]
    );
    assert_eq!(plan.hooks.run_after, []);
}

#[test]
fn a_repository_says_who_maintains_it_and_how_snapshots_are_grouped() {
    let text =
        repository_with("password-env = \"X\"\nmaintenance = \"hub\"\ngroup-by = \"tags,host\"\n");
    let nas = parsed(&text).expect("valid").config.repositories()[&alias("nas")].clone();
    assert_eq!(nas.maintenance, Maintenance::Hub);
    assert_eq!(
        nas.group_by,
        GroupBy {
            host: true,
            label: false,
            paths: false,
            tags: true
        }
    );
}

#[test]
fn each_of_the_five_password_sources_is_read() {
    let source = |lines: &str| {
        let parsed = parsed(&repository_with(lines)).expect("valid");
        parsed.config.repositories()[&alias("nas")]
            .settings
            .password
            .clone()
    };
    assert_eq!(
        source("password = \"demo-password\"\n"),
        SecretRef::Inline(SecretValue::new("demo-password"))
    );
    assert_eq!(
        source("password-command = \"security find-generic-password -s demo -w\"\n"),
        SecretRef::Command(
            CommandLine::parse("security find-generic-password -s demo -w").expect("a command")
        )
    );
    assert_eq!(
        source("password-keyring = \"ladderback/demo nas\"\n"),
        SecretRef::Keyring(KeyringEntry::parse("ladderback/demo nas").expect("an entry"))
    );
    assert_eq!(
        source("password-file = \"/home/demo/.pw\"\n"),
        SecretRef::File {
            path: "/home/demo/.pw".to_owned(),
            read: FileRead::FirstLine
        },
        "a password file is read as rustic reads it: its first line"
    );
    assert_eq!(
        source("password-env = \"DEMO_PASSWORD\"\n"),
        SecretRef::Env("DEMO_PASSWORD".to_owned())
    );
}

#[test]
fn references_in_a_repository_are_kept_as_written() {
    let text = repository_with(
        "password-env = \"X\"\nrepo-hot = \"s3:${DEMO_HOST}/hot\"\n\
         options = { endpoint = \"https://${DEMO_HOST}\", key = \"${file:/home/demo/.key}\" }\n\
         options-hot = { region = \"${DEMO_REGION}\" }\noptions-cold = { class = \"GLACIER\" }\n",
    )
    .replace("/srv/demo-repo", "s3:${DEMO_HOST}/cold");
    let parsed = parsed(&text).expect("valid");
    let settings = &parsed.config.repositories()[&alias("nas")].settings;
    assert_eq!(settings.repository, "s3:${DEMO_HOST}/cold");
    assert_eq!(settings.repo_hot.as_deref(), Some("s3:${DEMO_HOST}/hot"));
    let by_name = |options: &[(&str, &str)]| {
        options
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(
        settings.options,
        by_name(&[
            ("endpoint", "https://${DEMO_HOST}"),
            ("key", "${file:/home/demo/.key}")
        ])
    );
    assert_eq!(
        settings.options_hot,
        by_name(&[("region", "${DEMO_REGION}")])
    );
    assert_eq!(settings.options_cold, by_name(&[("class", "GLACIER")]));
}

#[test]
fn a_repository_needs_exactly_one_password_source() {
    let none = invalid(parsed(&repository_with("")));
    assert_eq!(none.problem, Problem::Password(PasswordProblem::Missing));
    assert_eq!(none.key.as_deref(), Some("repositories.nas"));

    let two = invalid(parsed(&repository_with(
        "password = \"demo\"\npassword-env = \"DEMO_PASSWORD\"\n",
    )));
    assert_eq!(two.problem, Problem::Password(PasswordProblem::Several));
    assert_eq!(two.key.as_deref(), Some("repositories.nas"));
}

#[test]
fn a_password_source_that_is_not_one_is_refused_where_it_is_written() {
    let text = repository_with("password-file = \"relative/pw\"\n");
    let relative = invalid(parsed(&text));
    assert_eq!(
        relative.problem,
        Problem::Password(PasswordProblem::RelativeFile)
    );
    assert_eq!(
        relative.key.as_deref(),
        Some("repositories.nas.password-file")
    );
    assert_eq!(relative.at, Some(position_of(&text, "\"relative/pw\"")));

    let text = repository_with("password-keyring = \"no-slash\"\n");
    let keyring = invalid(parsed(&text));
    assert_eq!(
        keyring.problem,
        Problem::Password(PasswordProblem::NotAKeyringEntry)
    );
    assert_eq!(
        keyring.key.as_deref(),
        Some("repositories.nas.password-keyring")
    );
}

#[test]
fn a_password_command_that_cannot_be_split_is_refused_without_repeating_it() {
    let text = repository_with("password-command = \"get-secret --token hunter2 'oops\"\n");
    let result = parsed(&text);
    let shown = format!("{result:?}");
    let error = invalid(result);
    assert_eq!(
        error.problem,
        Problem::Password(PasswordProblem::BadCommand)
    );
    assert_eq!(
        error.key.as_deref(),
        Some("repositories.nas.password-command")
    );
    assert!(!shown.contains("hunter2"), "{shown}");
}

#[test]
fn a_plan_id_and_an_alias_must_be_what_they_are_for() {
    let bad_id = invalid(parsed(
        &BASE.replace("id = \"home\"", "id = \"Not A Slug\""),
    ));
    assert_eq!(bad_id.problem, Problem::Id(IdKind::Plan));
    assert_eq!(bad_id.key.as_deref(), Some("plans[0].id"));

    let bad_alias = invalid(parsed(
        &BASE.replace("[repositories.nas]", "[repositories.Not-Slug]"),
    ));
    assert_eq!(bad_alias.problem, Problem::Id(IdKind::RepositoryAlias));
    assert_eq!(bad_alias.key.as_deref(), Some("repositories.Not-Slug"));

    let bad_reference = invalid(parsed(
        &BASE.replace("repository = \"nas\"", "repository = \"Nas Repo\""),
    ));
    assert_eq!(bad_reference.problem, Problem::Id(IdKind::RepositoryAlias));
    assert_eq!(bad_reference.key.as_deref(), Some("plans[0].repository"));
}

#[test]
fn a_tag_that_looks_like_one_of_ladderbacks_is_refused() {
    let text = base_with("tags = [\"demo\", \"ladderback-plan:other\"]\n");
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::ReservedTag);
    assert_eq!(error.key.as_deref(), Some("plans[0].tags[1]"));
    assert_eq!(
        error.at,
        Some(position_of(&text, "\"ladderback-plan:other\""))
    );
}

#[test]
fn plans_that_disagree_with_the_file_are_refused_where_they_are_written() {
    let two_plans = |second: &str| format!("{BASE}\n[[plans]]\n{second}");

    let text =
        two_plans("id = \"work\"\nrepository = \"cloud\"\nsources = [\"/home/demo/work\"]\n");
    let unknown = invalid(parsed(&text));
    assert_eq!(
        unknown.problem,
        Problem::Violation(Violation::UnknownRepository {
            index: 1,
            plan: PlanId::parse("work").expect("an ID"),
            repository: alias("cloud"),
        })
    );
    assert_eq!(unknown.key.as_deref(), Some("plans[1].repository"));
    assert_eq!(unknown.at, Some(position_of(&text, "\"cloud\"")));

    let text = two_plans("id = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo/other\"]\n");
    let repeated = invalid(parsed(&text));
    assert_eq!(
        repeated.problem,
        Problem::Violation(Violation::DuplicatePlan {
            index: 1,
            plan: PlanId::parse("home").expect("an ID"),
        })
    );
    assert_eq!(repeated.key.as_deref(), Some("plans[1].id"));

    let text = BASE.replace("sources = [\"/home/demo\"]", "sources = []");
    let empty = invalid(parsed(&text));
    assert_eq!(
        empty.problem,
        Problem::Violation(Violation::NoSources {
            index: 0,
            plan: PlanId::parse("home").expect("an ID"),
        })
    );
    assert_eq!(empty.key.as_deref(), Some("plans[0].sources"));
    assert_eq!(empty.at, Some(position_of(&text, "[]")));
}

/// A value the file holds that the model refuses: where it sits, what it was
/// meant to be, and the text to find its place by.
struct Refused {
    file: String,
    key: &'static str,
    kind: ValueKind,
    value: &'static str,
}

fn refused(file: String, key: &'static str, kind: ValueKind, value: &'static str) -> Refused {
    Refused {
        file,
        key,
        kind,
        value,
    }
}

#[test]
fn a_value_the_model_refuses_is_reported_with_its_key_and_its_place() {
    let agent = |lines: &str| format!("schema = 1\n\n[agent]\n{lines}");
    let rows = [
        refused(
            agent("concurrency = 0\n"),
            "agent.concurrency",
            ValueKind::Concurrency,
            "0",
        ),
        refused(agent("nice = 20\n"), "agent.nice", ValueKind::Nice, "20"),
        refused(
            agent("hostname = \"\"\n"),
            "agent.hostname",
            ValueKind::Hostname,
            "\"\"",
        ),
        refused(
            agent("locale = \"en_US\"\n"),
            "agent.locale",
            ValueKind::Locale,
            "\"en_US\"",
        ),
        refused(
            base_with("schedule = { cron = \"every day\" }\n"),
            "plans[0].schedule.cron",
            ValueKind::Cron,
            "\"every day\"",
        ),
        refused(
            base_with("schedule = { cron = \"0 0 31 2 *\" }\n"),
            "plans[0].schedule.cron",
            ValueKind::Cron,
            "\"0 0 31 2 *\"",
        ),
        refused(
            base_with("schedule = { cron = \"@daily\", jitter = \"5 parsecs\" }\n"),
            "plans[0].schedule.jitter",
            ValueKind::Duration,
            "\"5 parsecs\"",
        ),
        refused(
            base_with("retry = { attempts = 0, backoff = \"10m\" }\n"),
            "plans[0].retry.attempts",
            ValueKind::Attempts,
            "0",
        ),
        refused(
            base_with("\n[plans.retention]\nkeep-last = -2\n"),
            "plans[0].retention.keep-last",
            ValueKind::KeepCount,
            "-2",
        ),
        refused(
            base_with("\n[plans.retention]\nkeep-within = \"-1d\"\n"),
            "plans[0].retention.keep-within",
            ValueKind::KeepWithin,
            "\"-1d\"",
        ),
        refused(
            base_with("host = \"\"\n"),
            "plans[0].host",
            ValueKind::Hostname,
            "\"\"",
        ),
        refused(
            BASE.replace("[\"/home/demo\"]", "[\"/home/demo\", \"home/demo\"]"),
            "plans[0].sources[1]",
            ValueKind::Source,
            "\"home/demo\"",
        ),
        refused(
            repository_with("password-env = \"X\"\nmaintenance = \"sometimes\"\n"),
            "repositories.nas.maintenance",
            ValueKind::Maintenance,
            "\"sometimes\"",
        ),
        refused(
            repository_with("password-env = \"X\"\ngroup-by = \"colour\"\n"),
            "repositories.nas.group-by",
            ValueKind::GroupBy,
            "\"colour\"",
        ),
    ];
    assert_refused_where_written(rows);
}

/// Each row is refused as its kind, at its key and its value's place.
fn assert_refused_where_written(rows: impl IntoIterator<Item = Refused>) {
    for row in rows {
        let error = invalid(parsed(&row.file));
        assert_eq!(error.problem, Problem::Value(row.kind), "{}", row.key);
        assert_eq!(error.key.as_deref(), Some(row.key));
        assert_eq!(
            error.at,
            Some(position_of(&row.file, row.value)),
            "{}",
            row.key
        );
        assert_eq!(error.detail, "", "{}: the problem says it all", row.key);
        assert_eq!(error.file, FILE);
    }
}

// rustic reads a glob file from the worker's working directory, the daemon's,
// so a relative one would mean whatever folder the daemon was started in.
#[test]
fn a_glob_file_that_is_not_an_absolute_path_is_refused_at_its_plan_and_key() {
    assert_refused_where_written([
        refused(
            base_with("glob-files = [\"/home/demo/.g\", \".excludes\"]\n"),
            "plans[0].glob-files[1]",
            ValueKind::GlobFile,
            "\".excludes\"",
        ),
        refused(
            base_with("iglob-files = [\"globs/home.txt\"]\n"),
            "plans[0].iglob-files[0]",
            ValueKind::GlobFile,
            "\"globs/home.txt\"",
        ),
    ]);
}

#[test]
fn a_hook_command_is_refused_without_repeating_a_word_of_it() {
    let rows = [
        (
            "hooks = { run-before = [\"login --password hunter2 'never closed\"] }\n",
            "plans[0].hooks.run-before[0]",
            ValueKind::HookCommand,
        ),
        (
            "hooks = { run-failed = [{ command = \"\", args = [\"--password\", \"hunter2\"] }] }\n",
            "plans[0].hooks.run-failed[0].command",
            ValueKind::HookCommand,
        ),
        (
            "hooks = { run-after = [{ command = \"login\", args = [\"hunter2\"], \
             on-failure = \"explode\" }] }\n",
            "plans[0].hooks.run-after[0].on-failure",
            ValueKind::OnFailure,
        ),
    ];
    for (hooks, key, kind) in rows {
        let result = parsed(&base_with(hooks));
        let shown = format!("{result:?}");
        let error = invalid(result);
        assert_eq!(error.problem, Problem::Value(kind), "{key}");
        assert_eq!(error.key.as_deref(), Some(key));
        assert!(!shown.contains("hunter2"), "{key}: {shown}");
        assert!(!shown.contains("login"), "{key}: {shown}");
    }
}

#[test]
fn a_value_of_the_wrong_type_is_reported_without_the_value() {
    let text = repository_with("password = 123456789\n");
    let result = parsed(&text);
    let shown = format!("{result:?}");
    let error = invalid(result);
    assert_eq!(error.problem, Problem::WrongType);
    assert_eq!(error.key.as_deref(), Some("repositories.nas.password"));
    assert_eq!(error.at, Some(position_of(&text, "123456789")));
    assert!(!shown.contains("123456789"), "{shown}");
    assert!(
        error.detail.contains("integer") && error.detail.contains("string"),
        "the parser's words stay, with the value left out: {:?}",
        error.detail
    );

    let text = base_with("schedule = { cron = 5 }\n");
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::WrongType);
    assert_eq!(error.key.as_deref(), Some("plans[0].schedule.cron"));
}

#[test]
fn a_wrong_type_says_what_the_setting_could_have_been() {
    let error = invalid(parsed(&base_with("hooks = { run-before = [5] }\n")));
    assert_eq!(error.problem, Problem::WrongType);
    assert_eq!(error.key.as_deref(), Some("plans[0].hooks.run-before[0]"));
    assert!(
        error.detail.ends_with("expected a command line or a table"),
        "{:?}",
        error.detail
    );

    let error = invalid(parsed("schema = 1\nrepositories = 5\n"));
    assert_eq!(error.problem, Problem::WrongType);
    assert_eq!(error.key.as_deref(), Some("repositories"));
    assert!(
        error.detail.ends_with("expected a table of repositories"),
        "{:?}",
        error.detail
    );
}

#[test]
fn what_a_wrong_type_says_after_a_quoted_value_is_kept_whatever_the_values_length() {
    for value in ["a", "ab", "abc", "abcd", "a\\\\", "a\\\\b"] {
        let result = parsed(&base_with(&format!("schedule = '{value}'\n")));
        let error = invalid(result);
        assert_eq!(error.problem, Problem::WrongType, "{value}");
        assert!(
            error.detail.starts_with("invalid type: string,") && error.detail.contains("expected"),
            "{value}: {:?}",
            error.detail
        );
    }
}

#[test]
fn a_wrong_type_value_holding_quotes_and_backslashes_is_reported_without_a_word_of_it() {
    // serde quotes a string it can't use with `{:?}`, which writes an inner
    // quote as `\"` and a backslash as `\\`: neither ends the quoted text.
    let rows = [
        "schedule = 'curl -H \"Authorization: CANARY-bearer\" https://nas.demo'\n",
        "schedule = 'a \\\\\"CANARY-after-a-backslash\" b'\n",
        "schedule = 'a \\\\ \"CANARY-after-two\" b'\n",
    ];
    for row in rows {
        let result = parsed(&base_with(row));
        let shown = format!("{result:?}");
        let error = invalid(result);
        assert_eq!(error.problem, Problem::WrongType, "{row}");
        assert_eq!(error.key.as_deref(), Some("plans[0].schedule"), "{row}");
        assert!(!shown.contains("CANARY"), "{row}: {shown}");
        assert!(error.detail.contains("string"), "{:?}", error.detail);
    }
}

#[test]
fn text_that_is_not_toml_is_reported_where_it_stops_without_quoting_it() {
    let text = "schema = 1\n\n[repositories.nas]\npassword = \"hunter2\" oops\n";
    let result = parsed(text);
    let shown = format!("{result:?}");
    let error = invalid(result);
    assert_eq!(error.problem, Problem::Syntax);
    assert_eq!(error.key, None);
    assert_eq!(error.at.map(|at| at.line), Some(4));
    assert!(!error.detail.is_empty(), "the parser says what it expected");
    assert!(!shown.contains("hunter2"), "{shown}");

    let error = invalid(parsed("schema = 1\n[agent\n"));
    assert_eq!(error.problem, Problem::Syntax);
    assert_eq!(error.at.map(|at| at.line), Some(2));
}

/// A key the format doesn't have: where it is, and the text to find its place by.
struct Unknown {
    file: String,
    key: &'static str,
    at: Option<&'static str>,
}

fn unknown(file: String, key: &'static str, at: Option<&'static str>) -> Unknown {
    Unknown { file, key, at }
}

#[test]
fn an_unknown_key_is_reported_with_its_file_and_line() {
    let rows = [
        unknown(
            "schema = 1\ncolour = \"red\"\n".to_owned(),
            "colour",
            Some("colour"),
        ),
        unknown(
            "schema = 1\n\n[agent]\nhostnam = \"demo\"\n".to_owned(),
            "agent.hostnam",
            Some("hostnam"),
        ),
        unknown(
            "schema = 1\n\n[hooks]\nrun-sometimes = []\n".to_owned(),
            "hooks.run-sometimes",
            Some("run-sometimes"),
        ),
        unknown(
            base_with("\n[plans.retention]\nkeep-tags = [\"demo\"]\n"),
            "plans[0].retention.keep-tags",
            Some("keep-tags"),
        ),
        unknown(
            base_with("exclude-larger-than = \"1GiB\"\n"),
            "plans[0].exclude-larger-than",
            Some("exclude-larger-than"),
        ),
        unknown(
            base_with("schedule = { cron = \"@daily\", timezone = \"UTC\" }\n"),
            "plans[0].schedule.timezone",
            Some("timezone"),
        ),
        unknown(
            repository_with("password-env = \"X\"\nprune-everything = true\n"),
            "repositories.nas.prune-everything",
            Some("prune-everything"),
        ),
        // Not modelled yet: refused rather than silently ignored.
        unknown(
            "schema = 1\n\n[hub]\nurl = \"https://hub.example.lan:9899\"\n".to_owned(),
            "hub",
            None,
        ),
        unknown(
            format!(
                "{}\n[repositories.nas.prune]\nkeep-delete = \"6d\"\n",
                repository_with("password-env = \"X\"\n")
            ),
            "repositories.nas.prune",
            None,
        ),
    ];
    for row in rows {
        let error = invalid(parsed(&row.file));
        assert_eq!(error.problem, Problem::UnknownKey, "{}", row.key);
        assert_eq!(error.key.as_deref(), Some(row.key));
        assert_eq!(error.file, FILE);
        if let Some(needle) = row.at {
            assert_eq!(
                error.at,
                Some(position_of(&row.file, needle)),
                "{}",
                row.key
            );
        }
    }
}

#[test]
fn the_first_unknown_key_is_the_first_in_the_file_not_the_first_by_name() {
    let text = "schema = 1\nzebra = 1\napple = 2\n";
    let error = invalid(parsed(text));
    assert_eq!(error.key.as_deref(), Some("zebra"));
    assert_eq!(error.at, Some(Position { line: 2, column: 1 }));
}

#[test]
fn a_file_with_no_schema_is_refused_as_not_a_config() {
    let error = parsed("[agent]\nconcurrency = 2\n").expect_err("no schema");
    assert_eq!(
        error,
        ConfigError::SchemaMissing {
            file: FILE.to_owned()
        }
    );
    // Even when what it holds is no use either: the missing schema is the answer.
    let error = parsed("[agent]\nbogus = 2\n").expect_err("no schema");
    assert_eq!(
        error,
        ConfigError::SchemaMissing {
            file: FILE.to_owned()
        }
    );
}

#[test]
fn a_schema_that_is_not_a_version_is_refused_at_its_line() {
    for value in ["\"1\"", "-1", "1.5", "true", "[1]"] {
        let text = format!("# demo\nschema = {value}\n");
        let error = invalid(parsed(&text));
        assert_eq!(error.problem, Problem::SchemaNotAVersion, "{value}");
        assert_eq!(error.key.as_deref(), Some("schema"));
        assert_eq!(error.at.map(|at| at.line), Some(2), "{value}");
    }
}

#[test]
fn a_file_from_a_newer_version_is_refused_before_anything_else_in_it_is() {
    // What a newer build wrote may look wrong to this one: it isn't read at all.
    let text = "schema = 2\n[agent]\nbogus = 1\n[[plans]]\nid = \"Not A Slug\"\n";
    assert_eq!(
        parsed(text).expect_err("newer"),
        ConfigError::SchemaNewer {
            file: FILE.to_owned(),
            found: 2,
            supported: 1
        }
    );
    assert_eq!(
        parsed("schema = 9223372036854775807\n").expect_err("newer"),
        ConfigError::SchemaNewer {
            file: FILE.to_owned(),
            found: i64::MAX.unsigned_abs(),
            supported: 1,
        }
    );
}

#[test]
fn a_file_older_than_the_oldest_version_read_is_refused() {
    let error = invalid(parsed("schema = 0\n"));
    assert_eq!(
        error.problem,
        Problem::SchemaTooOld {
            found: 0,
            oldest: 1
        }
    );
    assert_eq!(error.key.as_deref(), Some("schema"));
    assert_eq!(
        error.at,
        Some(Position {
            line: 1,
            column: 10
        })
    );

    let schemas = Schemas {
        current: 3,
        steps: vec![rename_name, rename_priority],
    };
    assert_eq!(schemas.oldest(), 1);
    let schemas = Schemas {
        current: 3,
        steps: vec![rename_priority],
    };
    let error = invalid(parsed_with("schema = 1\n", &schemas));
    assert_eq!(
        error.problem,
        Problem::SchemaTooOld {
            found: 1,
            oldest: 2
        }
    );
}

/// Version 1 to 2: `[agent] name` became `hostname`.
fn rename_name(table: &mut toml::Table) -> Result<(), MigrationError> {
    if let Some(agent) = table.get_mut("agent").and_then(toml::Value::as_table_mut)
        && let Some(name) = agent.remove("name")
    {
        agent.insert("hostname".to_owned(), name);
    }
    Ok(())
}

/// Version 2 to 3: `[agent] priority` became `nice`.
fn rename_priority(table: &mut toml::Table) -> Result<(), MigrationError> {
    if let Some(agent) = table.get_mut("agent").and_then(toml::Value::as_table_mut)
        && let Some(priority) = agent.remove("priority")
    {
        agent.insert("nice".to_owned(), priority);
    }
    Ok(())
}

fn chain() -> Schemas {
    Schemas {
        current: 3,
        steps: vec![rename_name, rename_priority],
    }
}

#[test]
fn an_older_file_goes_through_every_migration_after_its_version() {
    let parsed = parsed_with(
        "schema = 1\n\n[agent]\nname = \"demo-host\"\npriority = 5\n",
        &chain(),
    )
    .expect("valid after migration");
    assert!(parsed.migrated);
    assert_eq!(
        parsed.config.agent().hostname,
        Some(Hostname::parse("demo-host").expect("host"))
    );
    assert_eq!(
        parsed.config.agent().nice,
        Some(Nice::new(5).expect("nice"))
    );
}

#[test]
fn a_file_goes_through_only_the_migrations_it_has_not_had() {
    let parsed = parsed_with("schema = 2\n\n[agent]\npriority = 5\n", &chain()).expect("valid");
    assert!(parsed.migrated);
    assert_eq!(
        parsed.config.agent().nice,
        Some(Nice::new(5).expect("nice"))
    );

    // `name` was renamed by the step from version 1: a version 2 file that
    // still has it is wrong, so the step must not have run on it.
    let error = invalid(parsed_with(
        "schema = 2\n\n[agent]\nname = \"demo-host\"\n",
        &chain(),
    ));
    assert_eq!(error.problem, Problem::UnknownKey);
    assert_eq!(error.key.as_deref(), Some("agent.name"));

    let parsed = parsed_with("schema = 3\n\n[agent]\nnice = 5\n", &chain()).expect("valid");
    assert!(!parsed.migrated, "a current file is read as it is");
    let error = invalid(parsed_with(
        "schema = 3\n\n[agent]\npriority = 5\n",
        &chain(),
    ));
    assert_eq!(
        error.key.as_deref(),
        Some("agent.priority"),
        "no step runs on a current file"
    );
}

fn failing(_table: &mut toml::Table) -> Result<(), MigrationError> {
    Err(MigrationError {
        detail: "the demo setting can't be moved".to_owned(),
    })
}

fn sets_concurrency_to_zero(table: &mut toml::Table) -> Result<(), MigrationError> {
    let mut agent = toml::Table::new();
    agent.insert("concurrency".to_owned(), toml::Value::Integer(0));
    table.insert("agent".to_owned(), toml::Value::Table(agent));
    Ok(())
}

#[test]
fn a_migration_that_fails_is_reported_with_its_own_words_and_no_place() {
    let schemas = Schemas {
        current: 2,
        steps: vec![failing],
    };
    let error = invalid(parsed_with("schema = 1\n", &schemas));
    assert_eq!(error.problem, Problem::Migration);
    assert_eq!(error.detail, "the demo setting can't be moved");
    assert_eq!(error.at, None);
}

#[test]
fn what_is_wrong_in_a_migrated_file_has_a_key_and_no_line() {
    // The text the migration made was never in the file, so a line would point
    // at something else.
    let schemas = Schemas {
        current: 2,
        steps: vec![sets_concurrency_to_zero],
    };
    let error = invalid(parsed_with("schema = 1\n", &schemas));
    assert_eq!(error.problem, Problem::Value(ValueKind::Concurrency));
    assert_eq!(error.key.as_deref(), Some("agent.concurrency"));
    assert_eq!(error.at, None);

    let schemas = Schemas {
        current: 2,
        steps: vec![rename_name],
    };
    let error = invalid(parsed_with("schema = 1\n\n[agent]\nbogus = 1\n", &schemas));
    assert_eq!(error.problem, Problem::UnknownKey);
    assert_eq!(error.key.as_deref(), Some("agent.bogus"));
    assert_eq!(error.at, None);
}

#[test]
fn an_error_names_the_file_as_it_was_given() {
    let file = std::path::Path::new("demo/config.toml");
    let error = config_adapter::parse(file, "schema = 1\ncolour = 1\n", &Schemas::production())
        .expect_err("unknown key");
    assert!(
        matches!(&error, ConfigError::Invalid { file, .. } if file == "demo/config.toml"),
        "{error:?}"
    );
}

#[test]
fn a_secret_written_in_a_file_that_is_refused_is_not_repeated() {
    // Inline passwords and a secret in a command line, beside the mistake.
    let text = "schema = 1\ncolour = 1\n\n[repositories.nas]\nrepository = \"rest:http://demo:CANARY-url@host/r\"\n\
                password = \"CANARY-password\"\noptions = { sas_token = \"CANARY-option\" }\n";
    let result = parsed(text);
    let shown = format!("{result:?}");
    assert!(shown.contains("colour"), "{shown}");
    assert!(!shown.contains("CANARY"), "{shown}");
}
