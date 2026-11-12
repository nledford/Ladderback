//! Saving over the file a person wrote (§6.1): the change a save makes is
//! edited into the file as it is, and the rest of it, comments, blank lines
//! and layout, stays as written.

use std::path::Path;

use domain::planning::AgentConfig;

use crate::schema::Schemas;

/// `text`, the file as it is, changed to hold `config`, whose text in the
/// daemon's layout is `rendered`; `None` when it can't be.
pub(crate) fn saved(
    file: &Path,
    text: &str,
    config: &AgentConfig,
    rendered: &str,
    schemas: &Schemas,
) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use domain::planning::{
        AgentConfig, AgentSettings, Concurrency, CronExpression, GroupBy, Hooks, Hostname,
        KeepCount, Maintenance, Plan, RepositoryConfig, RepositorySettings, RetentionPolicy,
        Schedule, Source,
    };
    use domain::secrets::SecretRef;
    use domain::{PlanId, RepositoryAlias};

    use super::saved;
    use crate::schema::{CURRENT_SCHEMA, MAX_CONFIG_BYTES, Schemas, parse, render_as};

    const FILE: &str = "config.toml";

    /// A file as a person writes one: comments, blank lines, their own
    /// order and spacing, defaults said out loud, and values written in
    /// another form than the daemon's (`'…'`, `"5 minutes"`, a list over
    /// several lines).
    const WRITTEN: &str = r#"# The demo laptop's agent, written by hand.
schema = 1

[agent]
concurrency = 1        # how many operations run at once
nice    =   10

# The NAS in the hall cupboard.
[repositories.nas]
  repository = '/srv/demo-repo'
  password-env = "DEMO_PASSWORD"
  maintenance = "none"   # the hub may own it later
  options = { timeout = "60s" }

[[plans]]
id = "home"
repository = "nas"
sources = [
  "/home/demo",        # everything
  "/srv/demo-shared",  # and the shared folder
]
schedule = { cron = "0 * * * *", jitter = "5 minutes" }

# How long the history goes back.
[plans.retention]
keep-daily = 7
"#;

    /// The parts of a configuration, to change one and build it again.
    struct Parts {
        agent: AgentSettings,
        hooks: Hooks,
        repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
        plans: Vec<Plan>,
    }

    fn read(text: &str) -> AgentConfig {
        parse(Path::new(FILE), text, &Schemas::production())
            .expect("the file parses")
            .config
    }

    fn rebuilt(config: &AgentConfig, change: impl FnOnce(&mut Parts)) -> AgentConfig {
        let mut parts = Parts {
            agent: config.agent().clone(),
            hooks: config.hooks().clone(),
            repositories: config.repositories().clone(),
            plans: config.plans().to_vec(),
        };
        change(&mut parts);
        AgentConfig::new(parts.agent, parts.hooks, parts.repositories, parts.plans)
            .expect("a valid configuration")
    }

    /// What a save of `config` writes over `text`, when it edits it.
    fn saved_as(text: &str, config: &AgentConfig) -> Option<String> {
        let rendered = render_as(Path::new(FILE), config, CURRENT_SCHEMA).expect("renders");
        saved(
            Path::new(FILE),
            text,
            config,
            &rendered,
            &Schemas::production(),
        )
    }

    /// What a save of the configuration in `text`, changed by `change`,
    /// writes over `text`.
    fn changed(text: &str, change: impl FnOnce(&mut Parts)) -> Option<String> {
        saved_as(text, &rebuilt(&read(text), change))
    }

    fn alias(name: &str) -> RepositoryAlias {
        RepositoryAlias::parse(name).expect("an alias")
    }

    fn plan_id(name: &str) -> PlanId {
        PlanId::parse(name).expect("a plan ID")
    }

    fn entry_at(location: &str) -> RepositoryConfig {
        RepositoryConfig {
            settings: RepositorySettings {
                repository: location.to_owned(),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            id: None,
            maintenance: Maintenance::default(),
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        }
    }

    fn plan_mut<'p>(parts: &'p mut Parts, id: &str) -> &'p mut Plan {
        let id = plan_id(id);
        parts
            .plans
            .iter_mut()
            .find(|plan| plan.id == id)
            .expect("the plan")
    }

    #[test]
    fn a_save_of_the_configuration_as_it_was_changes_no_byte() {
        assert_eq!(changed(WRITTEN, |_| {}).as_deref(), Some(WRITTEN));
    }

    #[test]
    fn a_changed_setting_is_changed_where_it_is_written_beside_its_comment() {
        let saved = changed(WRITTEN, |parts| {
            parts.agent.concurrency = Concurrency::new(2).expect("a concurrency");
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace("concurrency = 1        #", "concurrency = 2        #"))
        );
    }

    #[test]
    fn a_setting_a_save_adds_follows_the_last_one_of_its_table() {
        let saved = changed(WRITTEN, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "nice    =   10\n",
                "nice    =   10\nhostname = \"demo-host\"\n"
            ))
        );
    }

    #[test]
    fn a_setting_a_save_removes_goes_with_its_line() {
        let saved = changed(WRITTEN, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.settings.options.clear();
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace("  options = { timeout = \"60s\" }\n", ""))
        );
    }

    #[test]
    fn a_value_written_another_way_is_kept_until_it_changes_then_written_the_daemons_way() {
        let saved = changed(WRITTEN, |parts| {
            let home = plan_mut(parts, "home");
            let jitter = home.schedule.as_ref().and_then(Schedule::jitter);
            let schedule = Schedule::new(CronExpression::parse("0 2 * * *").expect("a cron"));
            home.schedule = Some(
                schedule
                    .with_jitter(jitter.expect("a jitter"))
                    .expect("a schedule"),
            );
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "schedule = { cron = \"0 * * * *\", jitter = \"5 minutes\" }",
                "schedule = { cron = \"0 2 * * *\", jitter = \"5m\" }"
            ))
        );
    }

    #[test]
    fn a_repository_a_save_adds_follows_the_last_one() {
        let saved = changed(WRITTEN, |parts| {
            parts
                .repositories
                .insert(alias("cloud"), entry_at("/srv/demo-cloud"));
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "  options = { timeout = \"60s\" }\n",
                "  options = { timeout = \"60s\" }\n\n[repositories.cloud]\nrepository = \"/srv/demo-cloud\"\npassword-env = \"DEMO_PASSWORD\"\n"
            ))
        );
    }

    #[test]
    fn a_repository_a_save_removes_goes_with_the_comment_above_it() {
        let two = format!(
            "{WRITTEN}\n# The old USB drive, retired.\n[repositories.usb]\nrepository = \"/Volumes/demo-usb/repo\"\npassword-env = \"DEMO_PASSWORD\"\n"
        );
        let saved = changed(&two, |parts| {
            parts.repositories.remove(&alias("usb"));
        });
        assert_eq!(saved.as_deref(), Some(WRITTEN));
    }

    #[test]
    fn the_last_repository_and_plan_a_save_removes_leave_the_rest() {
        let saved = changed(WRITTEN, |parts| {
            parts.repositories.clear();
            parts.plans.clear();
        });
        assert_eq!(
            saved.as_deref(),
            Some(
                "# The demo laptop's agent, written by hand.\nschema = 1\n\n[agent]\nconcurrency = 1        # how many operations run at once\nnice    =   10\n"
            )
        );
    }

    #[test]
    fn a_changed_plan_keeps_what_is_written_around_the_change() {
        let saved = changed(WRITTEN, |parts| {
            plan_mut(parts, "home").label = Some("demo-laptop home".to_owned());
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "jitter = \"5 minutes\" }\n",
                "jitter = \"5 minutes\" }\nlabel = \"demo-laptop home\"\n"
            ))
        );
    }

    /// The plan `docs`, keeping its last 3 snapshots, and its text in the
    /// daemon's layout.
    fn docs() -> Plan {
        let mut docs = Plan::new(
            plan_id("docs"),
            alias("nas"),
            vec![Source::parse("/home/demo/docs").expect("a source")],
        );
        docs.retention = RetentionPolicy {
            keep_last: Some(KeepCount::new(3).expect("a count")),
            ..RetentionPolicy::default()
        };
        docs
    }

    const DOCS: &str = "\n[[plans]]\nid = \"docs\"\nrepository = \"nas\"\nsources = [\"/home/demo/docs\"]\n\n[plans.retention]\nkeep-last = 3\n";

    #[test]
    fn a_plan_a_save_adds_follows_the_last_plan_and_its_retention() {
        let saved = changed(WRITTEN, |parts| parts.plans.push(docs()));
        assert_eq!(saved, Some(format!("{WRITTEN}{DOCS}")));
    }

    /// A file that writes options as tables of their own has more tables
    /// before its plans than the daemon's layout: a plan added keeps its
    /// place after them, and its retention stays with it.
    #[test]
    fn a_plan_added_after_options_written_as_tables_keeps_its_retention() {
        let written = WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n",
            "\n[repositories.nas.options]\ntimeout = \"60s\"\n\n[repositories.nas.options-hot]\ntimeout = \"30s\"\n\n[repositories.nas.options-cold]\ntimeout = \"90s\"\n",
        );
        let saved = changed(&written, |parts| parts.plans.push(docs()));
        assert_eq!(saved, Some(format!("{written}{DOCS}")));
    }

    #[test]
    fn an_option_written_in_a_table_of_its_own_is_changed_there() {
        let written = WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n",
            "\n# Options for the NAS.\n[repositories.nas.options]\ntimeout = \"60s\"   # it is slow\nretries = \"3\"\n",
        );
        let saved = changed(&written, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.settings
                .options
                .insert("timeout".to_owned(), "90s".to_owned());
        });
        assert_eq!(
            saved,
            Some(written.replace("timeout = \"60s\"   #", "timeout = \"90s\"   #"))
        );
    }

    #[test]
    fn an_entry_written_inline_is_written_inline_again_when_it_changes() {
        let written = "schema = 1\n\n[repositories]\n# The NAS.\nnas = { repository = \"/srv/demo-repo\", password-env = \"DEMO_PASSWORD\" }  # by hand\n";
        let saved = changed(written, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.maintenance = Maintenance::Local;
        });
        assert_eq!(
            saved.as_deref(),
            Some(
                "schema = 1\n\n[repositories]\n# The NAS.\nnas = { repository = \"/srv/demo-repo\", password-env = \"DEMO_PASSWORD\", maintenance = \"local\" }  # by hand\n"
            )
        );
    }

    #[test]
    fn a_table_a_save_adds_goes_at_the_end_of_the_file() {
        let written = "# By hand.\nschema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\npassword-env = \"DEMO_PASSWORD\"\n";
        let saved = changed(written, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(
            saved,
            Some(format!("{written}\n[agent]\nhostname = \"demo-host\"\n"))
        );
    }

    #[test]
    fn a_file_with_crlf_lines_and_a_byte_order_mark_keeps_both() {
        let written = format!("\u{feff}{}", WRITTEN.replace('\n', "\r\n"));
        let saved = changed(&written, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(
            saved,
            Some(written.replace(
                "nice    =   10\r\n",
                "nice    =   10\r\nhostname = \"demo-host\"\r\n"
            ))
        );
    }

    #[test]
    fn a_file_whose_lines_end_both_ways_is_not_edited() {
        let written = WRITTEN.replacen('\n', "\r\n", 1);
        assert_eq!(changed(&written, |_| {}), None);
    }

    #[test]
    fn a_file_that_is_not_a_configuration_is_not_edited() {
        let config = read(WRITTEN);
        assert_eq!(saved_as("schema = [", &config), None);
        assert_eq!(saved_as("schema = 1\ncolour = \"red\"\n", &config), None);
    }

    /// A file in an older version is the load's to migrate, as a whole.
    #[test]
    fn a_file_in_an_older_version_is_not_edited() {
        let newer = Schemas {
            current: 2,
            steps: vec![|_| Ok(())],
        };
        let config = read(WRITTEN);
        let rendered = render_as(Path::new(FILE), &config, 2).expect("renders");
        assert_eq!(
            saved(Path::new(FILE), WRITTEN, &config, &rendered, &newer),
            None
        );
    }

    /// Plans are in the order of the file, and an edit keeps each where it
    /// is written: plans put in another order can't be edited in.
    #[test]
    fn plans_put_in_another_order_are_not_edited_in() {
        let two = format!(
            "{WRITTEN}\n[[plans]]\nid = \"docs\"\nrepository = \"nas\"\nsources = [\"/home/demo/docs\"]\n"
        );
        assert_eq!(changed(&two, |parts| parts.plans.reverse()), None);
    }

    #[test]
    fn an_edit_larger_than_a_file_may_be_is_not_made() {
        let comment = "#".repeat(usize::try_from(MAX_CONFIG_BYTES).expect("fits") - WRITTEN.len());
        let written = format!("{WRITTEN}{comment}");
        assert!(changed(&written, |_| {}).is_some(), "at the cap");
        let saved = changed(&written, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(saved, None);
    }
}
