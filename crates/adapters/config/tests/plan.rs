//! One plan on its own, in the file's keys, as the Plans page sends it and
//! shows it (decision 96): read by the loader's code and written as the file
//! writes it.

mod support;

use app::ports::{ConfigError, Problem};
use config_adapter::{plan_as, plan_table, read_plan};
use domain::IdKind;
use domain::planning::{Plan, ValueKind};
use support::*;

/// The table `text` writes, as a person writes a plan's keys.
fn table(text: &str) -> toml::Table {
    text.parse().expect("the plan's keys are TOML")
}

#[expect(
    clippy::result_large_err,
    reason = "`ConfigError` is the port's, its shape fixed there, as the crate's own expectation says"
)]
fn read(text: &str) -> Result<Plan, ConfigError> {
    read_plan(&table(text))
}

/// The keys of a plan with a bit of every kind of setting.
const FULL: &str = r#"id = "home"
repository = "nas"
label = "Documents"
host = "laptop"
sources = ["/home/demo", "/srv/shared"]
tags = ["documents"]
globs = ["!*.tmp"]
git-ignore = true
one-file-system = true
overlap = "queue"
schedule = { cron = "0 3 * * *", timezone = "Europe/Berlin", jitter = "5m", catch-up = false }
retry = { attempts = 3, backoff = "1m" }
retention = { keep-last = 7, keep-within-daily = "30 days" }
hooks = { run-before = ["/usr/local/bin/mount-nas"] }
"#;

#[test]
fn a_plan_s_keys_read_as_the_loader_reads_the_same_keys_in_the_file() {
    let file = parsed(&format!(
        "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\n\
         password-env = \"DEMO_PASSWORD\"\n\n[[plans]]\n{FULL}"
    ))
    .expect("the file loads");
    assert_eq!(read(FULL).expect("the plan reads"), file.config.plans()[0]);
}

#[test]
fn a_plan_written_and_read_back_is_the_same_plan() {
    let plan = read(FULL).expect("the plan reads");
    let written: toml::Table = plan_as(&plan).expect("the plan writes");
    assert_eq!(read_plan(&written).expect("the written plan reads"), plan);
}

#[test]
fn a_plan_is_written_with_what_was_chosen_and_no_default() {
    let plan = read("id = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]\n")
        .expect("the plan reads");
    let written: toml::Table = plan_as(&plan).expect("the plan writes");
    assert_eq!(
        written,
        table("id = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]\n")
    );
}

#[test]
fn a_value_the_setting_can_t_take_is_refused_at_its_key_from_the_plan_s_own_table() {
    let refused = invalid(read(
        "id = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]\n\
         schedule = { cron = \"61 * * * *\" }\n",
    ));
    assert_eq!(refused.key.as_deref(), Some("schedule.cron"));
    assert_eq!(refused.problem, Problem::Value(ValueKind::Cron));
    assert_eq!(refused.at, None, "the text is not one a person wrote");
}

#[test]
fn a_missing_key_is_refused_by_its_name() {
    let refused = invalid(read("id = \"home\"\nrepository = \"nas\"\n"));
    assert_eq!(refused.key.as_deref(), Some("sources"));
    assert_eq!(refused.problem, Problem::MissingKey);
}

#[test]
fn a_key_a_plan_doesn_t_have_is_refused_by_its_name_at_any_depth() {
    let refused = invalid(read(
        "id = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]\ncolour = \"red\"\n",
    ));
    assert_eq!(refused.key.as_deref(), Some("colour"));
    assert_eq!(refused.problem, Problem::UnknownKey);
    let refused = invalid(read(
        "id = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]\n\
         schedule = { cron = \"0 3 * * *\", often = true }\n",
    ));
    assert_eq!(refused.key.as_deref(), Some("schedule.often"));
    assert_eq!(refused.problem, Problem::UnknownKey);
}

#[test]
fn a_value_of_another_type_is_refused_at_its_key_without_the_value() {
    let refused = invalid(read(
        "id = \"home\"\nrepository = \"nas\"\nsources = \"hunter2-not-a-list\"\n",
    ));
    assert_eq!(refused.key.as_deref(), Some("sources"));
    assert_eq!(refused.problem, Problem::WrongType);
    assert!(!refused.detail.contains("hunter2"), "{}", refused.detail);
}

#[test]
fn an_id_that_isn_t_one_is_refused_at_the_id() {
    let refused = invalid(read(
        "id = \"Home Folder\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]\n",
    ));
    assert_eq!(refused.key.as_deref(), Some("id"));
    assert!(
        matches!(refused.problem, Problem::Id(IdKind::Plan)),
        "{refused:?}"
    );
}

/// A plan given as TOML text, as an MCP client proposes one (§4.6), reads
/// as the same keys the text writes, and so as the same plan.
#[test]
fn a_plan_given_as_text_is_its_table_in_the_same_keys() {
    let given = plan_table(FULL).expect("the text is a table");
    assert_eq!(
        serde_json::Value::Object(given.clone()),
        serde_json::to_value(table(FULL)).unwrap()
    );
    assert_eq!(
        read_plan(&given).expect("the plan reads"),
        read(FULL).expect("the plan reads")
    );
}

/// Text that isn't a TOML table is refused as a whole, its detail naming
/// none of the text: the plan is checked by the daemon, not here.
#[test]
fn text_that_isn_t_a_table_is_refused_as_a_whole_without_its_text() {
    for text in [
        "id = ",
        "canary",
        "id = \"canary\"\nid = \"canary\"",
        "[canary",
    ] {
        let refused = invalid(plan_table(text));
        assert_eq!(refused.key, None, "{text:?}");
        assert_eq!(refused.problem, Problem::Syntax, "{text:?}");
        assert!(
            !refused.detail.contains("canary"),
            "{text:?}: {}",
            refused.detail
        );
    }
}
