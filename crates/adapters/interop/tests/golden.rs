//! The profile the writer makes, read by the stock `rustic` 0.11.4: it
//! accepts every key, and reads each as the plan says it. The stock CLI is
//! not a dependency of the build, so a machine without it skips the check.

mod common;

use std::collections::BTreeMap;
use std::process::{Command, Stdio};

use app::ports::{PlanExport, ProfileWriter};
use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::planning::{
    GroupBy, HookEntry, Hooks, Hostname, KeepCount, KeepWithin, Maintenance, OnFailure, Plan,
    RepositoryConfig, RepositorySettings, Source,
};
use domain::secrets::{FileRead, SecretRef};
use domain::{AgentId, PlanId, RepositoryAlias, Uuid};
use interop::RusticProfileWriter;
use tempfile::tempdir;

const AGENT: &str = "00000000-0000-0000-0000-000000000001";

/// Hooks that run `true` and print nothing, so rustic's output stays TOML.
fn quiet() -> Hooks {
    let hook = |on_failure| HookEntry::new("true", ["a b"], on_failure).expect("a hook");
    Hooks {
        run_before: vec![hook(OnFailure::Error), hook(OnFailure::Warn)],
        run_after: vec![hook(OnFailure::Ignore)],
        run_failed: vec![hook(OnFailure::Error)],
        run_finally: vec![hook(OnFailure::Error)],
    }
}

/// A plan that sets every key the writer knows.
fn export() -> PlanExport {
    let id = PlanId::parse("home").expect("a plan ID");
    let mut plan = Plan::new(
        id.clone(),
        RepositoryAlias::parse("nas").expect("an alias"),
        vec![
            Source::parse("/home/demo").expect("a source"),
            Source::parse("/srv/demo data").expect("a source"),
        ],
    );
    plan.host = Some(Hostname::parse("demo-laptop").expect("a hostname"));
    plan.label = Some("demo home".to_owned());
    plan.tags = vec![UserTag::new("laptop").expect("a tag")];
    plan.filter.globs = vec!["*.tmp".to_owned()];
    plan.filter.iglobs = vec!["*.BAK".to_owned()];
    plan.filter.glob_files = vec![domain::planning::GlobFile::parse("/etc/globs").unwrap()];
    plan.filter.iglob_files = vec![domain::planning::GlobFile::parse("/etc/iglobs").unwrap()];
    plan.filter.git_ignore = true;
    plan.filter.no_require_git = true;
    plan.filter.custom_ignorefiles = vec![".ignore".to_owned()];
    plan.filter.exclude_if_present = vec![".nobackup".to_owned()];
    plan.filter.exclude_if_xattr = vec!["user.skip".to_owned()];
    plan.filter.one_file_system = true;
    plan.hooks = quiet();
    let count = |n| Some(KeepCount::new(n).expect("a count"));
    let within = |text: &str| Some(KeepWithin::parse(text).expect("a span"));
    plan.retention.keep_last = count(3);
    plan.retention.keep_daily = count(7);
    plan.retention.keep_yearly = count(-1);
    plan.retention.keep_within = within("1y 2mo");
    plan.retention.keep_within_weekly = within("4w");
    let options = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };
    PlanExport {
        plan,
        repository: RepositoryConfig {
            id: None,
            settings: RepositorySettings {
                repository: "/srv/demo-repo".to_owned(),
                repo_hot: Some("/srv/demo-hot".to_owned()),
                password: SecretRef::File {
                    path: "/srv/demo-password".to_owned(),
                    read: FileRead::FirstLine,
                },
                options: options(&[("retry", "3"), ("region", "eu-demo-1")]),
                options_hot: options(&[("region", "hot-1")]),
                options_cold: options(&[("region", "cold-1")]),
            },
            maintenance: Maintenance::Local,
            group_by: GroupBy::default(),
            hooks: quiet(),
        },
        agent_hooks: quiet(),
        agent_hostname: None,
        tags: PlanTags::new(id, AgentId::from_uuid(Uuid::from_u128(1))),
    }
}

fn at<'a>(value: &'a toml::Value, path: &str) -> &'a toml::Value {
    path.split('.')
        .try_fold(value, |value, key| match value {
            toml::Value::Table(table) => table.get(key),
            toml::Value::Array(items) => items.get(key.parse::<usize>().ok()?),
            _ => None,
        })
        .unwrap_or_else(|| panic!("rustic shows nothing at {path}"))
}

#[test]
fn the_stock_rustic_reads_every_key_the_writer_writes() {
    let mut version = Command::new("rustic");
    version
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if spawn_gate::hold(|| version.spawn())
        .and_then(|mut child| child.wait())
        .is_err()
    {
        testkit::skip!("rustic is not on the PATH");
    }
    let export = export();
    let written = RusticProfileWriter::new()
        .render(&export)
        .expect("the export is made");
    let scratch = tempdir().expect("a scratch folder");
    let profile = scratch.path().join("exported.toml");
    std::fs::write(&profile, &written.text).expect("the profile");
    let mut rustic = Command::new("rustic");
    rustic
        .arg("-P")
        .arg(&profile)
        .arg("show-config")
        .current_dir(scratch.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", scratch.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = spawn_gate::hold(|| rustic.spawn())
        .expect("rustic runs")
        .wait_with_output()
        .expect("rustic runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "rustic refused the profile: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        written.text
    );
    let shown: toml::Value = toml::from_str(&stdout)
        .unwrap_or_else(|error| panic!("show-config is not TOML: {error}\n{stdout}"));
    let text = |path: &str| at(&shown, path).as_str().expect("text").to_owned();
    let list = |path: &str| -> Vec<String> {
        at(&shown, path)
            .as_array()
            .expect("a list")
            .iter()
            .map(|item| item.as_str().expect("text").to_owned())
            .collect()
    };

    assert_eq!(text("repository.repository"), "/srv/demo-repo");
    assert_eq!(text("repository.repo-hot"), "/srv/demo-hot");
    assert_eq!(text("repository.password-file"), "/srv/demo-password");
    assert_eq!(text("repository.options.region"), "eu-demo-1");
    assert_eq!(text("repository.options.retry"), "3");
    assert_eq!(text("repository.options-hot.region"), "hot-1");
    assert_eq!(text("repository.options-cold.region"), "cold-1");

    assert_eq!(text("backup.snapshots.0.name"), "home");
    assert_eq!(
        list("backup.snapshots.0.sources"),
        ["/home/demo", "/srv/demo data"]
    );
    assert_eq!(text("backup.snapshots.0.label"), "demo home");
    assert_eq!(text("backup.snapshots.0.host"), "demo-laptop");
    assert_eq!(
        list("backup.snapshots.0.tags"),
        [
            "laptop".to_owned(),
            "ladderback-plan:home".to_owned(),
            format!("ladderback-agent:{AGENT}"),
        ]
    );
    for (key, expected) in [
        ("globs", "*.tmp"),
        ("iglobs", "*.BAK"),
        ("glob-files", "/etc/globs"),
        ("iglob-files", "/etc/iglobs"),
        ("custom-ignorefiles", ".ignore"),
        ("exclude-if-present", ".nobackup"),
        ("exclude-if-xattr", "user.skip"),
    ] {
        assert_eq!(
            list(&format!("backup.snapshots.0.{key}")),
            [expected],
            "{key}"
        );
    }
    for key in ["git-ignore", "no-require-git", "one-file-system"] {
        assert_eq!(
            at(&shown, &format!("backup.snapshots.0.{key}")).as_bool(),
            Some(true),
            "{key}"
        );
    }
    // The hooks at the three levels: the ones that fail the operation are
    // strings, the others tables rustic reads as such.
    for level in [
        "global.hooks",
        "repository.hooks",
        "backup.snapshots.0.hooks",
    ] {
        assert_eq!(
            at(&shown, &format!("{level}.run-before.0")).as_str(),
            Some("true 'a b'"),
            "{level}"
        );
        let table = at(&shown, &format!("{level}.run-before.1"));
        assert_eq!(
            table.get("command").and_then(toml::Value::as_str),
            Some("true"),
            "{level}"
        );
        assert_eq!(
            table.get("on-failure").and_then(toml::Value::as_str),
            Some("warn"),
            "{level}"
        );
        assert_eq!(
            at(&shown, &format!("{level}.run-after.0"))
                .get("on-failure")
                .and_then(toml::Value::as_str),
            Some("ignore"),
            "{level}"
        );
    }

    assert_eq!(text("forget.group-by"), "");
    assert_eq!(at(&shown, "forget.prune").as_bool(), Some(false));
    assert_eq!(at(&shown, "forget.keep-last").as_integer(), Some(3));
    assert_eq!(at(&shown, "forget.keep-daily").as_integer(), Some(7));
    assert_eq!(at(&shown, "forget.keep-yearly").as_integer(), Some(-1));
    // rustic shows the spans in its own words: both must read as the plan's.
    for (key, original) in [("keep-within", "1y 2mo"), ("keep-within-weekly", "4w")] {
        assert_eq!(
            KeepWithin::parse(&text(&format!("forget.{key}"))).expect("a span"),
            KeepWithin::parse(original).expect("a span"),
            "{key}"
        );
    }
    // One entry, whose two tags rustic ANDs; in whatever order it keeps them.
    let filter = list("snapshot-filter.filter-tags");
    assert_eq!(filter.len(), 1, "{filter:?}");
    let mut tags: Vec<_> = filter[0].split(',').collect();
    tags.sort_unstable();
    assert_eq!(
        tags,
        [
            format!("ladderback-agent:{AGENT}").as_str(),
            "ladderback-plan:home"
        ]
    );
}
