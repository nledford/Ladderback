//! `FileConfigStore` on a real folder: what loading a file does and refuses,
//! and what it leaves alone (technical design §6.1).

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use std::sync::Arc;

use app::ports::{Clock, ConfigError, ConfigStore, Problem};
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
    assert_eq!(kept[0].0, "000001-20261108T132021.123Z.toml");
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
        kept[0].0, "000001-20261108T132022.123Z.toml",
        "numbered in order of keeping, then the time of the save that replaced it, in UTC"
    );
    assert_eq!(mode_of(&scratch.history_dir()), 0o700);
    assert_eq!(mode_of(&scratch.history_dir().join(&kept[0].0)), 0o600);
    assert_eq!(mode_of(&scratch.path), 0o600);
}

#[tokio::test]
async fn a_save_over_a_file_a_person_wrote_changes_only_what_it_changes() {
    let scratch = Scratch::new();
    let written = "# By hand.\nschema = 1\n\n[agent]\nconcurrency = 2   # two at once\n";
    scratch.write(written);
    let store = scratch.store();
    assert_eq!(store.load().await, Ok(with_concurrency(2)));

    store.save(&with_concurrency(3)).await.expect("saved");
    assert_eq!(
        scratch.read(),
        written.replace("concurrency = 2 ", "concurrency = 3 ")
    );
    let kept = scratch.history();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, written.as_bytes(), "the file before, as it was");
    assert_eq!(store.load().await, Ok(with_concurrency(3)));
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
            "000001-20261108T132021.123Z.toml",
            "000002-20261108T132021.123Z.toml",
            "000003-20261108T132021.123Z.toml"
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

/// A clock the test can set to any time, back included: the fake clock only
/// moves forward, and a real one is set back by a person or by NTP.
struct SettableClock(std::sync::Mutex<domain::Timestamp>);

impl SettableClock {
    fn at(timestamp: &str) -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(
            timestamp.parse().expect("a timestamp"),
        )))
    }

    fn set(&self, timestamp: &str) {
        *self.0.lock().expect("the clock") = timestamp.parse().expect("a timestamp");
    }
}

#[async_trait::async_trait]
impl Clock for SettableClock {
    fn now(&self) -> domain::Timestamp {
        *self.0.lock().expect("the clock")
    }

    async fn sleep_until(&self, _deadline: domain::Timestamp) {}
}

#[tokio::test]
async fn a_clock_set_back_never_makes_a_save_drop_the_newest_versions() {
    let scratch = Scratch::new();
    let clock = SettableClock::at("2026-11-08T13:20:21.123Z");
    let store = config_adapter::FileConfigStore::new(scratch.path.clone(), clock.clone());
    let mut written = Vec::new();
    for n in 1..=14 {
        if n == 7 {
            clock.set("2026-11-07T09:00:00.000Z");
        }
        store.save(&with_concurrency(n)).await.expect("saved");
        written.push(scratch.read_bytes());
    }
    let kept: Vec<_> = scratch
        .history()
        .into_iter()
        .map(|(_, bytes)| bytes)
        .collect();
    // Saves 2 to 14 each kept the version before; the three oldest are gone,
    // though saves 7 to 14 were made a day before the first six.
    assert_eq!(kept, written[3..13], "versions 4 to 13, oldest first");
}

#[tokio::test]
async fn a_save_after_a_clock_set_back_keeps_the_version_it_replaced() {
    let scratch = Scratch::new();
    let clock = SettableClock::at("2026-11-08T13:20:21.123Z");
    let store = config_adapter::FileConfigStore::new(scratch.path.clone(), clock.clone());
    for n in 1..=12 {
        store.save(&with_concurrency(n)).await.expect("saved");
    }
    clock.set("2025-01-01T00:00:00.000Z");
    let replaced = scratch.read_bytes();
    store.save(&with_concurrency(13)).await.expect("saved");
    let kept = scratch.history();
    assert_eq!(kept.len(), config_adapter::KEPT_VERSIONS);
    assert_eq!(
        kept.last().map(|(_, bytes)| bytes),
        Some(&replaced),
        "the version just replaced is the newest one kept, though its time is a year earlier"
    );
}

/// A scratch folder with the config in a folder of its own, as a dotfiles
/// repository would hold it, and `config.toml` a link to it.
struct Linked {
    scratch: Scratch,
    target: std::path::PathBuf,
}

impl Linked {
    fn to(relative: &str) -> Self {
        let scratch = Scratch::new();
        let target = scratch.dir.path().join(relative);
        std::fs::create_dir_all(target.parent().expect("a folder")).expect("the target's folder");
        Self { scratch, target }
    }

    fn link_to(&self, to: &std::path::Path) {
        std::os::unix::fs::symlink(to, &self.scratch.path).expect("a symlink");
    }

    fn link_is_kept(&self, to: &std::path::Path) {
        let meta = std::fs::symlink_metadata(&self.scratch.path).expect("the link");
        assert!(meta.file_type().is_symlink(), "config.toml is still a link");
        assert_eq!(
            std::fs::read_link(&self.scratch.path).expect("the link"),
            to
        );
    }
}

#[tokio::test]
async fn a_save_through_a_symlinked_config_keeps_the_link_and_writes_its_target() {
    let linked = Linked::to("dotfiles/ladderback/config.toml");
    let scratch = &linked.scratch;
    std::fs::write(&linked.target, "schema = 1\n# mine\n").expect("the target");
    linked.link_to(&linked.target);
    let store = scratch.store();

    store.save(&with_concurrency(2)).await.expect("saved");

    linked.link_is_kept(&linked.target);
    assert_eq!(
        std::fs::read_to_string(&linked.target).expect("the target"),
        "schema = 1\n\n[agent]\nconcurrency = 2\n# mine\n",
        "the file behind the link has the new version, edited in"
    );
    assert_eq!(mode_of(&linked.target), 0o600);
    assert_eq!(store.load().await, Ok(with_concurrency(2)));
    assert_eq!(
        std::fs::read_dir(linked.target.parent().expect("a folder"))
            .expect("the target's folder")
            .count(),
        1,
        "only the target is in its folder: no temporary file, and no history"
    );
}

#[tokio::test]
async fn the_history_of_a_symlinked_config_stays_in_the_config_folder() {
    let linked = Linked::to("dotfiles/config.toml");
    let scratch = &linked.scratch;
    std::fs::write(&linked.target, OLD_FILE).expect("the target");
    linked.link_to(&linked.target);

    scratch
        .store()
        .save(&with_concurrency(2))
        .await
        .expect("saved");

    let kept = scratch.history();
    assert_eq!(kept.len(), 1, "the version the save replaced");
    assert_eq!(kept[0].1, OLD_FILE.as_bytes());
    assert!(
        !linked
            .target
            .parent()
            .expect("a folder")
            .join("history")
            .exists(),
        "a dotfiles repository doesn't get a folder of old copies of a file that may hold a password"
    );
}

#[tokio::test]
async fn a_relative_link_through_a_chain_of_links_is_followed_to_the_end() {
    let linked = Linked::to("dotfiles/real.toml");
    let scratch = &linked.scratch;
    std::fs::write(&linked.target, OLD_FILE).expect("the target");
    let middle = scratch.dir.path().join("dotfiles").join("middle.toml");
    std::os::unix::fs::symlink("real.toml", &middle).expect("a symlink");
    let first = std::path::Path::new("dotfiles/middle.toml");
    linked.link_to(first);

    scratch
        .store()
        .save(&with_concurrency(2))
        .await
        .expect("saved");

    linked.link_is_kept(first);
    assert_eq!(
        std::fs::read_link(&middle).expect("the middle link"),
        std::path::Path::new("real.toml")
    );
    assert_eq!(
        std::fs::read_to_string(&linked.target).expect("the target"),
        render(&scratch.path, &with_concurrency(2)).expect("written")
    );
}

#[tokio::test]
async fn a_save_through_a_link_to_nothing_makes_the_target_and_keeps_the_link() {
    let linked = Linked::to("dotfiles/config.toml");
    let scratch = &linked.scratch;
    linked.link_to(&linked.target);
    assert!(!linked.target.exists());

    scratch
        .store()
        .save(&with_concurrency(2))
        .await
        .expect("saved");

    linked.link_is_kept(&linked.target);
    assert_eq!(scratch.store().load().await, Ok(with_concurrency(2)));
    assert!(scratch.history().is_empty(), "nothing was there to keep");
}

#[tokio::test]
async fn a_save_through_a_link_that_loops_changes_nothing() {
    let scratch = Scratch::new();
    let other = scratch.dir.path().join("other.toml");
    std::os::unix::fs::symlink(&other, &scratch.path).expect("a symlink");
    std::os::unix::fs::symlink(&scratch.path, &other).expect("a symlink");

    let saved = scratch.store().save(&with_concurrency(2)).await;

    assert!(
        matches!(&saved, Err(ConfigError::Unavailable { file, .. }) if *file == scratch.name()),
        "{saved:?}"
    );
    assert!(scratch.history().is_empty());
    for link in [&scratch.path, &other] {
        assert!(
            std::fs::symlink_metadata(link)
                .expect("the link")
                .file_type()
                .is_symlink()
        );
    }
}
