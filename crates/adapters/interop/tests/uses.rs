//! `use-profiles`: the profiles a profile uses are found the way rustic finds
//! them and merged the way rustic merges them, within bounds a profile
//! nobody vetted can't push past (technical design §6.2).

mod common;

use std::fs;

use app::ports::{ProfileError, ProfileNotice, ProfileProblem, ProfileReader};
use common::{ENTRY, REPOSITORY, Scratch, commands, ids, ignored, line_of, nas, plan, write_in};
use domain::planning::{GroupBy, KeepCount};
use domain::secrets::{FileRead, SecretRef};
use interop::{MAX_PROFILES, MAX_USE_DEPTH};

/// `nas.toml`: uses `names`, then `text`.
fn uses(names: &[&str], text: &str) -> String {
    let list: Vec<_> = names.iter().map(|name| format!("{name:?}")).collect();
    format!("[global]\nuse-profiles = [{}]\n\n{text}", list.join(", "))
}

/// A refusal for an invalid profile: the file, the key, the line and the
/// problem.
fn invalid(error: ProfileError) -> (String, String, Option<usize>, ProfileProblem) {
    match error {
        ProfileError::Invalid {
            file,
            key,
            at,
            problem,
            ..
        } => (file, key.unwrap_or_default(), at.map(|at| at.line), problem),
        other => panic!("the profile should be invalid, got {other:?}"),
    }
}

#[tokio::test]
async fn a_profiles_own_value_beats_the_profile_it_uses() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "[repository]\nrepository = \"/srv/used\"\npassword-file = \"/srv/demo-password\"\n",
    );
    let profile = scratch
        .read(&uses(
            &["base"],
            &format!("[repository]\nrepository = \"/srv/own\"\n\n{ENTRY}"),
        ))
        .await
        .expect("a profile");
    let settings = &profile.repository.settings;
    assert_eq!(settings.repository, "/srv/own");
    assert_eq!(
        settings.password,
        SecretRef::File {
            path: "/srv/demo-password".to_owned(),
            read: FileRead::FirstLine,
        },
        "what the own profile leaves out comes from the used one"
    );
}

#[tokio::test]
async fn a_setting_the_profile_leaves_out_comes_from_the_used_profile_and_reaches_the_plans() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "\
[global]
group-by = \"tags\"

[backup]
host = \"demo-laptop\"
label = \"laptop\"

[forget]
keep-daily = 7
keep-last = 9
",
    );
    let profile = scratch
        .read(&uses(
            &["base"],
            &format!("{REPOSITORY}\n[forget]\nkeep-last = 3\n\n{ENTRY}"),
        ))
        .await
        .expect("a profile");
    assert_eq!(
        profile.repository.group_by,
        GroupBy::parse("tags").expect("a grouping")
    );
    let home = plan(&profile, "home");
    assert_eq!(
        home.host.as_ref().map(|host| host.as_str()),
        Some("demo-laptop")
    );
    assert_eq!(home.label.as_deref(), Some("laptop"));
    assert_eq!(home.retention.keep_last.map(KeepCount::get), Some(3));
    assert_eq!(home.retention.keep_daily.map(KeepCount::get), Some(7));
}

#[tokio::test]
async fn a_list_the_profile_sets_replaces_the_used_one_and_an_empty_list_takes_it() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "\
[backup]
exclude-if-present = [\".from-base\"]
globs = [\"!*.base\"]
tags = [\"base\"]
",
    );
    let profile = scratch
        .read(&uses(
            &["base"],
            &format!(
                "{REPOSITORY}\n[backup]\nexclude-if-present = [\".own\"]\nglobs = []\n\n{ENTRY}"
            ),
        ))
        .await
        .expect("a profile");
    let home = plan(&profile, "home");
    assert_eq!(home.filter.exclude_if_present, [".own"]);
    assert_eq!(home.filter.globs, ["!*.base"]);
    let tags: Vec<_> = home.tags.iter().map(|tag| tag.as_str()).collect();
    assert_eq!(tags, ["base"]);
}

#[tokio::test]
async fn a_switch_either_profile_turns_on_is_on() {
    let scratch = Scratch::new();
    scratch.profile("base.toml", "[backup]\ngit-ignore = true\n");
    let profile = scratch
        .read(&uses(
            &["base"],
            &format!(
                "{REPOSITORY}\n[backup]\ngit-ignore = false\none-file-system = true\n\n{ENTRY}"
            ),
        ))
        .await
        .expect("a profile");
    let home = plan(&profile, "home");
    assert!(home.filter.git_ignore);
    assert!(home.filter.one_file_system);
}

#[tokio::test]
async fn hooks_are_appended_the_profiles_own_first() {
    let scratch = Scratch::new();
    scratch.profile(
        "one.toml",
        "\
[global.hooks]
run-before = [\"echo agent-one\"]
[repository.hooks]
run-before = [\"echo repository-one\"]
[backup.hooks]
run-before = [\"echo backup-one\"]
run-after = [\"echo backup-after-one\"]
",
    );
    scratch.profile(
        "two.toml",
        "[global.hooks]\nrun-before = [\"echo agent-two\"]\n",
    );
    let profile = scratch
        .read(&uses(
            &["one", "two"],
            &format!(
                "\
[global.hooks]
run-before = [\"echo agent-own\"]
[repository.hooks]
run-before = [\"echo repository-own\"]
[backup.hooks]
run-before = [\"echo backup-own\"]

{REPOSITORY}
{ENTRY}"
            ),
        ))
        .await
        .expect("a profile");
    assert_eq!(
        commands(&profile.hooks.run_before),
        ["echo agent-own", "echo agent-one", "echo agent-two"]
    );
    assert_eq!(
        commands(&profile.repository.hooks.run_before),
        ["echo repository-own", "echo repository-one"]
    );
    let hooks = &plan(&profile, "home").hooks;
    assert_eq!(
        commands(&hooks.run_before),
        ["echo backup-own", "echo backup-one"]
    );
    assert_eq!(commands(&hooks.run_after), ["echo backup-after-one"]);
}

#[tokio::test]
async fn options_are_merged_key_by_key_and_the_profiles_own_key_wins() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "\
[repository.options]
shared = \"used\"
only-used = \"used\"
[repository.options-hot]
region = \"used\"
",
    );
    let profile = scratch
        .read(&uses(
            &["base"],
            &format!("{REPOSITORY}\n[repository.options]\nshared = \"own\"\nonly-own = \"own\"\n\n{ENTRY}"),
        ))
        .await
        .expect("a profile");
    let options = &profile.repository.settings.options;
    assert_eq!(options["shared"], "own");
    assert_eq!(options["only-used"], "used");
    assert_eq!(options["only-own"], "own");
    assert_eq!(profile.repository.settings.options_hot["region"], "used");
}

#[tokio::test]
async fn snapshot_entries_are_concatenated_sorted_and_the_profiles_own_wins_a_repeat() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "\
[[backup.snapshots]]
name = \"docs\"
sources = [\"/home/demo/docs\"]
label = \"from the used profile\"

[[backup.snapshots]]
name = \"photos\"
sources = [\"/home/demo/photos\"]
",
    );
    let profile = scratch
        .read(&uses(
            &["base"],
            &format!(
                "{REPOSITORY}\n[[backup.snapshots]]\nname = \"home\"\nsources = [\"/home/demo\"]\n\n[[backup.snapshots]]\nname = \"docs\"\nsources = [\"/home/demo/docs\"]\nlabel = \"own\"\n"
            ),
        ))
        .await
        .expect("a profile");
    assert_eq!(ids(&profile), ["docs", "home", "photos"]);
    assert_eq!(plan(&profile, "docs").label.as_deref(), Some("own"));
}

#[tokio::test]
async fn of_several_used_profiles_the_one_listed_first_fills_a_gap_first() {
    let scratch = Scratch::new();
    scratch.profile("one.toml", "[backup]\nlabel = \"one\"\n");
    scratch.profile(
        "two.toml",
        "[backup]\nlabel = \"two\"\nhost = \"demo-two\"\n",
    );
    let profile = scratch
        .read(&uses(&["one", "two"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .expect("a profile");
    let home = plan(&profile, "home");
    assert_eq!(home.label.as_deref(), Some("one"));
    assert_eq!(
        home.host.as_ref().map(|host| host.as_str()),
        Some("demo-two")
    );
}

#[tokio::test]
async fn a_used_profile_may_use_another_and_the_nearer_one_wins() {
    let scratch = Scratch::new();
    scratch.profile(
        "middle.toml",
        &uses(&["root"], "[backup]\nlabel = \"middle\"\n"),
    );
    scratch.profile(
        "root.toml",
        "[backup]\nlabel = \"root\"\nhost = \"demo-root\"\n",
    );
    let profile = scratch
        .read(&uses(&["middle"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .expect("a profile");
    let home = plan(&profile, "home");
    assert_eq!(home.label.as_deref(), Some("middle"));
    assert_eq!(
        home.host.as_ref().map(|host| host.as_str()),
        Some("demo-root")
    );
}

#[tokio::test]
async fn a_used_profile_is_found_in_rustics_folder_then_etc_then_the_working_folder() {
    let scratch = Scratch::new();
    for (folder, mark) in [
        (&scratch.work, ".from-the-working-folder"),
        (&scratch.etc, ".from-etc"),
        (&scratch.rustic, ".from-rustics-folder"),
    ] {
        write_in(
            folder,
            "base.toml",
            &format!("[backup]\nexclude-if-present = [\"{mark}\"]\n"),
        );
    }
    let text = uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}"));
    let marks = |profile: &app::ports::ImportedProfile| {
        plan(profile, "home").filter.exclude_if_present.clone()
    };

    let profile = scratch.read(&text).await.expect("a profile");
    assert_eq!(marks(&profile), [".from-rustics-folder"]);
    fs::remove_file(scratch.rustic.join("base.toml")).expect("a file");
    let profile = scratch.read(&text).await.expect("a profile");
    assert_eq!(marks(&profile), [".from-etc"]);
    fs::remove_file(scratch.etc.join("base.toml")).expect("a file");
    let profile = scratch.read(&text).await.expect("a profile");
    assert_eq!(marks(&profile), [".from-the-working-folder"]);
}

#[tokio::test]
async fn a_used_profiles_name_may_end_in_toml() {
    let scratch = Scratch::new();
    scratch.profile("base.toml", "[backup]\nlabel = \"base\"\n");
    let profile = scratch
        .read(&uses(&["base.toml"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .expect("a profile");
    assert_eq!(plan(&profile, "home").label.as_deref(), Some("base"));
}

#[tokio::test]
async fn a_used_profile_that_is_not_found_is_noted_and_the_rest_is_read() {
    let scratch = Scratch::new();
    let profile = scratch
        .read(&uses(&["ghost"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .expect("a profile");
    assert_eq!(
        profile.notices,
        [ProfileNotice::UsedProfileMissing {
            name: "ghost".to_owned()
        }]
    );
    assert_eq!(ids(&profile), ["home"]);
}

#[tokio::test]
async fn a_used_profile_is_a_file_name_never_a_path() {
    let scratch = Scratch::new();
    // What a path would reach, if it were followed.
    write_in(
        scratch.work.parent().expect("a parent"),
        "outside.toml",
        "[backup]\nlabel = \"outside\"\n",
    );
    for name in [
        "../outside",
        "sub/base",
        "sub\\base",
        "/etc/passwd",
        "..",
        ".",
        "",
        "a\\u0000b",
        "a\\nb",
    ] {
        let text = format!(
            "[global]\nuse-profiles = [\"ok-not-found\", \"{name}\"]\n\n{REPOSITORY}\n{ENTRY}"
        );
        let error = scratch.read(&text).await.unwrap_err();
        let (_, key, line, problem) = invalid(error);
        assert_eq!(key, "global.use-profiles[1]", "{name:?}");
        assert_eq!(line, Some(line_of(&text, "use-profiles")), "{name:?}");
        assert_eq!(problem, ProfileProblem::UsedName, "{name:?}");
    }
}

#[tokio::test]
async fn a_name_too_long_for_a_file_name_is_refused() {
    let scratch = Scratch::new();
    let name = "a".repeat(300);
    let error = scratch
        .read(&uses(&[&name], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .unwrap_err();
    assert_eq!(invalid(error).3, ProfileProblem::UsedName);
}

#[tokio::test]
async fn profiles_that_use_each_other_or_themselves_are_refused() {
    let cases: [(&[(&str, &str)], &str); 3] = [
        // nas.toml is the profile being read: it uses itself, through `one`.
        (&[("one.toml", "nas")], "one"),
        (&[("one.toml", "two"), ("two.toml", "one")], "one"),
        (
            &[
                ("one.toml", "two"),
                ("two.toml", "three"),
                ("three.toml", "one"),
            ],
            "one",
        ),
    ];
    for (files, first) in cases {
        let scratch = Scratch::new();
        for (name, uses_name) in files {
            scratch.profile(name, &uses(&[uses_name], ""));
        }
        let text = uses(&[first], &format!("{REPOSITORY}\n{ENTRY}"));
        let (_, key, _, problem) = invalid(scratch.read(&text).await.unwrap_err());
        assert_eq!(key, "global.use-profiles[0]", "{files:?}");
        assert_eq!(problem, ProfileProblem::UsedCycle, "{files:?}");
    }
}

#[tokio::test]
async fn a_profile_that_uses_its_own_file_is_a_cycle() {
    let scratch = Scratch::new();
    let file = scratch.profile(
        "nas.toml",
        &uses(&["nas"], &format!("{REPOSITORY}\n{ENTRY}")),
    );
    let error = scratch.reader().read(&file, &nas()).await.unwrap_err();
    let (_, key, _, problem) = invalid(error);
    assert_eq!(key, "global.use-profiles[0]");
    assert_eq!(problem, ProfileProblem::UsedCycle);
}

/// Writes `count` profiles that each use the next, `p1` the first.
fn chain(scratch: &Scratch, count: usize) {
    for number in 1..=count {
        let text = if number == count {
            "[backup]\nlabel = \"last\"\n".to_owned()
        } else {
            uses(&[&format!("p{}", number + 1)], "")
        };
        scratch.profile(&format!("p{number}.toml"), &text);
    }
}

#[tokio::test]
async fn profiles_are_followed_so_deep_and_no_deeper() {
    let scratch = Scratch::new();
    chain(&scratch, MAX_USE_DEPTH);
    let text = uses(&["p1"], &format!("{REPOSITORY}\n{ENTRY}"));
    let profile = scratch.read(&text).await.expect("a profile");
    assert_eq!(plan(&profile, "home").label.as_deref(), Some("last"));

    let scratch = Scratch::new();
    chain(&scratch, MAX_USE_DEPTH + 1);
    let (_, key, _, problem) = invalid(scratch.read(&text).await.unwrap_err());
    assert_eq!(problem, ProfileProblem::UsedTooDeep);
    assert!(key.starts_with("global.use-profiles"), "{key}");
}

#[tokio::test]
async fn only_so_many_profiles_are_read_in_all() {
    let scratch = Scratch::new();
    for number in 0..MAX_PROFILES {
        scratch.profile(&format!("p{number}.toml"), "");
    }
    let names = |count: usize| -> Vec<String> { (0..count).map(|n| format!("p{n}")).collect() };
    let text = |count: usize| {
        let names = names(count);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        uses(&names, &format!("{REPOSITORY}\n{ENTRY}"))
    };

    // The profile being read counts as one.
    scratch
        .read(&text(MAX_PROFILES - 1))
        .await
        .expect("a profile");
    let (_, key, _, problem) = invalid(scratch.read(&text(MAX_PROFILES)).await.unwrap_err());
    assert_eq!(problem, ProfileProblem::UsedTooMany);
    assert!(key.starts_with("global.use-profiles"), "{key}");
}

#[tokio::test]
async fn a_profile_used_twice_is_read_twice_as_rustic_merges_it_twice() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "[global.hooks]\nrun-before = [\"echo base\"]\n",
    );
    scratch.profile("left.toml", &uses(&["base"], ""));
    scratch.profile("right.toml", &uses(&["base"], ""));
    let profile = scratch
        .read(&uses(&["left", "right"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .expect("a profile");
    assert_eq!(
        commands(&profile.hooks.run_before),
        ["echo base", "echo base"]
    );
}

#[tokio::test]
async fn a_refusal_in_a_used_profile_names_that_profiles_file() {
    let scratch = Scratch::new();
    let base = scratch.profile("base.toml", "[forget]\nkeep-lots = 7\n");
    let error = scratch
        .read(&uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .unwrap_err();
    let (file, key, line, problem) = invalid(error);
    assert_eq!(file, base.display().to_string());
    assert_eq!(key, "forget.keep-lots");
    assert_eq!(line, Some(2));
    assert_eq!(problem, ProfileProblem::UnknownKey);

    let base = scratch.profile("base.toml", "[forget]\nkeep-tags = [\"x\"]\n");
    let error = scratch
        .read(&uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .unwrap_err();
    match error {
        ProfileError::Unsupported { file, key, .. } => {
            assert_eq!(file, base.display().to_string());
            assert_eq!(key, "forget.keep-tags");
        }
        other => panic!("unsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn a_used_profile_over_the_cap_or_not_a_file_is_refused_naming_it() {
    let scratch = Scratch::new();
    let text = uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}"));

    let base = scratch.profile("base.toml", &format!("# {}\n", "x".repeat(1024 * 1024)));
    let error = scratch.read(&text).await.unwrap_err();
    assert_eq!(
        error,
        ProfileError::TooLarge {
            file: base.display().to_string(),
            limit: 1024 * 1024,
        }
    );

    fs::remove_file(&base).expect("a file");
    let folder = scratch.rustic.join("base.toml");
    fs::create_dir(&folder).expect("a folder");
    let error = scratch.read(&text).await.unwrap_err();
    assert_eq!(
        error,
        ProfileError::Unreadable {
            file: folder.display().to_string()
        }
    );
}

#[tokio::test]
async fn what_a_used_profile_leaves_out_is_noted_and_its_text_stays_text() {
    let scratch = Scratch::new();
    scratch.profile(
        "base.toml",
        "[global]\ndry-run = true\n\n[repository]\nrepository = \"${HOME}/repo\"\n",
    );
    let profile = scratch
        .read(&uses(
            &["base"],
            "[repository]\npassword-file = \"/srv/demo-password\"\n",
        ))
        .await
        .expect("a profile");
    assert!(profile.notices.contains(&ignored("global.dry-run")));
    assert_eq!(profile.repository.settings.repository, "$${HOME}/repo");
}

#[tokio::test]
async fn a_list_of_more_profiles_than_are_read_is_refused_whether_they_exist_or_not() {
    let scratch = Scratch::new();
    let names = |count: usize| -> Vec<String> { (0..count).map(|n| format!("none{n}")).collect() };
    let text = |count: usize| {
        let names = names(count);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        uses(&names, &format!("{REPOSITORY}\n{ENTRY}"))
    };

    let profile = scratch.read(&text(MAX_PROFILES)).await.expect("a profile");
    assert_eq!(profile.notices.len(), MAX_PROFILES);

    let over = text(MAX_PROFILES + 1);
    let (_, key, line, problem) = invalid(scratch.read(&over).await.unwrap_err());
    assert_eq!(problem, ProfileProblem::UsedTooMany);
    assert_eq!(key, "global.use-profiles");
    assert_eq!(line, Some(line_of(&over, "use-profiles")));
}

#[tokio::test]
async fn a_snapshot_filter_and_retention_that_come_from_two_profiles_are_refused_too() {
    let scratch = Scratch::new();
    let text = uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}"));

    // The filter is the used profile's: the refusal is at its line.
    let base = scratch.profile(
        "base.toml",
        "[snapshot-filter]\nfilter-hosts = [\"demo\"]\n",
    );
    let own = format!("{text}\n[forget]\nkeep-last = 3\n");
    match scratch.read(&own).await.unwrap_err() {
        ProfileError::Unsupported { file, key, at } => {
            assert_eq!(file, base.display().to_string());
            assert_eq!(key, "snapshot-filter.filter-hosts");
            assert_eq!(at.map(|at| at.line), Some(2));
        }
        other => panic!("unsupported, got {other:?}"),
    }

    // The retention is the used profile's, the filter the profile's own.
    scratch.profile("base.toml", "[forget]\nkeep-last = 3\n");
    let own = format!("{text}\n[snapshot-filter]\nfilter-tags = [\"x\"]\n");
    let file = scratch.work.join("nas.toml").display().to_string();
    match scratch.read(&own).await.unwrap_err() {
        ProfileError::Unsupported {
            file: found, key, ..
        } => {
            assert_eq!(found, file);
            assert_eq!(key, "snapshot-filter.filter-tags");
        }
        other => panic!("unsupported, got {other:?}"),
    }

    // Both profiles have a filter: the profile's own is the one named.
    scratch.profile(
        "base.toml",
        "[forget]\nkeep-last = 3\n\n[snapshot-filter]\nfilter-hosts = [\"demo\"]\n",
    );
    match scratch.read(&own).await.unwrap_err() {
        ProfileError::Unsupported {
            file: found, key, ..
        } => {
            assert_eq!(found, file);
            assert_eq!(key, "snapshot-filter.filter-tags");
        }
        other => panic!("unsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn the_switch_in_a_used_profile_is_refused_naming_that_profile() {
    let scratch = Scratch::new();
    let base = scratch.profile("base.toml", "[global]\nprofile-substitute-env = true\n");
    match scratch
        .read(&uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .unwrap_err()
    {
        ProfileError::Unsupported { file, key, at } => {
            assert_eq!(file, base.display().to_string());
            assert_eq!(key, "global.profile-substitute-env");
            assert_eq!(at.map(|at| at.line), Some(2));
        }
        other => panic!("unsupported, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_used_profile_that_is_a_pipe_is_unreadable_and_is_not_opened() {
    let scratch = Scratch::new();
    let pipe = scratch.rustic.join("base.toml");
    if common::fifo(&pipe).is_none() {
        testkit::skip!("mkfifo is not available");
    }
    let error = scratch
        .read(&uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProfileError::Unreadable {
            file: pipe.display().to_string()
        }
    );
}

#[tokio::test]
async fn a_used_profile_that_is_a_folder_is_unreadable() {
    let scratch = Scratch::new();
    let folder = scratch.work.join("base.toml");
    fs::create_dir(&folder).expect("a folder");
    let error = scratch
        .read(&uses(&["base"], &format!("{REPOSITORY}\n{ENTRY}")))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProfileError::Unreadable {
            file: folder.display().to_string()
        }
    );
}

#[tokio::test]
async fn the_environment_and_prune_of_either_profile_count() {
    // The environment (a notice) and prune (a warning and local
    // maintenance), from the used profile, the own one, or both.
    let scratch = Scratch::new();
    let env = "[global.env]\nDEMO = \"x\"\n";
    let prune = "[forget]\nprune = true\n";
    for (own, used) in [(env, ""), ("", env), (env, env)] {
        scratch.profile("base.toml", used);
        let profile = scratch
            .read(&uses(&["base"], &format!("{own}\n{REPOSITORY}\n{ENTRY}")))
            .await
            .expect("a profile");
        assert!(
            profile.notices.contains(&ProfileNotice::EnvIgnored),
            "{own:?} {used:?}"
        );
    }
    for (own, used) in [(prune, ""), ("", prune), (prune, prune)] {
        scratch.profile("base.toml", used);
        let profile = scratch
            .read(&uses(&["base"], &format!("{own}\n{REPOSITORY}\n{ENTRY}")))
            .await
            .expect("a profile");
        assert!(
            profile.notices.contains(&ProfileNotice::Prune),
            "{own:?} {used:?}"
        );
    }
}
