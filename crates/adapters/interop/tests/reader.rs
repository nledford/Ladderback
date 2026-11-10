//! What the reader makes of one rustic profile (technical design §6.2): the
//! strictness rustic 0.11.4 has for each table, what becomes a plan, a
//! repository and hooks, what is left out with a notice, and what is refused.

mod common;

use app::ports::{ProfileError, ProfileNotice, ProfileProblem, ProfileReader};
use common::{
    ENTRY, REPOSITORY, Scratch, commands, ids, ignored, line_of, nas, plan, read_err, read_ok,
    write_in,
};
use domain::planning::{GroupBy, KeepCount, KeepWithin, Maintenance, OnFailure};
use domain::secrets::{FileRead, Part, Reference, SecretRef, SecretValue, Template};
use interop::{MAX_ENTRIES, MAX_ITEMS, MAX_LEFT_OUT_KEYS};

/// The key, the line and the problem of a refusal for an invalid profile.
fn invalid(error: ProfileError) -> (String, Option<usize>, ProfileProblem) {
    match error {
        ProfileError::Invalid {
            key, at, problem, ..
        } => (key.unwrap_or_default(), at.map(|at| at.line), problem),
        other => panic!("the profile should be invalid, got {other:?}"),
    }
}

/// The key and the line of a refusal for something Ladderback can't honour.
fn unsupported(error: ProfileError) -> (String, Option<usize>) {
    match error {
        ProfileError::Unsupported { key, at, .. } => (key, at.map(|at| at.line)),
        other => panic!("the profile should be unsupported, got {other:?}"),
    }
}

fn profile_text(extra: &str) -> String {
    format!("{REPOSITORY}\n{extra}\n{ENTRY}")
}

// --- The plain case ---------------------------------------------------------

#[tokio::test]
async fn a_profile_gives_its_repository_and_a_plan_for_each_snapshot_entry() {
    let profile = read_ok(
        "\
[repository]
repository = \"/srv/demo-repo\"
repo-hot = \"/srv/demo-hot\"
password-file = \"/srv/demo-password\"

[repository.options]
endpoint = \"https://demo.example.lan\"

[repository.options-hot]
region = \"demo-1\"

[repository.options-cold]
tier = \"cold\"

[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\", \"/etc/demo\"]

[[backup.snapshots]]
name = \"docs\"
sources = [\"/home/demo/docs\"]
",
    )
    .await;

    let settings = &profile.repository.settings;
    assert_eq!(settings.repository, "/srv/demo-repo");
    assert_eq!(settings.repo_hot.as_deref(), Some("/srv/demo-hot"));
    assert_eq!(
        settings.password,
        SecretRef::File {
            path: "/srv/demo-password".to_owned(),
            read: FileRead::FirstLine,
        }
    );
    assert_eq!(settings.options["endpoint"], "https://demo.example.lan");
    assert_eq!(settings.options_hot["region"], "demo-1");
    assert_eq!(settings.options_cold["tier"], "cold");
    assert_eq!(profile.repository.maintenance, Maintenance::None);

    let mut found = ids(&profile);
    found.sort_unstable();
    assert_eq!(found, ["docs", "home"]);
    let home = plan(&profile, "home");
    assert_eq!(home.repository, nas(), "every plan backs up into the alias");
    let sources: Vec<_> = home.sources.iter().map(|source| source.as_str()).collect();
    assert_eq!(sources, ["/home/demo", "/etc/demo"]);
    assert!(profile.notices.is_empty(), "{:?}", profile.notices);
}

#[tokio::test]
async fn a_snapshot_entry_inherits_the_backup_section_and_replaces_its_lists() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[backup]
git-ignore = true
no-require-git = true
one-file-system = true
label = \"laptop\"
host = \"demo-laptop\"
globs = [\"!*.tmp\"]
iglobs = [\"!*.BAK\"]
glob-files = [\"/etc/demo/globs\"]
iglob-files = [\"/etc/demo/iglobs\"]
custom-ignorefiles = [\".demoignore\"]
exclude-if-present = [\".nobackup\", \"CACHEDIR.TAG\"]
exclude-if-xattr = [\"user.skip\"]
tags = [\"all\"]

[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\"]

[[backup.snapshots]]
name = \"docs\"
label = \"documents\"
host = \"demo-desktop\"
sources = [\"/home/demo/docs\"]
exclude-if-present = [\".skip\"]
tags = [\"docs\"]
"
    ))
    .await;

    let home = plan(&profile, "home");
    assert!(home.filter.git_ignore && home.filter.no_require_git && home.filter.one_file_system);
    assert_eq!(home.label.as_deref(), Some("laptop"));
    assert_eq!(
        home.host.as_ref().map(|host| host.as_str()),
        Some("demo-laptop")
    );
    assert_eq!(home.filter.globs, ["!*.tmp"]);
    assert_eq!(home.filter.iglobs, ["!*.BAK"]);
    assert_eq!(home.filter.glob_files, ["/etc/demo/globs"]);
    assert_eq!(home.filter.iglob_files, ["/etc/demo/iglobs"]);
    assert_eq!(home.filter.custom_ignorefiles, [".demoignore"]);
    assert_eq!(
        home.filter.exclude_if_present,
        [".nobackup", "CACHEDIR.TAG"]
    );
    assert_eq!(home.filter.exclude_if_xattr, ["user.skip"]);
    let tags: Vec<_> = home.tags.iter().map(|tag| tag.as_str()).collect();
    assert_eq!(tags, ["all"]);

    let docs = plan(&profile, "docs");
    assert!(
        docs.filter.git_ignore,
        "a bool the backup section sets stays set"
    );
    assert_eq!(
        docs.filter.exclude_if_present,
        [".skip"],
        "the entry's list replaces"
    );
    assert_eq!(
        docs.filter.globs,
        ["!*.tmp"],
        "a list the entry leaves empty is inherited"
    );
    assert_eq!(docs.label.as_deref(), Some("documents"));
    assert_eq!(
        docs.host.as_ref().map(|host| host.as_str()),
        Some("demo-desktop")
    );
    let tags: Vec<_> = docs.tags.iter().map(|tag| tag.as_str()).collect();
    assert_eq!(tags, ["docs"]);
}

#[tokio::test]
async fn a_profile_with_no_snapshot_entry_gives_the_repository_and_no_plan() {
    let profile = read_ok(REPOSITORY).await;
    assert!(profile.plans.is_empty());
    assert_eq!(profile.repository.settings.repository, "/srv/demo-repo");
}

#[tokio::test]
async fn entries_come_sorted_by_name_and_sources_and_one_of_two_alike_is_kept() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[[backup.snapshots]]
name = \"zeta\"
sources = [\"/srv/zeta\"]

[[backup.snapshots]]
name = \"alpha\"
sources = [\"/srv/b\"]

[[backup.snapshots]]
name = \"alpha\"
sources = [\"/srv/a\"]

[[backup.snapshots]]
name = \"alpha\"
sources = [\"/srv/a\"]
label = \"the second one\"
"
    ))
    .await;

    let found: Vec<_> = profile
        .plans
        .iter()
        .map(|plan| {
            (
                plan.id.as_str(),
                plan.sources[0].as_str(),
                plan.label.clone(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("alpha", "/srv/a", None),
            ("alpha", "/srv/b", None),
            ("zeta", "/srv/zeta", None),
        ],
        "rustic sorts the entries by name, then sources, and drops a repeat"
    );
}

// --- A plan's ID ------------------------------------------------------------

#[tokio::test]
async fn a_plan_id_is_the_name_else_the_label_else_the_folder_else_plan() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[[backup.snapshots]]
name = \"Home Dir\"
label = \"Ignored Label\"
sources = [\"/home/demo\"]

[[backup.snapshots]]
label = \"Work Laptop Docs\"
sources = [\"/home/demo/work\"]

[[backup.snapshots]]
sources = [\"/srv/demo data/\", \"/srv/other\"]

[[backup.snapshots]]
sources = [\"/\"]
"
    ))
    .await;

    let mut found = ids(&profile);
    found.sort_unstable();
    assert_eq!(found, ["demo-data", "home-dir", "plan", "work-laptop-docs"]);
    assert_eq!(
        plan(&profile, "work-laptop-docs").label.as_deref(),
        Some("Work Laptop Docs")
    );
    assert_eq!(
        plan(&profile, "home-dir").label.as_deref(),
        Some("Ignored Label")
    );
}

#[tokio::test]
async fn two_entries_that_get_one_id_are_both_returned_for_the_use_case_to_refuse() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[[backup.snapshots]]
sources = [\"/home/demo/data\"]

[[backup.snapshots]]
sources = [\"/srv/demo/data\"]
"
    ))
    .await;
    assert_eq!(ids(&profile), ["data", "data"]);
}

// --- Tags -------------------------------------------------------------------

#[tokio::test]
async fn tags_are_split_at_commas_and_reserved_ones_are_dropped_with_a_notice() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\"]
tags = [\"laptop,nightly\", \"Ladderback-Plan:old\", \"ladderback-agent:x,kept\", \"\"]
"
    ))
    .await;
    let tags: Vec<_> = plan(&profile, "home")
        .tags
        .iter()
        .map(|tag| tag.as_str())
        .collect();
    assert_eq!(tags, ["laptop", "nightly", "kept"]);
    for dropped in ["Ladderback-Plan:old", "ladderback-agent:x"] {
        assert!(
            profile.notices.contains(&ProfileNotice::TagDropped {
                tag: dropped.to_owned()
            }),
            "{dropped}: {:?}",
            profile.notices
        );
    }
}

// --- Strictness -------------------------------------------------------------

#[tokio::test]
async fn a_key_rustic_does_not_know_is_refused_where_it_is_written() {
    // The text, the key the refusal names, and what is on its line.
    let cases = [
        ("mystery = 1\n", "mystery"),
        ("[global]\nmystery = 1\n", "global.mystery"),
        ("[global.hooks]\nmystery = 1\n", "global.hooks.mystery"),
        ("[backup]\nmystery = 1\n", "backup.mystery"),
        ("[backup.hooks]\nmystery = 1\n", "backup.hooks.mystery"),
        (
            "[[backup.snapshots]]\nsources = [\"/home/demo\"]\nmystery = 1\n",
            "backup.snapshots[0].mystery",
        ),
        (
            "[[backup.snapshots]]\nsources = [\"/home/demo\"]\n[backup.snapshots.hooks]\nmystery = 1\n",
            "backup.snapshots[0].hooks.mystery",
        ),
        ("[forget]\nmystery = 1\n", "forget.mystery"),
        (
            "[snapshot-filter]\nmystery = 1\n",
            "snapshot-filter.mystery",
        ),
        (
            "[repository.hooks]\nmystery = 1\n",
            "repository.hooks.mystery",
        ),
    ];
    for (fragment, key) in cases {
        let text = format!("{fragment}\n{REPOSITORY}");
        let (found, line, problem) = invalid(read_err(&text).await);
        assert_eq!(found, key, "{fragment}");
        assert_eq!(line, Some(line_of(&text, "mystery")), "{fragment}");
        assert_eq!(problem, ProfileProblem::UnknownKey, "{fragment}");
    }
}

#[tokio::test]
async fn the_repository_table_ignores_what_it_does_not_know_and_says_so() {
    let profile = read_ok(&profile_text("")).await;
    assert!(profile.notices.is_empty());

    let text = format!("{REPOSITORY}mystery = 1\n\n{ENTRY}");
    let profile = read_ok(&text).await;
    assert_eq!(profile.notices, [ignored("repository.mystery")]);
}

#[tokio::test]
async fn keys_rustic_knows_and_ladderback_does_not_carry_are_noted_once_each() {
    let profile = read_ok(&format!(
        "\
[global]
log-level = \"debug\"
dry-run = true

{REPOSITORY}no-cache = true
warm-up = true

[backup]
group-by = \"host\"
force = true

[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\"]
force = true
description = \"demo\"

[[backup.snapshots]]
name = \"docs\"
sources = [\"/home/demo/docs\"]
force = true
"
    ))
    .await;

    let mut keys: Vec<_> = profile
        .notices
        .iter()
        .filter_map(|notice| match notice {
            ProfileNotice::KeyIgnored { key } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "backup.force",
            "backup.group-by",
            "backup.snapshots.description",
            "backup.snapshots.force",
            "global.dry-run",
            "global.log-level",
            "repository.no-cache",
            "repository.warm-up",
        ]
    );
}

#[tokio::test]
async fn sections_of_other_purposes_are_one_notice_each_and_empty_ones_none() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[copy]
targets = [\"/srv/copy\"]

[webdav]
address = \"localhost:8000\"

[mount]
mystery = 1

[init]
mystery = 1

[snapshot-filter]
filter-hosts = [\"demo-laptop\"]
filter-tags = [\"a\", \"b\"]
"
    ))
    .await;
    for key in ["copy", "webdav", "mount", "init", "snapshot-filter"] {
        let count = profile
            .notices
            .iter()
            .filter(|notice| **notice == ignored(key))
            .count();
        assert_eq!(count, 1, "{key}: {:?}", profile.notices);
    }

    let profile = read_ok(&format!(
        "{REPOSITORY}\n[copy]\n[webdav]\n[snapshot-filter]\n"
    ))
    .await;
    assert!(profile.notices.is_empty(), "{:?}", profile.notices);
}

#[tokio::test]
async fn forget_filters_are_known_keys_that_are_left_out() {
    let profile = read_ok(&format!(
        "{REPOSITORY}\n[forget]\nkeep-last = 2\nfilter-tags = [\"a\"]\n"
    ))
    .await;
    assert!(profile.notices.contains(&ignored("forget.filter-tags")));
}

#[tokio::test]
async fn a_name_or_sources_on_the_backup_section_and_snapshots_in_an_entry_are_refused() {
    let cases = [
        (
            "[backup]\nsources = [\"/home/demo\"]\n",
            "backup.sources",
            "sources",
        ),
        ("[backup]\nname = \"home\"\n", "backup.name", "name"),
        (
            "[[backup.snapshots]]\nsources = [\"/home/demo\"]\n[[backup.snapshots.snapshots]]\nsources = [\"/srv\"]\n",
            "backup.snapshots[0].snapshots",
            "backup.snapshots.snapshots",
        ),
    ];
    for (fragment, key, on_line) in cases {
        let text = format!("{fragment}\n{REPOSITORY}");
        let (found, line, problem) = invalid(read_err(&text).await);
        assert_eq!(found, key, "{fragment}");
        assert_eq!(line, Some(line_of(&text, on_line)), "{fragment}");
        assert_eq!(problem, ProfileProblem::NotHere, "{fragment}");
    }
}

#[tokio::test]
async fn a_backup_that_reads_its_source_from_a_command_is_refused() {
    let text = profile_text("[backup]\nstdin-command = \"echo demo\"\n");
    let (key, line) = unsupported(read_err(&text).await);
    assert_eq!(key, "backup.stdin-command");
    assert_eq!(line, Some(line_of(&text, "stdin-command")));
}

#[tokio::test]
async fn a_file_that_is_not_toml_is_refused_where_the_parser_stopped_and_without_its_text() {
    let text = "[repository\npassword = \"CANARY-inline-password\"\n";
    let error = read_err(text).await;
    let shown = format!("{error:?}");
    assert!(!shown.contains("CANARY"), "{shown}");
    let (key, line, problem) = invalid(error);
    assert_eq!(key, "");
    assert_eq!(line, Some(1));
    assert_eq!(problem, ProfileProblem::Syntax);
}

#[tokio::test]
async fn a_value_of_the_wrong_type_is_refused_at_its_key() {
    let text = profile_text("[forget]\nkeep-last = \"three\"\n");
    let (key, line, problem) = invalid(read_err(&text).await);
    assert_eq!(key, "forget.keep-last");
    assert_eq!(line, Some(line_of(&text, "keep-last")));
    assert_eq!(problem, ProfileProblem::WrongType);
}

#[tokio::test]
async fn an_entry_without_sources_is_refused_at_the_entry() {
    let text = format!("{REPOSITORY}\n[[backup.snapshots]]\nname = \"home\"\n");
    let (key, line, problem) = invalid(read_err(&text).await);
    assert_eq!(key, "backup.snapshots[0].sources");
    assert!(line.is_some(), "the entry is pointed at");
    assert_eq!(problem, ProfileProblem::Missing);
}

#[tokio::test]
async fn a_profile_without_a_repository_is_refused() {
    let (key, _, problem) = invalid(read_err(ENTRY).await);
    assert_eq!(key, "repository.repository");
    assert_eq!(problem, ProfileProblem::Missing);
}

// --- The file ---------------------------------------------------------------

#[tokio::test]
async fn a_profile_over_one_mebibyte_is_refused_unread_and_one_at_the_cap_is_read() {
    let scratch = Scratch::new();
    let cap = 1024 * 1024;
    let mut text = profile_text("");
    text.push_str("# ");
    text.push_str(&"x".repeat(cap - text.len() - 1));
    text.push('\n');
    assert_eq!(text.len(), cap);
    assert!(
        scratch.read(&text).await.is_ok(),
        "a file at the cap is read"
    );

    text.push('#');
    let file = scratch.profile("nas.toml", &text);
    let error = scratch.reader().read(&file, &nas()).await.unwrap_err();
    assert_eq!(
        error,
        ProfileError::TooLarge {
            file: file.display().to_string(),
            limit: 1024 * 1024,
        }
    );
}

#[tokio::test]
async fn a_profile_that_does_not_exist_or_is_not_a_file_is_unreadable() {
    let scratch = Scratch::new();
    for path in [scratch.work.join("missing.toml"), scratch.work.clone()] {
        let error = scratch.reader().read(&path, &nas()).await.unwrap_err();
        assert_eq!(
            error,
            ProfileError::Unreadable {
                file: path.display().to_string()
            }
        );
    }
}

#[tokio::test]
async fn a_profile_that_is_not_text_is_invalid() {
    let scratch = Scratch::new();
    let file = scratch.work.join("nas.toml");
    std::fs::write(&file, [0xff, 0xfe, 0x00]).expect("a file");
    let error = scratch.reader().read(&file, &nas()).await.unwrap_err();
    let (_, _, problem) = invalid(error);
    assert_eq!(problem, ProfileProblem::Syntax);
}

// --- Sources ----------------------------------------------------------------

#[tokio::test]
async fn a_source_that_is_not_an_absolute_path_is_refused_at_the_source() {
    for source in ["docs", "./docs", "-", "~/docs"] {
        let text = format!(
            "{REPOSITORY}\n[[backup.snapshots]]\nname = \"home\"\nsources = [\"/home/demo\", \"{source}\"]\n"
        );
        let (key, line) = unsupported(read_err(&text).await);
        assert_eq!(key, "backup.snapshots[0].sources[1]", "{source}");
        assert_eq!(line, Some(line_of(&text, "sources")), "{source}");
    }
}

#[tokio::test]
async fn a_host_that_is_not_a_host_name_is_refused() {
    let text = profile_text("[backup]\nhost = \"\"\n");
    let (key, line, problem) = invalid(read_err(&text).await);
    assert_eq!(key, "backup.host");
    assert_eq!(line, Some(line_of(&text, "host")));
    assert_eq!(problem, ProfileProblem::BadValue);
}

// --- Retention, pruning and grouping ---------------------------------------

#[tokio::test]
async fn every_plan_gets_the_retention_of_the_forget_section() {
    let profile = read_ok(&format!(
        "\
{REPOSITORY}
[forget]
keep-last = 3
keep-daily = 7
keep-yearly = -1
keep-within-weekly = \"2 weeks\"

[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\"]

[[backup.snapshots]]
name = \"docs\"
sources = [\"/home/demo/docs\"]
"
    ))
    .await;
    for id in ["home", "docs"] {
        let retention = &plan(&profile, id).retention;
        assert_eq!(retention.keep_last.map(KeepCount::get), Some(3), "{id}");
        assert_eq!(retention.keep_daily.map(KeepCount::get), Some(7), "{id}");
        assert_eq!(retention.keep_yearly.map(KeepCount::get), Some(-1), "{id}");
        assert_eq!(
            retention.keep_within_weekly,
            Some(KeepWithin::parse("2 weeks").expect("a span")),
            "{id}"
        );
        assert_eq!(retention.keep_hourly, None, "{id}");
    }
}

#[tokio::test]
async fn a_rule_that_keeps_particular_snapshots_is_refused_not_ignored() {
    for (rule, key) in [
        ("keep-tags = [\"precious\"]", "forget.keep-tags"),
        ("keep-ids = [\"abcd1234\"]", "forget.keep-ids"),
    ] {
        let text = profile_text(&format!("[forget]\nkeep-last = 3\n{rule}\n"));
        let (found, line) = unsupported(read_err(&text).await);
        assert_eq!(found, key);
        assert_eq!(line, Some(line_of(&text, rule)));
    }

    let profile = read_ok(&profile_text(
        "[forget]\nkeep-last = 3\nkeep-tags = []\nkeep-ids = []\n",
    ))
    .await;
    assert!(
        profile.notices.is_empty(),
        "an empty rule keeps nothing apart"
    );
}

#[tokio::test]
async fn keeping_nothing_and_deleting_unchanged_are_noted_not_copied() {
    let profile = read_ok(&profile_text(
        "[forget]\nkeep-last = 3\nkeep-none = true\ndelete-unchanged = true\n",
    ))
    .await;
    assert!(profile.notices.contains(&ignored("forget.keep-none")));
    assert!(
        profile
            .notices
            .contains(&ignored("forget.delete-unchanged"))
    );
}

#[tokio::test]
async fn a_retention_value_that_is_not_one_is_refused() {
    for (rule, key) in [
        ("keep-last = -2", "forget.keep-last"),
        ("keep-within = \"soon\"", "forget.keep-within"),
    ] {
        let text = profile_text(&format!("[forget]\n{rule}\n"));
        let (found, line, problem) = invalid(read_err(&text).await);
        assert_eq!(found, key);
        assert_eq!(line, Some(line_of(&text, rule)));
        assert_eq!(problem, ProfileProblem::BadValue);
    }
}

#[tokio::test]
async fn prune_is_not_copied_the_repository_asks_for_local_maintenance_and_says_so() {
    let profile = read_ok(&profile_text("[forget]\nkeep-last = 3\nprune = true\n")).await;
    assert_eq!(profile.repository.maintenance, Maintenance::Local);
    assert!(profile.notices.contains(&ProfileNotice::Prune));
    assert!(
        profile
            .plans
            .iter()
            .all(|plan| plan.retention.keep_last.map(KeepCount::get) == Some(3))
    );

    for text in [
        profile_text("[forget]\nkeep-last = 3\n"),
        profile_text("[forget]\nkeep-last = 3\nprune = false\n"),
    ] {
        let profile = read_ok(&text).await;
        assert_eq!(profile.repository.maintenance, Maintenance::None);
        assert!(!profile.notices.contains(&ProfileNotice::Prune));
    }
}

#[tokio::test]
async fn the_forget_sections_grouping_is_the_repositorys_before_the_global_one() {
    let host_paths = GroupBy::parse("host,paths").expect("a grouping");
    let tags = GroupBy::parse("tags").expect("a grouping");

    let both = read_ok(&profile_text(
        "[global]\ngroup-by = \"tags\"\n\n[forget]\ngroup-by = \"host,paths\"\n",
    ))
    .await;
    assert_eq!(both.repository.group_by, host_paths);

    let global = read_ok(&profile_text("[global]\ngroup-by = \"tags\"\n")).await;
    assert_eq!(global.repository.group_by, tags);

    let none = read_ok(&profile_text("")).await;
    assert_eq!(none.repository.group_by, GroupBy::default());

    let empty = read_ok(&profile_text("[forget]\ngroup-by = \"\"\n")).await;
    assert_eq!(
        empty.repository.group_by,
        GroupBy::parse("").expect("nothing")
    );
}

#[tokio::test]
async fn the_backup_sections_grouping_is_left_out_with_a_notice() {
    let profile = read_ok(&profile_text("[backup]\ngroup-by = \"host\"\n")).await;
    assert!(profile.notices.contains(&ignored("backup.group-by")));
    assert_eq!(profile.repository.group_by, GroupBy::default());
}

#[tokio::test]
async fn a_grouping_that_is_not_one_is_refused() {
    let text = profile_text("[forget]\ngroup-by = \"colour\"\n");
    let (key, line, problem) = invalid(read_err(&text).await);
    assert_eq!(key, "forget.group-by");
    assert_eq!(line, Some(line_of(&text, "group-by")));
    assert_eq!(problem, ProfileProblem::BadValue);
}

// --- Hooks ------------------------------------------------------------------

#[tokio::test]
async fn hooks_land_on_the_agent_the_repository_and_each_plan_in_rustics_order() {
    let profile = read_ok(
        "\
[global.hooks]
run-before = [\"echo agent\"]
run-finally = [\"echo agent-finally\"]

[repository]
repository = \"/srv/demo-repo\"
password-file = \"/srv/demo-password\"

[repository.hooks]
run-before = [\"echo repository\"]

[backup.hooks]
run-before = [\"echo backup-before\"]
run-after = [\"echo backup-after\"]
run-failed = [\"echo backup-failed\"]
run-finally = [\"echo backup-finally\"]

[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\"]

[backup.snapshots.hooks]
run-before = [\"echo entry-before\"]
run-after = [\"echo entry-after\"]
run-failed = [\"echo entry-failed\"]
run-finally = [\"echo entry-finally\"]
",
    )
    .await;

    assert_eq!(commands(&profile.hooks.run_before), ["echo agent"]);
    assert_eq!(commands(&profile.hooks.run_finally), ["echo agent-finally"]);
    assert_eq!(
        commands(&profile.repository.hooks.run_before),
        ["echo repository"]
    );
    let hooks = &plan(&profile, "home").hooks;
    assert_eq!(
        commands(&hooks.run_before),
        ["echo backup-before", "echo entry-before"],
        "the backup section's hook is the outer one before"
    );
    assert_eq!(
        commands(&hooks.run_after),
        ["echo entry-after", "echo backup-after"]
    );
    assert_eq!(
        commands(&hooks.run_failed),
        ["echo entry-failed", "echo backup-failed"]
    );
    assert_eq!(
        commands(&hooks.run_finally),
        ["echo entry-finally", "echo backup-finally"]
    );
    assert!(profile.notices.contains(&ProfileNotice::HooksPerPlan));
}

#[tokio::test]
async fn backup_hooks_are_noted_only_when_there_are_some() {
    let profile = read_ok(&format!(
        "{REPOSITORY}\n[[backup.snapshots]]\nname = \"home\"\nsources = [\"/home/demo\"]\n\n[backup.snapshots.hooks]\nrun-before = [\"echo entry\"]\n"
    ))
    .await;
    assert!(!profile.notices.contains(&ProfileNotice::HooksPerPlan));
    assert_eq!(
        commands(&plan(&profile, "home").hooks.run_before),
        ["echo entry"]
    );
}

#[tokio::test]
async fn a_hook_is_a_command_line_or_a_table_with_what_a_failure_means() {
    let profile = read_ok(&profile_text(
        "[global.hooks]\nrun-before = [\"echo 'two words' x\", { command = \"/opt/demo hook\", args = [\"a b\"], on-failure = \"warn\" }]\n",
    ))
    .await;
    assert_eq!(
        commands(&profile.hooks.run_before),
        ["echo two words x", "/opt/demo hook a b"]
    );
    assert_eq!(profile.hooks.run_before[0].on_failure, OnFailure::Error);
    assert_eq!(profile.hooks.run_before[1].on_failure, OnFailure::Warn);
}

#[tokio::test]
async fn a_hook_that_is_not_a_command_is_refused_without_its_text() {
    let text = profile_text("[global.hooks]\nrun-before = [\"echo ok\", \"echo 'CANARY\"]\n");
    let error = read_err(&text).await;
    assert!(!format!("{error:?}").contains("CANARY"));
    let (key, line, problem) = invalid(error);
    assert_eq!(key, "global.hooks.run-before[1]");
    assert_eq!(line, Some(line_of(&text, "run-before")));
    assert_eq!(problem, ProfileProblem::BadValue);
}

#[tokio::test]
async fn the_profiles_environment_is_left_out_with_a_notice_and_never_kept() {
    let profile = read_ok(&profile_text("[global.env]\nDEMO_VARIABLE = \"CANARY\"\n")).await;
    assert!(profile.notices.contains(&ProfileNotice::EnvIgnored));
    assert!(!format!("{profile:?}").contains("CANARY"));
}

// --- The password -----------------------------------------------------------

#[tokio::test]
async fn an_inline_password_is_kept_with_a_notice_that_names_the_field() {
    let profile =
        read_ok("[repository]\nrepository = \"/srv/demo-repo\"\npassword = \"CANARY-inline\"\n")
            .await;
    assert_eq!(
        profile.repository.settings.password,
        SecretRef::Inline(SecretValue::new("CANARY-inline"))
    );
    assert!(profile.notices.contains(&ProfileNotice::InlineSecret {
        field: "repository.password".to_owned()
    }));
    assert!(!format!("{:?}", profile.notices).contains("CANARY"));
}

#[tokio::test]
async fn a_password_command_is_a_command_line_or_a_table() {
    for (written, program, args) in [
        (
            "\"printenv DEMO_PASSWORD\"",
            "printenv",
            vec!["DEMO_PASSWORD"],
        ),
        (
            "{ command = \"/opt/demo vault\", args = [\"get\", \"nas\"] }",
            "/opt/demo vault",
            vec!["get", "nas"],
        ),
    ] {
        let profile = read_ok(&format!(
            "[repository]\nrepository = \"/srv/demo-repo\"\npassword-command = {written}\n"
        ))
        .await;
        let SecretRef::Command(command) = &profile.repository.settings.password else {
            panic!("a command, got {:?}", profile.repository.settings.password);
        };
        assert_eq!(command.program(), program);
        assert_eq!(command.args(), args);
    }
}

#[tokio::test]
async fn with_several_password_sources_the_one_rustic_asks_first_is_kept() {
    let profile = read_ok(
        "\
[repository]
repository = \"/srv/demo-repo\"
password-command = \"printenv DEMO_PASSWORD\"
password-file = \"/srv/demo-password\"
password = \"CANARY-inline\"
",
    )
    .await;
    assert_eq!(
        profile.repository.settings.password,
        SecretRef::Inline(SecretValue::new("CANARY-inline"))
    );
    assert!(
        profile
            .notices
            .contains(&ignored("repository.password-file"))
    );
    assert!(
        profile
            .notices
            .contains(&ignored("repository.password-command"))
    );

    let profile = read_ok(
        "\
[repository]
repository = \"/srv/demo-repo\"
password-command = \"printenv DEMO_PASSWORD\"
password-file = \"/srv/demo-password\"
",
    )
    .await;
    assert!(matches!(
        profile.repository.settings.password,
        SecretRef::File { .. }
    ));
    assert!(
        profile
            .notices
            .contains(&ignored("repository.password-command"))
    );
}

#[tokio::test]
async fn a_password_file_that_is_not_an_absolute_path_is_refused() {
    let text = "[repository]\nrepository = \"/srv/demo-repo\"\npassword-file = \"password\"\n";
    let (key, line) = unsupported(read_err(text).await);
    assert_eq!(key, "repository.password-file");
    assert_eq!(line, Some(3));
}

#[tokio::test]
async fn a_profile_with_nowhere_to_read_the_password_from_is_refused() {
    let (key, line) =
        unsupported(read_err("[repository]\nrepository = \"/srv/demo-repo\"\n").await);
    assert_eq!(key, "repository.password");
    assert_eq!(line, None);
}

#[tokio::test]
async fn a_key_is_noted_and_the_password_source_stays_and_a_key_alone_is_refused() {
    let profile = read_ok(&format!(
        "{REPOSITORY}key = \"CANARY-key\"\nkey-file = \"/srv/demo-key\"\nkey-command = \"echo key\"\n"
    ))
    .await;
    for key in [
        "repository.key",
        "repository.key-file",
        "repository.key-command",
    ] {
        assert!(profile.notices.contains(&ignored(key)), "{key}");
    }
    assert!(matches!(
        profile.repository.settings.password,
        SecretRef::File { .. }
    ));
    assert!(!format!("{profile:?}").contains("CANARY"));

    let (key, _) = unsupported(
        read_err("[repository]\nrepository = \"/srv/demo-repo\"\nkey-file = \"/srv/demo-key\"\n")
            .await,
    );
    assert_eq!(key, "repository.password");
}

#[tokio::test]
async fn an_option_that_holds_a_credential_is_noted_by_name() {
    let profile = read_ok(&format!(
        "{REPOSITORY}\n[repository.options]\nsecret_access_key = \"CANARY-secret\"\nregion = \"demo-1\"\n"
    ))
    .await;
    assert_eq!(
        profile.notices,
        [ProfileNotice::InlineSecret {
            field: "repository.options.secret_access_key".to_owned()
        }]
    );
    assert!(!format!("{:?}", profile.notices).contains("CANARY"));
}

// --- Text that stays text ---------------------------------------------------

/// The text a repository field holds, as the daemon will read it: `${`
/// written in the profile has to come out as itself.
fn as_text(field: &str) -> String {
    let template = Template::parse(field).unwrap_or_else(|error| panic!("{field}: {error:?}"));
    match template.parts() {
        [] => String::new(),
        [Part::Text(text)] => text.clone(),
        other => panic!("{field} is not plain text: {other:?}"),
    }
}

#[tokio::test]
async fn a_reference_in_a_repository_value_is_written_so_that_it_stays_text() {
    let profile = read_ok(
        "\
[repository]
repository = \"s3:${file:/etc/passwd}/bucket\"
repo-hot = \"${HOME}\"
password-file = \"/srv/demo-password\"

[repository.options]
endpoint = \"https://${HOME}/path\"
literal = \"$${HOME}\"
dollars = \"$$${HOME}$\"

[repository.options-hot]
hot = \"${A}${B}\"

[repository.options-cold]
cold = \"${\"
",
    )
    .await;

    let settings = &profile.repository.settings;
    assert_eq!(settings.repository, "s3:$${file:/etc/passwd}/bucket");
    assert_eq!(
        as_text(&settings.repository),
        "s3:${file:/etc/passwd}/bucket"
    );
    let hot = settings.repo_hot.as_deref().expect("a hot repository");
    assert_eq!(hot, "$${HOME}");
    assert_eq!(as_text(hot), "${HOME}");
    assert_eq!(settings.options["endpoint"], "https://$${HOME}/path");
    assert_eq!(
        as_text(&settings.options["endpoint"]),
        "https://${HOME}/path"
    );
    assert_eq!(as_text(&settings.options["literal"]), "$${HOME}");
    assert_eq!(as_text(&settings.options["dollars"]), "$$${HOME}$");
    assert_eq!(as_text(&settings.options_hot["hot"]), "${A}${B}");
    assert_eq!(as_text(&settings.options_cold["cold"]), "${");
}

// --- rustic's variables -----------------------------------------------------

#[tokio::test]
async fn rustics_password_variable_becomes_a_reference_when_the_profile_has_no_source() {
    let scratch = Scratch::new();
    let profile = scratch
        .read_with(
            &[("RUSTIC_PASSWORD", "CANARY-environment-password")],
            "[repository]\nrepository = \"/srv/demo-repo\"\n",
        )
        .await
        .expect("a profile");
    assert_eq!(
        profile.repository.settings.password,
        SecretRef::Env("RUSTIC_PASSWORD".to_owned())
    );
    assert!(!format!("{profile:?}").contains("CANARY"));
}

#[tokio::test]
async fn rustics_password_file_and_command_variables_fill_the_gap_too() {
    let scratch = Scratch::new();
    let profile = scratch
        .read_with(
            &[("RUSTIC_PASSWORD_FILE", "/srv/demo-password")],
            "[repository]\nrepository = \"/srv/demo-repo\"\n",
        )
        .await
        .expect("a profile");
    assert_eq!(
        profile.repository.settings.password,
        SecretRef::File {
            path: "/srv/demo-password".to_owned(),
            read: FileRead::FirstLine,
        }
    );

    let profile = scratch
        .read_with(
            &[("RUSTIC_PASSWORD_COMMAND", "printenv DEMO_PASSWORD")],
            "[repository]\nrepository = \"/srv/demo-repo\"\n",
        )
        .await
        .expect("a profile");
    assert!(matches!(
        profile.repository.settings.password,
        SecretRef::Command(_)
    ));

    let error = scratch
        .read_with(
            &[("RUSTIC_PASSWORD_FILE", "password")],
            "[repository]\nrepository = \"/srv/demo-repo\"\n",
        )
        .await
        .unwrap_err();
    assert_eq!(unsupported(error).0, "RUSTIC_PASSWORD_FILE");
}

#[tokio::test]
async fn a_password_source_the_profile_names_beats_rustics_variable() {
    let scratch = Scratch::new();
    let profile = scratch
        .read_with(&[("RUSTIC_PASSWORD", "CANARY")], &profile_text(""))
        .await
        .expect("a profile");
    assert!(matches!(
        profile.repository.settings.password,
        SecretRef::File { .. }
    ));
}

#[tokio::test]
async fn rustics_repository_and_option_variables_fill_what_the_profile_leaves_out_as_references() {
    let scratch = Scratch::new();
    let profile = scratch
        .read_with(
            &[
                ("RUSTIC_REPOSITORY", "/srv/CANARY-repo"),
                ("RUSTIC_REPO_OPT_ENDPOINT", "https://CANARY.example.lan"),
                ("RUSTIC_REPO_OPT_FOO_BAR", "x"),
                ("RUSTIC_REPO_OPTHOT_REGION", "y"),
                ("RUSTIC_REPO_OPTCOLD_TIER", "z"),
                ("RUSTIC_REPO_OPT_OWN", "not used"),
                ("RUSTIC_DRY_RUN", "true"),
                ("OTHER", "unrelated"),
            ],
            "\
[repository]
password-file = \"/srv/demo-password\"

[repository.options]
own = \"the profile's\"
",
        )
        .await
        .expect("a profile");

    let settings = &profile.repository.settings;
    assert_eq!(
        settings.repository, "${RUSTIC_REPOSITORY}",
        "not escaped: made here"
    );
    assert_eq!(
        Template::parse(&settings.options["endpoint"])
            .expect("a template")
            .parts(),
        [Part::Reference(Reference::Env(
            "RUSTIC_REPO_OPT_ENDPOINT".to_owned()
        ))]
    );
    assert_eq!(settings.options["foo-bar"], "${RUSTIC_REPO_OPT_FOO_BAR}");
    assert_eq!(
        settings.options_hot["region"],
        "${RUSTIC_REPO_OPTHOT_REGION}"
    );
    assert_eq!(settings.options_cold["tier"], "${RUSTIC_REPO_OPTCOLD_TIER}");
    assert_eq!(settings.options["own"], "the profile's");
    assert_eq!(settings.options.len(), 3, "{:?}", settings.options.keys());
    assert!(!format!("{profile:?}").contains("CANARY"));
}

#[tokio::test]
async fn the_profiles_repository_beats_rustics_variable() {
    let scratch = Scratch::new();
    let profile = scratch
        .read_with(
            &[("RUSTIC_REPOSITORY", "/srv/elsewhere")],
            &profile_text(""),
        )
        .await
        .expect("a profile");
    assert_eq!(profile.repository.settings.repository, "/srv/demo-repo");
}

#[tokio::test]
async fn the_profile_is_found_wherever_the_user_names_it() {
    let scratch = Scratch::new();
    let elsewhere = scratch.work.join("sub");
    std::fs::create_dir_all(&elsewhere).expect("a folder");
    let file = write_in(&elsewhere, "Laptop Profile.toml", &profile_text(""));
    let profile = scratch
        .reader()
        .read(&file, &nas())
        .await
        .expect("a profile");
    assert_eq!(ids(&profile), ["home"]);
}

#[tokio::test]
async fn only_so_many_snapshot_entries_are_taken() {
    let entries = |count: usize| -> String {
        (0..count)
            .map(|n| format!("[[backup.snapshots]]\nname = \"p{n}\"\nsources = [\"/srv/{n}\"]\n"))
            .collect()
    };
    let scratch = Scratch::new();
    let profile = scratch
        .read(&format!("{REPOSITORY}\n{}", entries(MAX_ENTRIES)))
        .await
        .expect("a profile");
    assert_eq!(profile.plans.len(), MAX_ENTRIES);

    let error = scratch
        .read(&format!("{REPOSITORY}\n{}", entries(MAX_ENTRIES + 1)))
        .await
        .unwrap_err();
    let (key, _, problem) = invalid(error);
    assert_eq!(key, "backup.snapshots");
    assert_eq!(problem, ProfileProblem::TooManyEntries);
}

/// A profile of `entries` snapshot entries, each with one source, after
/// `backup`, the text of its `[backup]` section.
fn crowd(entries: usize, backup: &str) -> String {
    let entries: String = (0..entries)
        .map(|n| format!("[[backup.snapshots]]\nname = \"p{n}\"\nsources = [\"/srv/{n}\"]\n"))
        .collect();
    format!("{REPOSITORY}\n{backup}\n{entries}")
}

/// `count` text items as a TOML array.
fn array(count: usize, item: &str) -> String {
    format!("[{}]", vec![format!("{item:?}"); count].join(", "))
}

#[tokio::test]
async fn the_items_the_plans_hold_in_all_are_bounded() {
    // Every plan holds the one source of its entry and the 99 globs of
    // `[backup]`: exactly the cap.
    let scratch = Scratch::new();
    let globs = |count| format!("[backup]\nglobs = {}\n", array(count, "*.a"));
    let profile = scratch
        .read(&crowd(MAX_ENTRIES, &globs(99)))
        .await
        .expect("a profile");
    assert_eq!(profile.plans.len(), MAX_ENTRIES);
    assert_eq!(MAX_ITEMS, MAX_ENTRIES * 100);

    let error = scratch
        .read(&crowd(MAX_ENTRIES, &globs(100)))
        .await
        .unwrap_err();
    let (key, _, problem) = invalid(error);
    assert_eq!(key, "backup");
    assert_eq!(problem, ProfileProblem::TooManyItems);
}

/// The lists a plan takes from `[backup]` or from an entry, as the table
/// each is in (`""` is the section or entry itself) and the text that sets
/// `count` items of it.
fn lists(count: usize) -> Vec<(&'static str, String)> {
    let many = array(count, "x");
    let commands = array(count, "true");
    let mut lists: Vec<(&'static str, String)> = [
        "tags",
        "globs",
        "iglobs",
        "glob-files",
        "iglob-files",
        "custom-ignorefiles",
        "exclude-if-present",
        "exclude-if-xattr",
    ]
    .into_iter()
    .map(|key| ("", format!("{key} = {many}\n")))
    .collect();
    for key in ["run-before", "run-after", "run-failed", "run-finally"] {
        lists.push((".hooks", format!("{key} = {commands}\n")));
    }
    lists
}

#[tokio::test]
async fn every_list_a_plan_gets_from_the_backup_section_counts() {
    let scratch = Scratch::new();
    for (table, list) in lists(100) {
        let text = crowd(MAX_ENTRIES, &format!("[backup{table}]\n{list}"));
        let (key, _, problem) = invalid(scratch.read(&text).await.unwrap_err());
        assert_eq!(problem, ProfileProblem::TooManyItems, "{list}");
        assert_eq!(key, "backup", "{list}");
    }
}

#[tokio::test]
async fn every_list_of_an_entry_counts() {
    let scratch = Scratch::new();
    let entry = "[[backup.snapshots]]\nname = \"home\"\nsources = [\"/srv/home\"]\n";
    for (table, list) in lists(MAX_ITEMS + 1) {
        let header = if table.is_empty() {
            String::new()
        } else {
            format!("\n[backup.snapshots{table}]\n")
        };
        let text = format!("{REPOSITORY}\n{entry}{header}{list}");
        let (key, _, problem) = invalid(scratch.read(&text).await.unwrap_err());
        assert_eq!(problem, ProfileProblem::TooManyItems, "{list:.30}");
        assert_eq!(key, "backup", "{list:.30}");
    }
    let sources: Vec<String> = (0..=MAX_ITEMS).map(|n| format!("\"/{n}\"")).collect();
    let text = format!(
        "{REPOSITORY}\n[[backup.snapshots]]\nname = \"home\"\nsources = [{}]\n",
        sources.join(",")
    );
    let (key, _, problem) = invalid(scratch.read(&text).await.unwrap_err());
    assert_eq!(problem, ProfileProblem::TooManyItems);
    assert_eq!(key, "backup");
}

#[tokio::test]
async fn only_so_many_different_keys_are_noted_as_left_out() {
    let keys = |count: usize| -> String {
        (0..count)
            .map(|n| format!("k{n:03} = 1\n"))
            .collect::<String>()
    };
    let text = |count: usize| {
        format!(
            "[repository]\nrepository = \"/srv/demo-repo\"\npassword-file = \"/srv/demo-password\"\n{}\n{ENTRY}",
            keys(count)
        )
    };
    let scratch = Scratch::new();
    let profile = scratch
        .read(&text(MAX_LEFT_OUT_KEYS))
        .await
        .expect("a profile");
    let noted = profile
        .notices
        .iter()
        .filter(|notice| matches!(notice, ProfileNotice::KeyIgnored { .. }))
        .count();
    assert_eq!(noted, MAX_LEFT_OUT_KEYS);

    let over = text(MAX_LEFT_OUT_KEYS + 1);
    let (key, line, problem) = invalid(scratch.read(&over).await.unwrap_err());
    assert_eq!(problem, ProfileProblem::TooManyKeys);
    assert_eq!(key, format!("repository.k{MAX_LEFT_OUT_KEYS:03}"));
    assert_eq!(
        line,
        Some(line_of(&over, &format!("k{MAX_LEFT_OUT_KEYS:03} =")))
    );
}

#[tokio::test]
async fn a_key_noted_in_many_entries_counts_once() {
    let entries: String = (0..300)
        .map(|n| {
            format!(
                "[[backup.snapshots]]\nname = \"p{n}\"\nsources = [\"/srv/{n}\"]\njson = true\n"
            )
        })
        .collect();
    let keys: String = (0..MAX_LEFT_OUT_KEYS - 1)
        .map(|n| format!("k{n:03} = 1\n"))
        .collect();
    let text = format!(
        "[repository]\nrepository = \"/srv/demo-repo\"\npassword-file = \"/srv/demo-password\"\n{keys}\n{entries}"
    );
    let profile = Scratch::new().read(&text).await.expect("a profile");
    assert_eq!(profile.plans.len(), 300);
    assert!(profile.notices.contains(&ignored("backup.snapshots.json")));
}

// --- A snapshot filter beside retention ---------------------------------------

/// A profile with `extra` after the repository, and the entry `home`.
fn with(extra: &str) -> String {
    format!("{REPOSITORY}\n{extra}\n{ENTRY}")
}

#[tokio::test]
async fn a_snapshot_filter_beside_retention_is_refused_at_the_first_key_of_the_filter() {
    // rustic's forget is narrower than the plan's: it reaches only the
    // snapshots the filter selects, which the retention would then be
    // applied to in Ladderback without it.
    let text = with(
        "[forget]\nkeep-last = 3\n\n[snapshot-filter]\nfilter-hosts = [\"demo\"]\nfilter-tags = [\"x\"]\n",
    );
    let (key, line) = unsupported(read_err(&text).await);
    assert_eq!(key, "snapshot-filter.filter-hosts");
    assert_eq!(line, Some(line_of(&text, "filter-hosts")));
}

#[tokio::test]
async fn any_kind_of_retention_with_a_snapshot_filter_is_refused() {
    for rule in [
        "keep-daily = 7",
        "keep-last = 0",
        "keep-within = \"1y\"",
        "keep-within-yearly = \"2y\"",
    ] {
        let text = with(&format!(
            "[forget]\n{rule}\n\n[snapshot-filter]\nfilter-tags = [\"x\"]\n"
        ));
        let (key, _) = unsupported(read_err(&text).await);
        assert_eq!(key, "snapshot-filter.filter-tags", "{rule}");
    }
}

#[tokio::test]
async fn a_snapshot_filter_without_retention_is_only_noted() {
    for forget in [
        "",
        "[forget]\nprune = true\n",
        "[forget]\nkeep-none = true\n",
        "[forget]\ngroup-by = \"host\"\n",
    ] {
        let text = with(&format!(
            "{forget}\n[snapshot-filter]\nfilter-tags = [\"x\"]\n"
        ));
        let profile = read_ok(&text).await;
        assert!(
            profile.notices.contains(&ignored("snapshot-filter")),
            "{forget}"
        );
    }
}

#[tokio::test]
async fn an_empty_snapshot_filter_beside_retention_is_not_a_filter() {
    let profile = read_ok(&with("[forget]\nkeep-last = 3\n\n[snapshot-filter]\n")).await;
    assert_eq!(ids(&profile), ["home"]);
}

#[tokio::test]
async fn the_filter_an_export_makes_is_not_a_narrowing() {
    // Selecting the snapshots of one plan of one agent is what the plan's
    // forget does anyway, so an exported profile is read back.
    let own = "[forget]\nkeep-last = 3\n\n[snapshot-filter]\nfilter-tags = [\"ladderback-plan:home,ladderback-agent:00000000-0000-0000-0000-000000000001\"]\n";
    let profile = read_ok(&with(own)).await;
    assert_eq!(
        plan(&profile, "home").retention.keep_last.map(|n| n.get()),
        Some(3)
    );
    assert!(profile.notices.contains(&ignored("snapshot-filter")));

    for filter in [
        // Two entries are OR-ed, not both required.
        "filter-tags = [\"ladderback-plan:home,ladderback-agent:a\", \"x\"]",
        // Another tag makes it narrower.
        "filter-tags = [\"ladderback-plan:home,ladderback-agent:a,x\"]",
        "filter-tags = [\"ladderback-plan:home\"]",
        "filter-tags = [\"ladderback-agent:a,ladderback-plan:home\"]",
        "filter-tags = [\"ladderback-plan:home,x\"]",
        "filter-tags = [\"ladderback-plan:home,ladderback-agent:a\"]\nfilter-hosts = [\"demo\"]",
        "filter-tags = []",
        "filter-tags = [1]",
    ] {
        let text = with(&format!(
            "[forget]\nkeep-last = 3\n\n[snapshot-filter]\n{filter}\n"
        ));
        // A narrower filter is refused; so is a filter-tags that is not
        // a list of texts (a wrong type).
        let error = read_err(&text).await;
        assert!(
            matches!(
                error,
                ProfileError::Unsupported { .. } | ProfileError::Invalid { .. }
            ),
            "{filter}: {error:?}"
        );
    }
}

// --- Substituting variables in the profiles a profile uses ----------------------

#[tokio::test]
async fn a_profile_that_has_rustic_substitute_variables_in_what_it_uses_is_refused() {
    // The importer escapes the `${` a used profile holds so that it stays
    // text; with the switch on, rustic would have read it as a variable.
    let text = with("[global]\nprofile-substitute-env = true\n");
    let (key, line) = unsupported(read_err(&text).await);
    assert_eq!(key, "global.profile-substitute-env");
    assert_eq!(line, Some(line_of(&text, "profile-substitute-env")));
}

#[tokio::test]
async fn the_switch_turned_off_is_only_noted() {
    let profile = read_ok(&with("[global]\nprofile-substitute-env = false\n")).await;
    assert!(
        profile
            .notices
            .contains(&ignored("global.profile-substitute-env"))
    );
}

#[tokio::test]
async fn a_switch_that_is_not_true_or_false_is_a_wrong_type() {
    let (key, _, problem) =
        invalid(read_err(&with("[global]\nprofile-substitute-env = \"yes\"\n")).await);
    assert_eq!(key, "global.profile-substitute-env");
    assert_eq!(problem, ProfileProblem::WrongType);
}

// --- Text of a profile in a notice ------------------------------------------------

#[tokio::test]
async fn a_dropped_tag_is_noted_without_control_characters_and_cut_short() {
    // The notice goes to a terminal: an escape sequence in a tag must not.
    let long = format!("ladderback-plan:{}", "t".repeat(200));
    let profile = read_ok(&with(&format!(
        "[backup]\ntags = [\"ladderback-plan:a\\u001b[2Jb\", \"ladderback-agent:x\\ny\", \"{long}\"]\n"
    )))
    .await;
    let dropped: Vec<&str> = profile
        .notices
        .iter()
        .filter_map(|notice| match notice {
            ProfileNotice::TagDropped { tag } => Some(tag.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(dropped.len(), 3, "{dropped:?}");
    assert!(dropped.contains(&"ladderback-plan:a?[2Jb"), "{dropped:?}");
    assert!(dropped.contains(&"ladderback-agent:x?y"), "{dropped:?}");
    assert!(
        dropped
            .iter()
            .any(|tag| tag.chars().count() == 80 && tag.starts_with("ladderback-plan:t")),
        "{dropped:?}"
    );
    assert!(dropped.iter().all(|tag| !tag.chars().any(char::is_control)));
}
