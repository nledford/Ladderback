//! `FileConfigStore` on a real folder: what loading a file does and refuses,
//! and what it leaves alone (technical design §6.1).

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use app::ports::{ConfigError, ConfigStore, Problem};
use config_adapter::{MAX_CONFIG_BYTES, MigrationError, Schemas, parse, render};
use domain::planning::{AgentConfig, AgentSettings, Concurrency, Hooks};
use support::model::expected;
use support::*;

/// Version 1 to 2: `[agent] name` became `hostname`.
fn rename_name(table: &mut toml::Table) -> Result<(), MigrationError> {
    if let Some(agent) = table.get_mut("agent").and_then(toml::Value::as_table_mut)
        && let Some(name) = agent.remove("name")
    {
        agent.insert("hostname".to_owned(), name);
    }
    Ok(())
}

fn version_2() -> Schemas {
    Schemas {
        current: 2,
        steps: vec![rename_name],
    }
}

const OLD_FILE: &str = "schema = 1\n# written by hand\n[agent]\nname = \"demo-host\"\n";

fn with_concurrency(n: i64) -> AgentConfig {
    AgentConfig::new(
        AgentSettings {
            concurrency: Concurrency::new(n).expect("a concurrency"),
            ..AgentSettings::default()
        },
        Hooks::default(),
        BTreeMap::new(),
        Vec::new(),
    )
    .expect("a valid configuration")
}

#[tokio::test]
async fn a_folder_without_a_config_file_loads_the_empty_configuration() {
    let scratch = Scratch::new();
    assert_eq!(scratch.store().load().await, Ok(AgentConfig::default()));
    assert!(
        scratch.entries().is_empty(),
        "loading doesn't create anything: {:?}",
        scratch.entries()
    );
}

#[tokio::test]
async fn a_current_file_is_loaded_and_left_exactly_as_it_is() {
    let scratch = Scratch::new();
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/v1.toml"),
    )
    .expect("the golden file");
    scratch.write(&text);
    let before = std::fs::metadata(&scratch.path)
        .expect("metadata")
        .modified()
        .expect("a time");
    assert_eq!(scratch.store().load().await, Ok(expected()));
    assert_eq!(
        scratch.read(),
        text,
        "hand-written comments and layout stay"
    );
    let after = std::fs::metadata(&scratch.path)
        .expect("metadata")
        .modified()
        .expect("a time");
    assert_eq!(before, after, "the file wasn't written");
    assert_eq!(scratch.entries(), ["config.toml"], "and nothing was kept");
}

#[tokio::test]
async fn a_file_over_the_cap_is_refused_unread_and_one_at_the_cap_is_read() {
    let scratch = Scratch::new();
    let mut text = String::from("schema = 1\n#");
    let padding = usize::try_from(MAX_CONFIG_BYTES).expect("fits") - text.len() - 1;
    text.push_str(&"x".repeat(padding));
    text.push('\n');
    assert_eq!(u64::try_from(text.len()).expect("fits"), MAX_CONFIG_BYTES);
    scratch.write(&text);
    assert_eq!(scratch.store().load().await, Ok(AgentConfig::default()));

    text.push('x');
    scratch.write(&text);
    assert_eq!(
        scratch.store().load().await,
        Err(ConfigError::TooLarge {
            file: scratch.name(),
            limit: MAX_CONFIG_BYTES
        })
    );
}

#[tokio::test]
async fn a_file_that_never_ends_is_refused_at_the_cap() {
    let scratch = Scratch::new();
    std::os::unix::fs::symlink("/dev/zero", &scratch.path).expect("a symlink");
    let store = scratch.store();
    let loaded = tokio::time::timeout(Duration::from_secs(30), store.load())
        .await
        .expect("the load stops at the cap");
    assert_eq!(
        loaded,
        Err(ConfigError::TooLarge {
            file: scratch.name(),
            limit: MAX_CONFIG_BYTES
        })
    );
}

#[tokio::test]
async fn a_file_that_is_not_text_is_refused_as_invalid() {
    let scratch = Scratch::new();
    scratch.write_bytes(b"schema = 1\n# \xff\xfe\n");
    let error = invalid(scratch.store().load().await);
    assert_eq!(error.problem, Problem::NotUtf8);
    assert_eq!(error.file, scratch.name());
    assert_eq!(error.key, None);
    assert_eq!(error.at, None);
}

#[tokio::test]
async fn a_config_path_that_cannot_be_read_is_unavailable() {
    let scratch = Scratch::new();
    std::fs::create_dir(&scratch.path).expect("a folder in the file's place");
    let loaded = scratch.store().load().await;
    assert!(
        matches!(&loaded, Err(ConfigError::Unavailable { file, detail })
            if *file == scratch.name() && !detail.is_empty()),
        "{loaded:?}"
    );
}

#[tokio::test]
async fn a_config_path_below_a_file_is_unavailable_not_an_empty_configuration() {
    let scratch = Scratch::new();
    let blocker = scratch.dir.path().join("not-a-folder");
    std::fs::write(&blocker, "a file where a folder should be").expect("a file");
    let path = blocker.join("config.toml");
    let store = config_adapter::FileConfigStore::new(path.clone(), scratch.clock.clone());
    let loaded = store.load().await;
    assert!(
        matches!(&loaded, Err(ConfigError::Unavailable { file, .. })
            if *file == path.display().to_string()),
        "{loaded:?}"
    );
}

#[tokio::test]
async fn a_file_that_is_not_a_config_is_refused_and_left_alone() {
    let scratch = Scratch::new();
    let cases: [(&str, ConfigError); 3] = [
        (
            "[agent]\nconcurrency = 2\n",
            ConfigError::SchemaMissing {
                file: scratch.name(),
            },
        ),
        (
            "schema = 2\n[agent]\nbogus = 1\n",
            ConfigError::SchemaNewer {
                file: scratch.name(),
                found: 2,
                supported: 1,
            },
        ),
        (
            "schema = 1\n[agent]\nconcurrency = 0\n",
            ConfigError::Invalid {
                file: scratch.name(),
                key: Some("agent.concurrency".to_owned()),
                at: Some(position_of("schema = 1\n[agent]\nconcurrency = 0\n", "0\n")),
                problem: Problem::Value(domain::planning::ValueKind::Concurrency),
                detail: String::new(),
            },
        ),
    ];
    for (text, error) in cases {
        scratch.write(text);
        assert_eq!(scratch.store().load().await, Err(error), "{text:?}");
        assert_eq!(scratch.read(), text, "the file is as it was");
        assert_eq!(
            scratch.entries(),
            ["config.toml"],
            "nothing was kept: {text:?}"
        );
    }
}

#[tokio::test]
async fn an_older_file_is_kept_whole_then_replaced_by_the_current_version() {
    let scratch = Scratch::new();
    scratch.write(OLD_FILE);
    let store = scratch.store_with(version_2());
    let loaded = store.load().await.expect("the old file loads");
    assert_eq!(
        loaded
            .agent()
            .hostname
            .as_ref()
            .map(domain::planning::Hostname::as_str),
        Some("demo-host")
    );

    let kept = scratch.history();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(
        kept[0].1,
        OLD_FILE.as_bytes(),
        "the old file, byte for byte"
    );
    assert_eq!(kept[0].0, "20261108T132021.123Z-00.toml");
    assert_eq!(mode_of(&scratch.history_dir()), 0o700);
    assert_eq!(mode_of(&scratch.history_dir().join(&kept[0].0)), 0o600);

    let now = scratch.read();
    assert!(now.starts_with("schema = 2\n"), "{now}");
    // `render` writes the version this build has (1); this store's chain makes
    // the current one 2, so the files differ in the schema line and no more.
    let rendered = render(std::path::Path::new(&scratch.name()), &loaded).expect("written");
    assert_eq!(
        now.strip_prefix("schema = 2\n"),
        rendered.strip_prefix("schema = 1\n"),
        "the file is the configuration, in the current version"
    );

    // Now current: loaded again, it is not rewritten and nothing more is kept.
    scratch.clock.advance(domain::SignedDuration::from_secs(5));
    assert_eq!(store.load().await, Ok(loaded));
    assert_eq!(scratch.read(), now);
    assert_eq!(scratch.history().len(), 1);
}

#[tokio::test]
async fn an_older_file_that_turns_out_invalid_is_neither_kept_nor_replaced() {
    let scratch = Scratch::new();
    let text = "schema = 1\n[agent]\nname = \"demo-host\"\nbogus = 1\n";
    scratch.write(text);
    let error = invalid(scratch.store_with(version_2()).load().await);
    assert_eq!(error.problem, Problem::UnknownKey);
    assert_eq!(scratch.read(), text);
    assert_eq!(scratch.entries(), ["config.toml"]);
}

#[tokio::test]
async fn an_older_file_is_not_replaced_when_its_old_version_cannot_be_kept() {
    let scratch = Scratch::new();
    scratch.write(OLD_FILE);
    // A file where the history folder goes.
    std::fs::write(scratch.history_dir(), "not a folder").expect("a file");
    let loaded = scratch.store_with(version_2()).load().await;
    assert!(
        matches!(&loaded, Err(ConfigError::Unavailable { file, .. }) if *file == scratch.name()),
        "{loaded:?}"
    );
    assert_eq!(scratch.read(), OLD_FILE, "the only copy is still the file");
}

#[tokio::test]
async fn a_first_save_makes_the_file_and_its_folder_and_keeps_nothing() {
    let scratch = Scratch::new();
    let path = scratch.dir.path().join("ladderback").join("config.toml");
    let store = config_adapter::FileConfigStore::new(path.clone(), scratch.clock.clone());
    store.save(&expected()).await.expect("saved");

    assert_eq!(
        std::fs::read_to_string(&path).expect("the file"),
        render(&path, &expected()).expect("written")
    );
    assert_eq!(mode_of(&path), 0o600, "the file may hold a password");
    assert_eq!(mode_of(path.parent().expect("a folder")), 0o700);
    assert_eq!(
        std::fs::read_dir(path.parent().expect("a folder"))
            .expect("the folder")
            .count(),
        1,
        "nothing but the file: no history of nothing, no temporary file"
    );
    assert_eq!(store.load().await, Ok(expected()));
}

#[tokio::test]
async fn a_save_over_a_file_keeps_that_file_in_the_history() {
    let scratch = Scratch::new();
    let store = scratch.store();
    store.save(&with_concurrency(2)).await.expect("saved");
    let first = scratch.read_bytes();
    assert!(
        scratch.history().is_empty(),
        "the first save has nothing to keep"
    );

    scratch.clock.advance(domain::SignedDuration::from_secs(1));
    store.save(&with_concurrency(3)).await.expect("saved");
    assert_eq!(store.load().await, Ok(with_concurrency(3)));
    assert_eq!(scratch.entries(), ["config.toml", "history"]);
    let kept = scratch.history();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, first);
    assert_eq!(
        kept[0].0, "20261108T132022.123Z-00.toml",
        "named for the time of the save that replaced it, in UTC"
    );
    assert_eq!(mode_of(&scratch.history_dir()), 0o700);
    assert_eq!(mode_of(&scratch.history_dir().join(&kept[0].0)), 0o600);
    assert_eq!(mode_of(&scratch.path), 0o600);
}

#[tokio::test]
async fn saves_in_the_same_millisecond_are_kept_apart_and_sort_in_order() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut written = Vec::new();
    for n in 1..=4 {
        store.save(&with_concurrency(n)).await.expect("saved");
        written.push(scratch.read_bytes());
    }
    let kept = scratch.history();
    let names: Vec<_> = kept.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "20261108T132021.123Z-00.toml",
            "20261108T132021.123Z-01.toml",
            "20261108T132021.123Z-02.toml"
        ]
    );
    let contents: Vec<_> = kept.into_iter().map(|(_, bytes)| bytes).collect();
    assert_eq!(contents, written[..3], "oldest first, each as it was");
}

#[tokio::test]
async fn only_the_ten_newest_versions_are_kept() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut written = Vec::new();
    for n in 1..=14 {
        store.save(&with_concurrency(n)).await.expect("saved");
        written.push(scratch.read_bytes());
        scratch.clock.advance(domain::SignedDuration::from_secs(1));
    }
    let kept: Vec<_> = scratch
        .history()
        .into_iter()
        .map(|(_, bytes)| bytes)
        .collect();
    assert_eq!(kept.len(), config_adapter::KEPT_VERSIONS);
    // Saves 2 to 14 each kept the version before; the first three are gone.
    assert_eq!(kept, written[3..13], "versions 4 to 13, oldest first");
}

#[tokio::test]
async fn a_save_over_a_file_nobody_could_read_still_keeps_it() {
    let scratch = Scratch::new();
    scratch.write_bytes(b"\xff\xfe not a config at all");
    scratch
        .store()
        .save(&with_concurrency(2))
        .await
        .expect("saved");
    let kept = scratch.history();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, b"\xff\xfe not a config at all");
    assert_eq!(scratch.store().load().await, Ok(with_concurrency(2)));
}

#[tokio::test]
async fn a_save_that_would_make_the_file_too_large_changes_nothing() {
    let scratch = Scratch::new();
    let store = scratch.store();
    store.save(&with_concurrency(2)).await.expect("saved");
    let before = scratch.read_bytes();

    let mut plan = domain::planning::Plan::new(
        domain::PlanId::parse("home").expect("an ID"),
        domain::RepositoryAlias::parse("nas").expect("an alias"),
        vec![domain::planning::Source::parse("/home/demo").expect("a source")],
    );
    plan.label = Some("x".repeat(usize::try_from(MAX_CONFIG_BYTES).expect("fits")));
    let huge = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(
            plan.repository.clone(),
            expected()
                .repositories()
                .values()
                .next()
                .expect("one")
                .clone(),
        )]),
        vec![plan],
    )
    .expect("valid");
    assert_eq!(
        store.save(&huge).await,
        Err(ConfigError::TooLarge {
            file: scratch.name(),
            limit: MAX_CONFIG_BYTES
        })
    );
    assert_eq!(scratch.read_bytes(), before);
    assert!(scratch.history().is_empty());
}

#[tokio::test]
async fn a_save_is_refused_when_the_old_file_cannot_be_kept() {
    let scratch = Scratch::new();
    let store = scratch.store();
    store.save(&with_concurrency(2)).await.expect("saved");
    let before = scratch.read_bytes();
    std::fs::write(scratch.history_dir(), "not a folder").expect("a file");

    let saved = store.save(&with_concurrency(3)).await;
    assert!(
        matches!(&saved, Err(ConfigError::Unavailable { file, .. }) if *file == scratch.name()),
        "{saved:?}"
    );
    assert_eq!(scratch.read_bytes(), before, "the file is as it was");
}

#[tokio::test]
async fn a_save_leaves_no_temporary_file_behind() {
    let scratch = Scratch::new();
    let store = scratch.store();
    for n in 1..=3 {
        store.save(&with_concurrency(n)).await.expect("saved");
    }
    assert_eq!(scratch.entries(), ["config.toml", "history"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_load_never_sees_a_save_half_written() {
    let scratch = Scratch::new();
    let store = std::sync::Arc::new(scratch.store());
    store.save(&expected()).await.expect("saved");
    let writer = {
        let store = store.clone();
        tokio::spawn(async move {
            for n in 1..=40 {
                store
                    .save(&with_concurrency(1 + n % 8))
                    .await
                    .expect("saved");
            }
        })
    };
    let reader = {
        let store = store.clone();
        tokio::spawn(async move {
            for _ in 0..200 {
                let loaded = store.load().await;
                assert!(loaded.is_ok(), "{loaded:?}");
            }
        })
    };
    writer.await.expect("the writer");
    reader.await.expect("the reader");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saves_made_at_once_all_land_whole_and_each_keeps_what_it_replaced() {
    let scratch = Scratch::new();
    let store = std::sync::Arc::new(scratch.store());
    store.save(&with_concurrency(1)).await.expect("saved");
    let saves: Vec<_> = (2..=9)
        .map(|n| {
            let store = store.clone();
            tokio::spawn(async move { store.save(&with_concurrency(n)).await })
        })
        .collect();
    for save in saves {
        assert_eq!(save.await.expect("the task"), Ok(()));
    }
    // Eight saves over the first one: each kept the file it replaced, so the
    // history has the first version and seven of the eight that followed, and
    // the file is the eighth, whichever it was.
    let kept = scratch.history();
    assert_eq!(kept.len(), 8, "{kept:?}");
    let seen = |bytes: &[u8]| {
        let text = String::from_utf8(bytes.to_vec()).expect("text");
        let parsed = parse(std::path::Path::new("kept"), &text, &Schemas::production())
            .expect("a kept version is a whole config");
        parsed.config.agent().concurrency.get()
    };
    let mut ids: Vec<_> = kept.iter().map(|(_, bytes)| seen(bytes)).collect();
    ids.push(seen(&scratch.read_bytes()));
    ids.sort_unstable();
    assert_eq!(
        ids,
        (1..=9u8).collect::<Vec<_>>(),
        "every version is somewhere, once"
    );
}
